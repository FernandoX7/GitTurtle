//! Authoritative GitHub review conversations. Connections are explicitly paged;
//! writes address captured global node identities and never infer REST grouping.
use super::*;

pub(crate) const THREAD_PAGE_SIZE: usize = 25;
pub(crate) const INITIAL_COMMENT_PAGE_SIZE: usize = 20;
pub(crate) const COMMENT_PAGE_SIZE: usize = 100;
const MAX_CURSOR: usize = 1024;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct CapturedThread {
    pub pull: CapturedPull,
    pub account: String,
    pub account_id: String,
    pub repository_id: String,
    pub pull_id: String,
    pub thread_id: String,
    pub path: String,
    pub root_comment_id: Option<String>,
    pub original_commit_id: Option<String>,
}
impl CapturedThread {
    pub fn destination(&self) -> String {
        format!(
            "github.com/{} #{} · account {} ({}) · repository {} · PR {} · {} · conversation {} · head {} · base {}",
            self.pull.repository.label(),
            self.pull.number,
            self.account,
            self.account_id,
            self.repository_id,
            self.pull_id,
            self.path,
            self.thread_id,
            self.pull.head.sha,
            self.pull.base.sha
        )
    }
    pub fn retained_bytes(&self) -> usize {
        self.account.len()
            + self.account_id.len()
            + self.repository_id.len()
            + self.pull_id.len()
            + self.thread_id.len()
            + self.path.len()
            + self.root_comment_id.as_ref().map_or(0, String::len)
            + self.original_commit_id.as_ref().map_or(0, String::len)
            + self.pull.repository.owner.len()
            + self.pull.repository.name.len()
            + self.pull.head.sha.len()
            + self.pull.head.branch.len()
            + self.pull.head.label.len()
            + self.pull.base.sha.len()
            + self.pull.base.branch.len()
            + self.pull.base.label.len()
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ThreadComment {
    pub id: String,
    pub body: String,
    pub author: String,
    pub created_at: String,
    pub url: String,
    pub reply_to: Option<String>,
    pub commit_id: Option<String>,
    pub original_commit_id: Option<String>,
}
impl ThreadComment {
    pub fn retained_bytes(&self) -> usize {
        self.id.len()
            + self.body.len()
            + self.author.len()
            + self.created_at.len()
            + self.url.len()
            + self.reply_to.as_ref().map_or(0, String::len)
            + self.commit_id.as_ref().map_or(0, String::len)
            + self.original_commit_id.as_ref().map_or(0, String::len)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Thread {
    pub target: CapturedThread,
    pub is_resolved: bool,
    pub is_outdated: bool,
    pub resolved_by: Option<String>,
    pub viewer_can_reply: bool,
    pub viewer_can_resolve: bool,
    pub viewer_can_unresolve: bool,
    pub line: Option<u32>,
    pub start_line: Option<u32>,
    pub original_line: Option<u32>,
    pub original_start_line: Option<u32>,
    pub side: DiffSide,
    pub start_side: Option<DiffSide>,
    pub comments: Vec<ThreadComment>,
    pub comments_total: u32,
    pub next_comments_cursor: Option<String>,
    /// Missing opening comment or nullable connection nodes. Additional ordinary
    /// pages are indicated separately by next_comments_cursor.
    pub context_incomplete: bool,
}
impl Thread {
    pub fn reply_available(&self) -> bool {
        self.viewer_can_reply && !self.context_incomplete && self.target.root_comment_id.is_some()
    }
    pub fn location(&self) -> String {
        let (start, line) = if self.is_outdated {
            (self.original_start_line, self.original_line)
        } else {
            (self.start_line, self.line)
        };
        let side = |side| {
            if side == DiffSide::Left {
                "Before"
            } else {
                "After"
            }
        };
        let end = line.map(|n| n.to_string()).unwrap_or_else(|| "file".into());
        let location = match (start, self.start_side) {
            (Some(start), Some(start_side)) if start_side != self.side => {
                format!("{} {start} → {} {end}", side(start_side), side(self.side))
            }
            (Some(start), Some(_)) => format!("{} {start}–{end}", side(self.side)),
            (Some(start), None) => format!("start {start} → {} {end}", side(self.side)),
            (None, _) => format!("{} {end}", side(self.side)),
        };
        format!(
            "{} · {location}{}",
            self.target.path,
            if self.is_outdated {
                " · outdated position"
            } else {
                ""
            }
        )
    }
    pub fn retained_bytes(&self) -> usize {
        self.target.retained_bytes()
            + self
                .comments
                .iter()
                .map(ThreadComment::retained_bytes)
                .sum::<usize>()
            + self.next_comments_cursor.as_ref().map_or(0, String::len)
            + self.resolved_by.as_ref().map_or(0, String::len)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct CursorPage<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
    pub total_count: u32,
}

const COMMENT_FIELDS: &str =
    "id body author { login } createdAt url replyTo { id } commit { oid } originalCommit { oid }";
const PULL_FIELDS: &str = "id number headRefOid baseRefOid headRefName baseRefName state";
fn thread_fields(comment_count: usize, paged: bool) -> String {
    format!(
        "id path diffSide startDiffSide line startLine originalLine originalStartLine isResolved isOutdated resolvedBy {{ login }} viewerCanReply viewerCanResolve viewerCanUnresolve repository {{ id nameWithOwner }} pullRequest {{ {PULL_FIELDS} }} root: comments(first: 1) {{ totalCount pageInfo {{ hasNextPage endCursor }} nodes {{ {COMMENT_FIELDS} }} }} comments(first: {comment_count}{}) {{ totalCount pageInfo {{ hasNextPage endCursor }} nodes {{ {COMMENT_FIELDS} }} }}",
        if paged { ", after: $after" } else { "" }
    )
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawPageInfo {
    has_next_page: bool,
    end_cursor: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawConnection<T> {
    nodes: Vec<Option<T>>,
    total_count: u32,
    page_info: RawPageInfo,
}
impl<T> RawConnection<T> {
    fn validate(&self, limit: usize, after: Option<&str>) -> Result<Option<String>> {
        ensure!(
            self.nodes.len() <= limit && self.nodes.len() <= self.total_count as usize,
            "GitHub returned an invalid conversation page size"
        );
        if self.page_info.has_next_page {
            let next = self.page_info.end_cursor.as_deref().ok_or_else(|| {
                anyhow!("GitHub omitted the next conversation cursor; refresh explicitly")
            })?;
            validate_cursor(Some(next))?;
            ensure!(
                Some(next) != after
                    && !self.nodes.is_empty()
                    && self.total_count as usize > self.nodes.len(),
                "GitHub returned a repeated or empty conversation page; refresh explicitly"
            );
            Ok(Some(next.into()))
        } else {
            ensure!(
                after.is_some() || self.nodes.len() == self.total_count as usize,
                "GitHub omitted conversation context without a continuation cursor; refresh explicitly"
            );
            Ok(None)
        }
    }
}
#[derive(Deserialize)]
struct RawNode {
    id: String,
}
#[derive(Deserialize)]
struct RawActor {
    login: String,
}
#[derive(Deserialize)]
struct RawViewer {
    id: String,
    login: String,
}
#[derive(Deserialize)]
struct RawCommit {
    oid: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawComment {
    id: String,
    body: String,
    author: Option<RawActor>,
    created_at: String,
    url: String,
    reply_to: Option<RawNode>,
    commit: Option<RawCommit>,
    original_commit: Option<RawCommit>,
}
impl RawComment {
    fn model(self) -> Result<ThreadComment> {
        validate_id(&self.id)?;
        validate_text(&self.body, false)?;
        ensure!(
            self.created_at.len() <= 100
                && self.url.len() <= 4096
                && self.url.starts_with("https://github.com/"),
            "GitHub returned invalid conversation metadata"
        );
        if let Some(reply) = &self.reply_to {
            validate_id(&reply.id)?;
        }
        for commit in self.commit.iter().chain(self.original_commit.iter()) {
            ensure!(
                valid_oid(&commit.oid),
                "GitHub returned an invalid conversation commit"
            );
        }
        let author = self
            .author
            .map(|author| author.login)
            .unwrap_or_else(|| "Deleted account".into());
        ensure!(
            author.len() <= 100,
            "GitHub returned invalid conversation author metadata"
        );
        Ok(ThreadComment {
            id: self.id,
            body: self.body,
            author,
            created_at: self.created_at,
            url: self.url,
            reply_to: self.reply_to.map(|node| node.id),
            commit_id: self.commit.map(|commit| commit.oid),
            original_commit_id: self.original_commit.map(|commit| commit.oid),
        })
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawRepository {
    id: String,
    name_with_owner: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawPull {
    id: String,
    number: u64,
    head_ref_oid: String,
    base_ref_oid: String,
    head_ref_name: String,
    base_ref_name: String,
    state: String,
}
fn validate_pull(
    repository: &RawRepository,
    pull: &RawPull,
    captured: &CapturedPull,
) -> Result<()> {
    validate_id(&repository.id)?;
    validate_id(&pull.id)?;
    ensure!(
        repository
            .name_with_owner
            .eq_ignore_ascii_case(&captured.repository.label())
            && pull.number == captured.number,
        "GitHub returned another repository or pull request; no target was remapped"
    );
    ensure!(
        pull.head_ref_oid == captured.head.sha
            && pull.base_ref_oid == captured.base.sha
            && pull.head_ref_name == captured.head.branch
            && pull.base_ref_name == captured.base.branch,
        "This pull request head or base moved. Your reply draft retains its original conversation and commits; refresh explicitly to review the new context"
    );
    ensure!(
        pull.state == "OPEN",
        "This pull request is no longer open. Your draft is retained"
    );
    Ok(())
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawThread {
    id: String,
    path: String,
    diff_side: DiffSide,
    start_diff_side: Option<DiffSide>,
    line: Option<u32>,
    start_line: Option<u32>,
    original_line: Option<u32>,
    original_start_line: Option<u32>,
    is_resolved: bool,
    is_outdated: bool,
    resolved_by: Option<RawActor>,
    viewer_can_reply: bool,
    viewer_can_resolve: bool,
    viewer_can_unresolve: bool,
    repository: RawRepository,
    pull_request: RawPull,
    root: RawConnection<RawComment>,
    comments: RawConnection<RawComment>,
}
impl RawThread {
    fn model(
        self,
        captured: &CapturedPull,
        viewer: &RawViewer,
        limit: usize,
        after: Option<&str>,
    ) -> Result<Thread> {
        validate_id(&self.id)?;
        validate_id(&viewer.id)?;
        ensure!(
            !viewer.login.is_empty() && viewer.login.len() <= 100 && safe_path(&self.path),
            "GitHub returned invalid account or conversation path metadata"
        );
        validate_pull(&self.repository, &self.pull_request, captured)?;
        for line in [
            self.line,
            self.start_line,
            self.original_line,
            self.original_start_line,
        ]
        .into_iter()
        .flatten()
        {
            ensure!(line > 0, "GitHub returned an invalid conversation position");
        }
        self.root.validate(1, None)?;
        let next_comments_cursor = self.comments.validate(limit, after)?;
        let root = self
            .root
            .nodes
            .into_iter()
            .next()
            .flatten()
            .map(RawComment::model)
            .transpose()?;
        let context_incomplete = root.as_ref().is_none_or(|root| root.reply_to.is_some())
            || self.comments.nodes.iter().any(Option::is_none);
        let target = CapturedThread {
            pull: captured.clone(),
            account: viewer.login.clone(),
            account_id: viewer.id.clone(),
            repository_id: self.repository.id,
            pull_id: self.pull_request.id,
            thread_id: self.id,
            path: self.path,
            root_comment_id: root
                .as_ref()
                .map(|root| root.reply_to.clone().unwrap_or_else(|| root.id.clone())),
            original_commit_id: root
                .as_ref()
                .and_then(|root| root.original_commit_id.clone()),
        };
        let comments = self
            .comments
            .nodes
            .into_iter()
            .flatten()
            .map(RawComment::model)
            .collect::<Result<Vec<_>>>()?;
        let mut ids = std::collections::HashSet::new();
        ensure!(
            comments.iter().all(|comment| ids.insert(&comment.id)),
            "GitHub returned duplicate comments in one conversation page"
        );
        Ok(Thread {
            target,
            is_resolved: self.is_resolved,
            is_outdated: self.is_outdated,
            resolved_by: self.resolved_by.map(|actor| actor.login),
            viewer_can_reply: self.viewer_can_reply,
            viewer_can_resolve: self.viewer_can_resolve,
            viewer_can_unresolve: self.viewer_can_unresolve,
            line: self.line,
            start_line: self.start_line,
            original_line: self.original_line,
            original_start_line: self.original_start_line,
            side: self.diff_side,
            start_side: self.start_diff_side,
            comments,
            comments_total: self.comments.total_count,
            next_comments_cursor,
            context_incomplete,
        })
    }
}

fn validate_id(id: &str) -> Result<()> {
    ensure!(
        !id.is_empty() && id.len() <= 1024 && id.bytes().all(|byte| byte.is_ascii_graphic()),
        "GitHub conversation identity is unavailable or invalid"
    );
    Ok(())
}
fn validate_cursor(cursor: Option<&str>) -> Result<()> {
    ensure!(
        cursor.is_none_or(|cursor| !cursor.is_empty()
            && cursor.len() <= MAX_CURSOR
            && cursor.bytes().all(|byte| byte.is_ascii_graphic())),
        "GitHub conversation cursor is invalid"
    );
    Ok(())
}
fn decode<T: for<'de> Deserialize<'de>>(value: Value) -> Result<T> {
    serde_json::from_value(value).map_err(|_| {
        anyhow!("GitHub conversation context is incomplete or unsupported; refresh explicitly")
    })
}

impl<T: Transport> Client<T> {
    fn graphql(
        &mut self,
        query: String,
        variables: Value,
        writing: bool,
        control: &OperationControl,
    ) -> Result<Value> {
        ensure!(
            !control.is_cancelled(),
            "GitHub action cancelled before submission"
        );
        let response = self.0.request(Request { method:"POST", endpoint:"graphql".into(),
            body:Some(json!({"query":query,"variables":variables})) }, control).map_err(|error| {
                if writing { anyhow!("{error}. The GitHub outcome may be uncertain. Check the conversation before resubmitting; no automatic retry was made.") }
                else { error }
            })?;
        response.check(writing)?;
        let envelope: Value = response.json().map_err(|_| anyhow!(if writing {
            "GitHub returned an unreadable mutation response. The outcome may be uncertain; check the conversation before resubmitting."
        } else { "GitHub returned unreadable conversation context; refresh explicitly" }))?;
        if envelope
            .get("errors")
            .is_some_and(|errors| !matches!(errors.as_array(), Some(errors) if errors.is_empty()))
        {
            let errors = envelope["errors"].as_array();
            let typed = |kind: &str| {
                errors.is_some_and(|errors| {
                    errors.iter().any(|error| {
                        error["type"].as_str() == Some(kind)
                            || error["extensions"]["type"].as_str() == Some(kind)
                            || error["extensions"]["code"].as_str() == Some(kind)
                    })
                })
            };
            let guidance = if typed("RATE_LIMITED")
                || response
                    .headers
                    .get("x-ratelimit-remaining")
                    .is_some_and(|value| value == "0")
                || response.headers.contains_key("retry-after")
            {
                let wait = response
                    .headers
                    .get("retry-after")
                    .map(|value| format!(" Retry after {value} seconds."))
                    .or_else(|| {
                        response
                            .headers
                            .get("x-ratelimit-reset")
                            .map(|value| format!(" Rate limit resets at Unix time {value}."))
                    })
                    .unwrap_or_default();
                format!("GitHub rate limit reached.{wait}")
            } else if typed("FORBIDDEN") {
                "GitHub refused this conversation action. Check repository access and review permissions.".into()
            } else if typed("UNAUTHORIZED") {
                "GitHub authorization expired or was rejected. Reconnect the account.".into()
            } else if typed("NOT_FOUND") {
                "This GitHub conversation is unavailable to the connected account.".into()
            } else {
                "GitHub could not establish complete conversation context or accept the reviewed action.".into()
            };
            bail!(
                "{guidance}{}",
                if writing {
                    " The outcome may be uncertain. Check the conversation before resubmitting; no automatic retry was made."
                } else {
                    " Refresh explicitly to try again."
                }
            );
        }
        envelope.get("data").filter(|value| value.is_object()).cloned().ok_or_else(|| anyhow!(if writing {
            "GitHub returned no mutation identity. The outcome may be uncertain; check the conversation before resubmitting."
        } else { "GitHub conversation context is unavailable; refresh explicitly" }))
    }

    pub fn review_threads(
        &mut self,
        pull: &CapturedPull,
        after: Option<&str>,
        control: &OperationControl,
    ) -> Result<CursorPage<Thread>> {
        validate_cursor(after)?;
        let query = format!(
            "query GitTurtleReviewThreads($owner: String!, $name: String!, $number: Int!, $after: String) {{ viewer {{ id login }} repository(owner: $owner, name: $name) {{ id nameWithOwner pullRequest(number: $number) {{ {PULL_FIELDS} reviewThreads(first: {THREAD_PAGE_SIZE}, after: $after) {{ totalCount pageInfo {{ hasNextPage endCursor }} nodes {{ {} }} }} }} }} }}",
            thread_fields(INITIAL_COMMENT_PAGE_SIZE, false)
        );
        let data = self.graphql(query, json!({"owner":pull.repository.owner,"name":pull.repository.name,"number":pull.number,"after":after}), false, control)?;
        let viewer: RawViewer = decode(data["viewer"].clone())?;
        let repository: RawRepository = decode(data["repository"].clone())?;
        let current: RawPull = decode(data["repository"]["pullRequest"].clone())?;
        validate_pull(&repository, &current, pull)?;
        let page: RawConnection<RawThread> =
            decode(data["repository"]["pullRequest"]["reviewThreads"].clone())?;
        let next_cursor = page.validate(THREAD_PAGE_SIZE, after)?;
        ensure!(
            page.nodes.iter().all(Option::is_some),
            "Some GitHub conversations are unavailable; this page is incomplete. Refresh explicitly"
        );
        let items = page
            .nodes
            .into_iter()
            .flatten()
            .map(|thread| thread.model(pull, &viewer, INITIAL_COMMENT_PAGE_SIZE, None))
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            items
                .iter()
                .all(|thread| thread.target.repository_id == repository.id
                    && thread.target.pull_id == current.id),
            "GitHub returned inconsistent repository or pull request identities; no conversation target was captured"
        );
        let mut ids = std::collections::HashSet::new();
        ensure!(
            items
                .iter()
                .all(|thread| ids.insert(&thread.target.thread_id)),
            "GitHub returned duplicate conversation identities"
        );
        Ok(CursorPage {
            items,
            next_cursor,
            total_count: page.total_count,
        })
    }

    pub fn thread_comments(
        &mut self,
        target: &CapturedThread,
        after: Option<&str>,
        control: &OperationControl,
    ) -> Result<Thread> {
        validate_id(&target.thread_id)?;
        validate_cursor(after)?;
        let query = format!(
            "query GitTurtleThreadComments($thread: ID!, $after: String) {{ viewer {{ id login }} node(id: $thread) {{ ... on PullRequestReviewThread {{ {} }} }} }}",
            thread_fields(COMMENT_PAGE_SIZE, true)
        );
        let data = self.graphql(
            query,
            json!({"thread":target.thread_id,"after":after}),
            false,
            control,
        )?;
        let viewer: RawViewer = decode(data["viewer"].clone())?;
        let raw: RawThread = decode(data["node"].clone())?;
        let thread = raw.model(&target.pull, &viewer, COMMENT_PAGE_SIZE, after)?;
        ensure!(
            thread.target == *target,
            "The connected account or original conversation identity changed. Your reply draft is retained for explicit recovery; no target was remapped"
        );
        Ok(thread)
    }

    pub(super) fn reply_to_thread(
        &mut self,
        target: &CapturedThread,
        body: &str,
        control: &OperationControl,
    ) -> Result<String> {
        validate_text(body, true)?;
        let thread = self.thread_comments(target, None, control)?;
        ensure!(
            thread.reply_available(),
            "GitHub does not permit replying to this conversation, or its opening context is unavailable. Your draft is retained"
        );
        let query = "mutation GitTurtleReply($thread: ID!, $body: String!) { addPullRequestReviewThreadReply(input: {pullRequestReviewThreadId: $thread, body: $body}) { comment { id url body replyTo { id } pullRequest { id number repository { id nameWithOwner } } } } }";
        let data = self.graphql(
            query.into(),
            json!({"thread":target.thread_id,"body":body}),
            true,
            control,
        )?;
        let comment = &data["addPullRequestReviewThreadReply"]["comment"];
        let valid = comment["id"]
            .as_str()
            .is_some_and(|id| validate_id(id).is_ok())
            && comment["body"].as_str() == Some(body)
            && comment["replyTo"]["id"].as_str() == target.root_comment_id.as_deref()
            && comment["pullRequest"]["id"].as_str() == Some(&target.pull_id)
            && comment["pullRequest"]["number"].as_u64() == Some(target.pull.number)
            && comment["pullRequest"]["repository"]["id"].as_str() == Some(&target.repository_id);
        ensure!(
            valid,
            "GitHub returned an incomplete or mismatched reply identity. The outcome may be uncertain; check the conversation before resubmitting. No automatic retry was made"
        );
        Ok(format!(
            "GitHub accepted reply {} at {}",
            comment["id"].as_str().unwrap_or_default(),
            target.destination()
        ))
    }

    pub(super) fn set_thread_resolved(
        &mut self,
        target: &CapturedThread,
        resolved: bool,
        control: &OperationControl,
    ) -> Result<String> {
        let thread = self.thread_comments(target, None, control)?;
        ensure!(
            thread.is_resolved != resolved,
            "This conversation is already {}. Refresh explicitly before choosing another action",
            if resolved { "resolved" } else { "open" }
        );
        ensure!(
            if resolved {
                thread.viewer_can_resolve
            } else {
                thread.viewer_can_unresolve
            },
            "GitHub refused permission to {} this conversation",
            if resolved { "resolve" } else { "reopen" }
        );
        let mutation = if resolved {
            "resolveReviewThread"
        } else {
            "unresolveReviewThread"
        };
        let query = format!(
            "mutation GitTurtleThreadResolution($thread: ID!) {{ {mutation}(input: {{threadId: $thread}}) {{ thread {{ id isResolved repository {{ id nameWithOwner }} pullRequest {{ id number }} }} }} }}"
        );
        let data = self.graphql(query, json!({"thread":target.thread_id}), true, control)?;
        let returned = &data[mutation]["thread"];
        ensure!(
            returned["id"].as_str() == Some(&target.thread_id)
                && returned["isResolved"].as_bool() == Some(resolved)
                && returned["repository"]["id"].as_str() == Some(&target.repository_id)
                && returned["pullRequest"]["id"].as_str() == Some(&target.pull_id)
                && returned["pullRequest"]["number"].as_u64() == Some(target.pull.number),
            "GitHub returned an incomplete or mismatched conversation state. The outcome may be uncertain; check the conversation before resubmitting. No automatic retry was made"
        );
        Ok(format!(
            "GitHub {} the conversation at {}",
            if resolved { "resolved" } else { "reopened" },
            target.destination()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    #[derive(Default)]
    struct Mock {
        requests: Vec<Request>,
        responses: VecDeque<Result<Response>>,
        cancel_after_read: bool,
    }
    impl Transport for Mock {
        fn request(&mut self, request: Request, control: &OperationControl) -> Result<Response> {
            self.requests.push(request);
            if self.cancel_after_read {
                control.cancel();
            }
            self.responses
                .pop_front()
                .expect("unexpected extra network request")
        }
    }
    fn response(value: Value) -> Result<Response> {
        Ok(Response {
            status: 200,
            headers: BTreeMap::new(),
            body: serde_json::to_vec(&value).unwrap(),
        })
    }
    fn pull() -> CapturedPull {
        CapturedPull {
            repository: Repository::parse("fixture/repo").unwrap(),
            number: 7,
            head: Revision {
                sha: "1".repeat(40),
                branch: "topic".into(),
                label: "fixture:topic".into(),
            },
            base: Revision {
                sha: "2".repeat(40),
                branch: "main".into(),
                label: "fixture:main".into(),
            },
        }
    }
    fn target() -> CapturedThread {
        CapturedThread {
            pull: pull(),
            account: "reviewer".into(),
            account_id: "U_review".into(),
            repository_id: "R_fixture".into(),
            pull_id: "PR_fixture".into(),
            thread_id: "PRRT_fixture".into(),
            path: "src/界面.rs".into(),
            root_comment_id: Some("PRRC_root".into()),
            original_commit_id: Some("1".repeat(40)),
        }
    }
    fn comment(id: &str, reply: bool) -> Value {
        json!({"id":id,"body":"Exact comment\n","author":{"login":"reviewer"},"createdAt":"2026-09-10T10:00:00Z",
            "url":"https://github.com/fixture/repo/pull/7#discussion_r1", "replyTo":if reply {json!({"id":"PRRC_root"})} else {Value::Null},
            "commit":{"oid":"1".repeat(40)},"originalCommit":{"oid":"1".repeat(40)}})
    }
    fn connection(nodes: Vec<Value>, total: u32, next: Option<&str>) -> Value {
        json!({"nodes":nodes,"totalCount":total,"pageInfo":{"hasNextPage":next.is_some(),"endCursor":next}})
    }
    fn thread() -> Value {
        json!({"id":"PRRT_fixture","path":"src/界面.rs","diffSide":"RIGHT","startDiffSide":"RIGHT","line":4,"startLine":3,
            "originalLine":4,"originalStartLine":3,"isResolved":false,"isOutdated":false,"resolvedBy":null,
            "viewerCanReply":true,"viewerCanResolve":true,"viewerCanUnresolve":true,
            "repository":{"id":"R_fixture","nameWithOwner":"fixture/repo"},
            "pullRequest":{"id":"PR_fixture","number":7,"headRefOid":"1".repeat(40),"baseRefOid":"2".repeat(40),
                "headRefName":"topic","baseRefName":"main","state":"OPEN"},
            "root":connection(vec![comment("PRRC_root",false)],2,Some("root_cursor")),
            "comments":connection(vec![comment("PRRC_root",false),comment("PRRC_reply",true)],2,None)})
    }
    fn node_response(node: Value) -> Value {
        json!({"data":{"viewer":{"id":"U_review","login":"reviewer"},"node":node}})
    }
    fn page_response(nodes: Vec<Value>, next: Option<&str>) -> Value {
        let total = if next.is_some() {
            40
        } else {
            nodes.len() as u32
        };
        let mut repository = thread()["repository"].clone();
        repository["pullRequest"] = thread()["pullRequest"].clone();
        repository["pullRequest"]["reviewThreads"] = connection(nodes, total, next);
        json!({"data":{"viewer":{"id":"U_review","login":"reviewer"},"repository":repository}})
    }
    fn client(values: impl IntoIterator<Item = Value>) -> Client<Mock> {
        Client(Mock {
            responses: values.into_iter().map(response).collect(),
            ..Default::default()
        })
    }
    fn reply_result(body: &str) -> Value {
        json!({"data":{"addPullRequestReviewThreadReply":{"comment":{"id":"PRRC_new","body":body,
            "url":"https://github.com/fixture/repo/pull/7#discussion_r3","replyTo":{"id":"PRRC_root"},
            "pullRequest":{"id":"PR_fixture","number":7,"repository":{"id":"R_fixture","nameWithOwner":"fixture/repo"}}}}}})
    }
    fn resolution_result(resolved: bool) -> Value {
        let field = if resolved {
            "resolveReviewThread"
        } else {
            "unresolveReviewThread"
        };
        json!({"data":{field:{"thread":{"id":"PRRT_fixture","isResolved":resolved,
            "repository":{"id":"R_fixture","nameWithOwner":"fixture/repo"},"pullRequest":{"id":"PR_fixture","number":7}}}}})
    }
    fn writes(client: &Client<Mock>) -> usize {
        client
            .0
            .requests
            .iter()
            .filter(|request| {
                request.body.as_ref().unwrap()["query"]
                    .as_str()
                    .unwrap()
                    .starts_with("mutation")
            })
            .count()
    }

    #[test]
    fn authoritative_threads_keep_resolution_original_identity_and_explicit_cursor() {
        let mut outdated = thread();
        outdated["isResolved"] = json!(true);
        outdated["isOutdated"] = json!(true);
        outdated["resolvedBy"] = json!({"login":"maintainer"});
        outdated["line"] = Value::Null;
        let mut client = client([page_response(vec![outdated], Some("next_cursor"))]);
        let page = client
            .review_threads(
                &pull(),
                Some("previous_cursor"),
                &OperationControl::default(),
            )
            .unwrap();
        assert_eq!(page.next_cursor.as_deref(), Some("next_cursor"));
        assert_eq!(page.items[0].target, target());
        assert!(page.items[0].is_resolved && page.items[0].is_outdated);
        assert_eq!(page.items[0].resolved_by.as_deref(), Some("maintainer"));
        assert!(page.items[0].location().contains("3–4 · outdated"));
        assert_eq!(client.0.requests.len(), 1);
        let request = &client.0.requests[0];
        assert_eq!(request.endpoint, "graphql");
        assert_eq!(
            request.body.as_ref().unwrap()["variables"]["after"],
            "previous_cursor"
        );
    }

    #[test]
    fn comment_continuation_checks_opening_context_without_reassigning_target() {
        let mut node = thread();
        node["comments"] = connection(vec![comment("PRRC_later", true)], 101, Some("later_cursor"));
        let mut client = client([node_response(node)]);
        let continued = client
            .thread_comments(
                &target(),
                Some("first_cursor"),
                &OperationControl::default(),
            )
            .unwrap();
        assert_eq!(continued.target, target());
        assert_eq!(continued.comments[0].id, "PRRC_later");
        assert_eq!(
            continued.next_comments_cursor.as_deref(),
            Some("later_cursor")
        );
        assert!(continued.reply_available());
        assert_eq!(client.0.requests.len(), 1);
    }

    #[test]
    fn external_cross_side_ranges_keep_original_side_and_reject_inconsistent_node_membership() {
        let mut node = thread();
        node["startDiffSide"] = json!("LEFT");
        node["isOutdated"] = json!(true);
        let mut client = client([page_response(vec![node], None)]);
        let page = client
            .review_threads(&pull(), None, &OperationControl::default())
            .unwrap();
        assert!(
            page.items[0]
                .location()
                .contains("Before 3 → After 4 · outdated")
        );
        for field in ["repository", "pullRequest"] {
            let mut page = page_response(vec![thread()], None);
            page["data"]["repository"]["pullRequest"]["reviewThreads"]["nodes"][0][field]["id"] =
                json!("different_node");
            let mut client = self::client([page]);
            assert!(
                client
                    .review_threads(&pull(), None, &OperationControl::default())
                    .unwrap_err()
                    .to_string()
                    .contains("inconsistent")
            );
            assert_eq!(client.0.requests.len(), 1);
        }
    }

    #[test]
    fn null_comment_context_is_readable_but_cannot_be_replied_to() {
        let mut node = thread();
        node["root"] = connection(vec![Value::Null], 2, Some("root_cursor"));
        node["comments"]["nodes"][0] = Value::Null;
        let mut client = client([page_response(vec![node], None)]);
        let page = client
            .review_threads(&pull(), None, &OperationControl::default())
            .unwrap();
        assert!(page.items[0].context_incomplete);
        assert!(!page.items[0].reply_available());
        assert_eq!(page.items[0].comments.len(), 1);
        assert!(page.items[0].target.root_comment_id.is_none());
    }

    #[test]
    fn malformed_partial_or_cyclic_pages_never_establish_thread_state() {
        let mut repeated = page_response(vec![thread()], Some("cursor"));
        let mut missing_state = page_response(vec![thread()], None);
        missing_state["data"]["repository"]["pullRequest"]["reviewThreads"]["nodes"][0]
            .as_object_mut()
            .unwrap()
            .remove("isResolved");
        let mut partial = page_response(vec![thread()], None);
        partial["errors"] = json!([{"type":"INTERNAL","message":"private diagnostic"}]);
        let mut null = page_response(vec![thread()], None);
        null["data"]["repository"]["pullRequest"]["reviewThreads"]["nodes"][0] = Value::Null;
        for value in [repeated.take(), missing_state, partial, null] {
            let mut client = client([value]);
            let error = client
                .review_threads(&pull(), Some("cursor"), &OperationControl::default())
                .unwrap_err();
            assert!(!error.to_string().contains("private diagnostic"));
            assert_eq!(client.0.requests.len(), 1);
        }
        let mut client = Client(Mock::default());
        assert!(
            client
                .review_threads(
                    &pull(),
                    Some(&"x".repeat(MAX_CURSOR + 1)),
                    &OperationControl::default()
                )
                .is_err()
        );
        assert!(client.0.requests.is_empty());
    }

    #[test]
    fn stale_account_repository_pull_head_and_root_refuse_reply_before_write() {
        let variants: Vec<Vec<&str>> = vec![
            vec!["viewer", "id"],
            vec!["viewer", "login"],
            vec!["node", "id"],
            vec!["node", "repository", "id"],
            vec!["node", "repository", "nameWithOwner"],
            vec!["node", "pullRequest", "id"],
            vec!["node", "pullRequest", "headRefOid"],
            vec!["node", "pullRequest", "baseRefOid"],
        ];
        for path in variants {
            let mut value = node_response(thread());
            let mut part = &mut value["data"];
            for name in path {
                part = &mut part[name];
            }
            *part = json!("changed_identity");
            let mut client = client([value]);
            assert!(
                client
                    .execute(
                        Action::Reply {
                            target: target(),
                            body: "Draft\n".into()
                        },
                        &OperationControl::default()
                    )
                    .is_err()
            );
            assert_eq!(writes(&client), 0);
        }
        let mut root_changed = thread();
        root_changed["root"]["nodes"][0]["id"] = json!("other_root");
        let mut client = client([node_response(root_changed)]);
        assert!(
            client
                .reply_to_thread(&target(), "Draft", &OperationControl::default())
                .is_err()
        );
        assert_eq!(writes(&client), 0);
    }

    #[test]
    fn reply_and_resolve_reopen_send_one_exact_mutation_and_validate_provider_state() {
        let body = "Exact text\n\n界面 remains attached.\n";
        let mut client = client([node_response(thread()), reply_result(body)]);
        client
            .execute(
                Action::Reply {
                    target: target(),
                    body: body.into(),
                },
                &OperationControl::default(),
            )
            .unwrap();
        assert_eq!(writes(&client), 1);
        let body_value = client.0.requests[1].body.as_ref().unwrap();
        assert_eq!(body_value["variables"]["thread"], "PRRT_fixture");
        assert_eq!(body_value["variables"]["body"], body);
        assert!(
            !body_value["query"]
                .as_str()
                .unwrap()
                .contains("pullRequestReviewId:")
        );
        for resolved in [true, false] {
            let mut node = thread();
            node["isResolved"] = json!(!resolved);
            let mut client = self::client([node_response(node), resolution_result(resolved)]);
            client
                .execute(
                    Action::SetThreadResolved {
                        target: target(),
                        resolved,
                    },
                    &OperationControl::default(),
                )
                .unwrap();
            assert_eq!(writes(&client), 1);
        }
    }

    #[test]
    fn denied_permission_closed_pull_and_cancellation_dispatch_no_mutation() {
        for (field, value) in [("viewerCanReply", json!(false)), ("state", json!("CLOSED"))] {
            let mut node = thread();
            if field == "state" {
                node["pullRequest"][field] = value;
            } else {
                node[field] = value;
            }
            let mut client = client([node_response(node)]);
            assert!(
                client
                    .reply_to_thread(&target(), "Draft", &OperationControl::default())
                    .is_err()
            );
            assert_eq!(writes(&client), 0);
        }
        let mut node = thread();
        node["viewerCanResolve"] = json!(false);
        let mut denied = client([node_response(node)]);
        assert!(
            denied
                .set_thread_resolved(&target(), true, &OperationControl::default())
                .is_err()
        );
        assert_eq!(writes(&denied), 0);
        let mut client = client([node_response(thread())]);
        client.0.cancel_after_read = true;
        assert!(
            client
                .reply_to_thread(&target(), "Draft", &OperationControl::default())
                .is_err()
        );
        assert_eq!(writes(&client), 0);
        let control = OperationControl::default();
        control.cancel();
        let mut client = Client(Mock::default());
        assert!(
            client
                .reply_to_thread(&target(), "Draft", &control)
                .is_err()
        );
        assert!(client.0.requests.is_empty());
    }

    #[test]
    fn uncertain_reply_or_resolution_outcomes_are_never_replayed() {
        let mut mismatch = reply_result("Draft");
        mismatch["data"]["addPullRequestReviewThreadReply"]["comment"]["replyTo"]["id"] =
            json!("wrong_root");
        let failures = [
            Err(anyhow!("connection lost")),
            response(json!({"data":null})),
            response(mismatch),
            response(
                json!({"data":{"addPullRequestReviewThreadReply":null},"errors":[{"type":"INTERNAL"}]}),
            ),
            Ok(Response {
                status: 200,
                headers: BTreeMap::new(),
                body: b"not json".to_vec(),
            }),
        ];
        for failure in failures {
            let mut client = Client(Mock {
                responses: VecDeque::from([response(node_response(thread())), failure]),
                ..Default::default()
            });
            let error = client
                .reply_to_thread(&target(), "Draft", &OperationControl::default())
                .unwrap_err();
            assert!(error.to_string().contains("uncertain"));
            assert_eq!(writes(&client), 1);
            assert_eq!(client.0.requests.len(), 2);
        }
        let mut wrong_state = resolution_result(true);
        wrong_state["data"]["resolveReviewThread"]["thread"]["isResolved"] = json!(false);
        let mut client = client([node_response(thread()), wrong_state]);
        assert!(
            client
                .set_thread_resolved(&target(), true, &OperationControl::default())
                .unwrap_err()
                .to_string()
                .contains("uncertain")
        );
        assert_eq!(writes(&client), 1);
    }

    #[test]
    fn graphql_http_200_rate_limit_is_actionable_without_displaying_remote_diagnostics() {
        let mut limited = response(
            json!({"data":null,"errors":[{"type":"RATE_LIMITED","message":"secret diagnostic"}]}),
        )
        .unwrap();
        limited
            .headers
            .insert("x-ratelimit-remaining".into(), "0".into());
        limited
            .headers
            .insert("x-ratelimit-reset".into(), "1789000000".into());
        let mut client = Client(Mock {
            responses: VecDeque::from([Ok(limited)]),
            ..Default::default()
        });
        let error = client
            .review_threads(&pull(), None, &OperationControl::default())
            .unwrap_err()
            .to_string();
        assert!(error.contains("rate limit") && error.contains("1789000000"));
        assert!(!error.contains("secret diagnostic"));
        assert_eq!(client.0.requests.len(), 1);
    }
}
