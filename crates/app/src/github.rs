//! Explicit GitHub collaboration. Call from the serialized operation executor;
//! opening the panel and ordinary local refresh never dispatch network traffic.
pub(crate) mod conversations;
pub(crate) mod drafts;
pub(crate) mod native_fixture;
pub(crate) mod review;
pub(crate) mod transport;

use anyhow::{Result, anyhow, bail, ensure};
use gitturtle_core::OperationControl;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::PathBuf};

pub(crate) const PAGE_SIZE: usize = 50;
pub(crate) const MAX_TEXT: usize = 256 * 1024;
pub(crate) const MAX_RESPONSE: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct Repository {
    pub owner: String,
    pub name: String,
}
impl Repository {
    pub fn parse(value: &str) -> Result<Self> {
        let value = value.trim();
        let path = value
            .strip_prefix("https://github.com/")
            .or_else(|| value.strip_prefix("git@github.com:"))
            .or_else(|| value.strip_prefix("ssh://git@github.com/"))
            .unwrap_or(value);
        let path = path
            .trim_end_matches('/')
            .strip_suffix(".git")
            .unwrap_or(path.trim_end_matches('/'));
        let parts: Vec<_> = path.split('/').collect();
        ensure!(
            parts.len() == 2
                && parts.iter().all(|p| !p.is_empty()
                    && p.len() <= 100
                    && *p != "."
                    && *p != ".."
                    && p.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))),
            "Choose a GitHub.com repository as owner/name or a GitHub remote URL without credentials"
        );
        Ok(Self {
            owner: parts[0].into(),
            name: parts[1].into(),
        })
    }
    pub fn label(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }
    fn endpoint(&self) -> String {
        format!("repos/{}", self.label())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct Revision {
    pub sha: String,
    #[serde(rename = "ref")]
    pub branch: String,
    #[serde(default)]
    pub label: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct PullRequest {
    pub number: u64,
    pub title: String,
    #[serde(default)]
    pub body: Option<String>,
    pub state: String,
    #[serde(default)]
    pub draft: bool,
    pub head: Revision,
    pub base: Revision,
    #[serde(default)]
    pub user: User,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct User {
    pub login: String,
}
impl PullRequest {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.number > 0 && valid_oid(&self.head.sha) && valid_oid(&self.base.sha),
            "GitHub returned an invalid pull request identity"
        );
        ensure!(
            self.title.len() <= MAX_TEXT && self.body.as_ref().is_none_or(|b| b.len() <= MAX_TEXT),
            "Pull request text exceeds the preview limit"
        );
        Ok(())
    }
    pub fn capture(&self, repository: Repository) -> CapturedPull {
        CapturedPull {
            repository,
            number: self.number,
            head: self.head.clone(),
            base: self.base.clone(),
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct CapturedPull {
    pub repository: Repository,
    pub number: u64,
    pub head: Revision,
    pub base: Revision,
}
#[derive(Clone, Debug)]
pub(crate) struct ComparisonTarget {
    pub worktree: PathBuf,
    pub pull: CapturedPull,
}
#[derive(Clone, Debug)]
pub(crate) struct Page<T> {
    pub items: Vec<T>,
    pub page: u32,
    pub has_next: bool,
}
#[derive(Clone, Debug, Default)]
pub(crate) struct PullStatus {
    pub reviews: Vec<String>,
    pub checks: Vec<String>,
    pub incomplete: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct NewPull {
    pub repository: Repository,
    pub title: String,
    pub body: String,
    pub head: String,
    pub base: String,
    pub draft: bool,
    #[serde(default)]
    pub expected_head: Option<String>,
    #[serde(default)]
    pub expected_base: Option<String>,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) enum ReviewEvent {
    Comment,
    Approve,
    RequestChanges,
}
impl ReviewEvent {
    pub fn api(self) -> &'static str {
        match self {
            Self::Comment => "COMMENT",
            Self::Approve => "APPROVE",
            Self::RequestChanges => "REQUEST_CHANGES",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Comment => "Comment",
            Self::Approve => "Approve",
            Self::RequestChanges => "Request changes",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct LineComment {
    pub path: String,
    pub line: u32,
    pub side: DiffSide,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_line: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_side: Option<DiffSide>,
    pub body: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum DiffSide {
    Left,
    Right,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ReviewDraft {
    pub pull: CapturedPull,
    pub body: String,
    pub event: ReviewEvent,
    pub comments: Vec<LineComment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub composing: Option<LineComment>,
    #[serde(default)]
    pub discussion: bool,
}
#[derive(Clone, Debug)]
pub(crate) enum Action {
    Create(NewPull),
    Comment {
        pull: CapturedPull,
        body: String,
    },
    Review(ReviewDraft),
    Reply {
        target: conversations::CapturedThread,
        body: String,
    },
    SetThreadResolved {
        target: conversations::CapturedThread,
        resolved: bool,
    },
}
impl Action {
    pub fn destination(&self) -> String {
        match self {
            Self::Create(p) => format!(
                "github.com/{} · {} ({}) → {} ({}) · {} PR",
                p.repository.label(),
                p.head,
                p.expected_head.as_deref().unwrap_or("unreviewed"),
                p.base,
                p.expected_base.as_deref().unwrap_or("unreviewed"),
                if p.draft { "draft" } else { "ready" }
            ),
            Self::Comment { pull, .. } | Self::Review(ReviewDraft { pull, .. }) => format!(
                "github.com/{} #{} · head {} · base {}",
                pull.repository.label(),
                pull.number,
                pull.head.sha,
                pull.base.sha
            ),
            Self::Reply { target, .. } | Self::SetThreadResolved { target, .. } => {
                target.destination()
            }
        }
    }
}
#[derive(Clone, Debug)]
pub(crate) struct Request {
    pub method: &'static str,
    pub endpoint: String,
    pub body: Option<Value>,
}
#[derive(Clone, Debug)]
pub(crate) struct Response {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}
impl Response {
    fn json<T: for<'de> Deserialize<'de>>(&self) -> Result<T> {
        ensure!(
            self.body.len() <= MAX_RESPONSE,
            "GitHub response exceeds the 8 MiB limit"
        );
        serde_json::from_slice(&self.body).map_err(|_| anyhow!("GitHub returned an unsupported response; refresh explicitly to verify the current state"))
    }
    fn check(&self, writing: bool) -> Result<()> {
        if (200..300).contains(&self.status) {
            return Ok(());
        }
        let suffix = if writing {
            " No automatic retry was made. Check GitHub before resubmitting."
        } else {
            ""
        };
        match self.status {
            401 => bail!(
                "GitHub authorization expired or was rejected. Reconnect the account.{suffix}"
            ),
            403 | 429
                if self.status == 429
                    || self
                        .headers
                        .get("x-ratelimit-remaining")
                        .is_some_and(|v| v == "0")
                    || self.headers.contains_key("retry-after") =>
            {
                let wait = self
                    .headers
                    .get("retry-after")
                    .map(|v| retry_after(v))
                    .or_else(|| {
                        self.headers
                            .get("x-ratelimit-reset")
                            .map(|v| format!(" Rate limit resets at Unix time {v}."))
                    })
                    .unwrap_or_default();
                bail!("GitHub rate limit reached.{wait}{suffix}")
            }
            403 => bail!(
                "GitHub refused this action. Check repository access, organization authorization, and review permissions.{suffix}"
            ),
            404 => {
                bail!("GitHub repository or pull request is unavailable to this account.{suffix}")
            }
            409 | 422 => bail!(
                "GitHub rejected the reviewed target or content. Refresh and review the branches, permissions, and diff positions.{suffix}"
            ),
            _ => bail!(
                "GitHub returned HTTP {}.{}",
                self.status,
                if writing {
                    " The remote outcome may be uncertain. Check GitHub before resubmitting; this action was not replayed."
                } else {
                    " Refresh explicitly when the service is available."
                }
            ),
        }
    }
    /// Whether this answers a creation with "the head branch already has a pull request", told
    /// by its form: status 422 and a body within the size limit that parses and holds an
    /// `errors` entry whose `resource` is "PullRequest" and whose `message` starts with GitHub's
    /// sentence for it. Any other 422, a 409, and a body that does not parse or is over the
    /// limit are not it.
    fn pull_exists(&self) -> bool {
        self.status == 422
            && self.json::<Value>().is_ok_and(|body| {
                body["errors"].as_array().is_some_and(|errors| {
                    errors.iter().any(|error| {
                        error["resource"].as_str() == Some("PullRequest")
                            && error["message"].as_str().is_some_and(|message| {
                                message.starts_with("A pull request already exists for ")
                            })
                    })
                })
            })
    }
    fn next_page(&self) -> bool {
        self.headers
            .get("link")
            .is_some_and(|v| v.split(',').any(|p| p.contains("rel=\"next\"")))
    }
}
/// GitHub refused a creation because its head branch already has a pull request
/// ([`Response::pull_exists`]). It is the one refused write whose outcome is certain, nothing
/// was created, so it is carried typed for the panel to say so from the head branch it
/// captured, with the pull request where its loaded list holds it. GitHub's own text is matched
/// and never kept: this type holds none of it, and its words are the app's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PullExists;
impl std::fmt::Display for PullExists {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("The head branch already has a pull request. Nothing was created.")
    }
}
impl std::error::Error for PullExists {}
/// What GitHub accepted ([`Client::execute`]).
#[derive(Clone, Debug)]
pub(crate) struct Sent {
    /// What the attempt record keeps of it: "GitHub accepted … at ‹destination›".
    pub notice: String,
    /// For a creation, what its answer told of the pull request it made.
    pub created: Option<Created>,
}
impl Sent {
    /// An accepted action that created no pull request.
    fn plain(notice: String) -> Self {
        Self {
            notice,
            created: None,
        }
    }
}
/// What a creation's answer told of the pull request GitHub made. Whatever it told, the
/// creation was accepted: an answer that cannot be read is no failure, and nothing is sent
/// again.
#[derive(Clone, Debug)]
pub(crate) enum Created {
    /// The pull request, as the list reads one: valid, open and the one sent, from the sent
    /// head branch onto the sent base.
    Pull(Box<PullRequest>),
    /// Its number alone.
    Number(u64),
    /// Nothing that could be read.
    Unread,
}
impl Created {
    /// What a creation of `new` learned from its answer: `pull` where the answer parsed as a
    /// pull request that is valid, open and the one sent; otherwise its `number`, or nothing.
    fn read(pull: Option<PullRequest>, number: Option<u64>, new: &NewPull) -> Self {
        let branch = new
            .head
            .split_once(':')
            .map_or(new.head.as_str(), |(_, branch)| branch);
        match (pull, number) {
            (Some(pull), _)
                if pull.validate().is_ok()
                    && pull.state == "open"
                    && pull.head.branch == branch
                    && pull.base.branch == new.base =>
            {
                Self::Pull(Box::new(pull))
            }
            (_, Some(number)) => Self::Number(number),
            _ => Self::Unread,
        }
    }
}
pub(crate) trait Transport {
    fn request(&mut self, request: Request, control: &OperationControl) -> Result<Response>;
}
/// A transport that answers from a script and keeps what it was asked, for a test of a route
/// the offline fixture has no answer for, as a creation is. It reaches no credential, process
/// or network, and a request past the script's end fails.
#[cfg(test)]
#[derive(Clone, Default)]
pub(crate) struct Scripted {
    pub requests: std::sync::Arc<std::sync::Mutex<Vec<Request>>>,
    answers: std::sync::Arc<std::sync::Mutex<std::collections::VecDeque<Response>>>,
}
#[cfg(test)]
impl Scripted {
    /// The account a scripted transport is signed in as.
    pub const LOGIN: &str = "scripted-author";
    /// Adds `body` under `status` as the next answer.
    pub fn answer(&self, status: u16, body: impl Into<Vec<u8>>) {
        self.answers.lock().unwrap().push_back(Response {
            status,
            headers: BTreeMap::new(),
            body: body.into(),
        });
    }
    /// How many requests of `method` it was asked at an endpoint ending in `ending`.
    pub fn asked(&self, method: &str, ending: &str) -> usize {
        let requests = self.requests.lock().unwrap();
        requests
            .iter()
            .filter(|request| request.method == method && request.endpoint.ends_with(ending))
            .count()
    }
}
/// GitHub's answer to a creation from a branch that already has a pull request, as captured on
/// 2026-11-16 under status 422, its owner and branch a fixture's.
#[cfg(test)]
pub(crate) const EXISTS: &str = r#"{"message":"Validation Failed","errors":[{"resource":"PullRequest","code":"custom","message":"A pull request already exists for fixture:feature."}],"documentation_url":"https://docs.github.com/rest/pulls/pulls#create-a-pull-request","status":"422"}"#;
#[cfg(test)]
impl Transport for Scripted {
    fn request(&mut self, request: Request, _: &OperationControl) -> Result<Response> {
        self.requests.lock().unwrap().push(request);
        let answer = self.answers.lock().unwrap().pop_front();
        answer.ok_or_else(|| anyhow!("The script holds no further answer"))
    }
}
pub(crate) struct Client<T>(pub T);
impl<T: Transport> Client<T> {
    fn get(&mut self, endpoint: String, control: &OperationControl) -> Result<Response> {
        ensure!(!control.is_cancelled(), "GitHub request cancelled");
        let response = self.0.request(
            Request {
                method: "GET",
                endpoint,
                body: None,
            },
            control,
        )?;
        response.check(false)?;
        Ok(response)
    }
    pub fn account(&mut self, control: &OperationControl) -> Result<User> {
        self.get("user".into(), control)?.json()
    }
    pub fn list(
        &mut self,
        repository: &Repository,
        page: u32,
        control: &OperationControl,
    ) -> Result<Page<PullRequest>> {
        ensure!(
            (1..=1000).contains(&page),
            "PR page must be between 1 and 1000"
        );
        let response = self.get(
            format!(
                "{}/pulls?state=open&sort=created&direction=desc&per_page={PAGE_SIZE}&page={page}",
                repository.endpoint()
            ),
            control,
        )?;
        let items: Vec<PullRequest> = response.json()?;
        ensure!(
            items.len() <= PAGE_SIZE,
            "GitHub returned too many pull requests for one page"
        );
        for item in &items {
            item.validate()?;
        }
        Ok(Page {
            items,
            page,
            has_next: response.next_page(),
        })
    }
    pub fn pull(
        &mut self,
        repository: &Repository,
        number: u64,
        control: &OperationControl,
    ) -> Result<PullRequest> {
        ensure!(number > 0, "Select a pull request");
        let pull: PullRequest = self
            .get(format!("{}/pulls/{number}", repository.endpoint()), control)?
            .json()?;
        pull.validate()?;
        ensure!(
            pull.number == number,
            "GitHub returned a different pull request"
        );
        Ok(pull)
    }
    pub fn status(
        &mut self,
        pull: &CapturedPull,
        control: &OperationControl,
    ) -> Result<PullStatus> {
        let reviews = self.get(
            format!(
                "{}/pulls/{}/reviews?per_page=100",
                pull.repository.endpoint(),
                pull.number
            ),
            control,
        )?;
        let checks = self.get(
            format!(
                "{}/commits/{}/check-runs?per_page=100",
                pull.repository.endpoint(),
                pull.head.sha
            ),
            control,
        )?;
        let statuses = self.get(
            format!(
                "{}/commits/{}/status?per_page=100",
                pull.repository.endpoint(),
                pull.head.sha
            ),
            control,
        )?;
        let review_values: Vec<Value> = reviews.json()?;
        let check_values: Value = checks.json()?;
        let status_values: Value = statuses.json()?;
        ensure!(
            review_values.len() <= 100
                && check_values["check_runs"]
                    .as_array()
                    .is_none_or(|v| v.len() <= 100)
                && status_values["statuses"]
                    .as_array()
                    .is_none_or(|v| v.len() <= 100),
            "GitHub returned too many status entries"
        );
        Ok(PullStatus {
            reviews: review_values
                .iter()
                .map(|v| {
                    format!(
                        "{}: {}{}",
                        v["user"]["login"].as_str().unwrap_or("Unknown"),
                        v["state"].as_str().unwrap_or("Unknown"),
                        if v["commit_id"]
                            .as_str()
                            .is_some_and(|oid| oid != pull.head.sha)
                        {
                            " (older head)"
                        } else {
                            ""
                        }
                    )
                })
                .collect(),
            checks: check_values["check_runs"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|v| {
                    format!(
                        "{}: {}",
                        v["name"].as_str().unwrap_or("Check"),
                        v["conclusion"]
                            .as_str()
                            .or_else(|| v["status"].as_str())
                            .unwrap_or("Unknown")
                    )
                })
                .chain(
                    status_values["statuses"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(|v| {
                            format!(
                                "{}: {}",
                                v["context"].as_str().unwrap_or("Status"),
                                v["state"].as_str().unwrap_or("Unknown")
                            )
                        }),
                )
                .collect(),
            incomplete: reviews.next_page() || checks.next_page() || statuses.next_page(),
        })
    }
    pub fn prepare_create(
        &mut self,
        mut pull: NewPull,
        control: &OperationControl,
    ) -> Result<NewPull> {
        validate_text(&pull.title, true)?;
        validate_text(&pull.body, false)?;
        ensure!(
            !pull.head.is_empty()
                && !pull.base.is_empty()
                && pull.head.len() <= 1024
                && pull.base.len() <= 1024,
            "Choose published head and destination base branches"
        );
        let (head_repo, head_branch) = if let Some((owner, branch)) = pull.head.split_once(':') {
            (
                Repository::parse(&format!("{owner}/{}", pull.repository.name))?,
                branch,
            )
        } else {
            (pull.repository.clone(), pull.head.as_str())
        };
        let head: Value = self
            .get(
                format!(
                    "{}/commits/{}",
                    head_repo.endpoint(),
                    encode_component(head_branch)
                ),
                control,
            )?
            .json()?;
        let base: Value = self
            .get(
                format!(
                    "{}/commits/{}",
                    pull.repository.endpoint(),
                    encode_component(&pull.base)
                ),
                control,
            )?
            .json()?;
        let head = head["sha"]
            .as_str()
            .filter(|value| valid_oid(value))
            .ok_or_else(|| anyhow!("GitHub head identity is unavailable"))?;
        let base = base["sha"]
            .as_str()
            .filter(|value| valid_oid(value))
            .ok_or_else(|| anyhow!("GitHub base identity is unavailable"))?;
        pull.expected_head = Some(head.into());
        pull.expected_base = Some(base.into());
        Ok(pull)
    }
    pub fn execute(&mut self, action: Action, control: &OperationControl) -> Result<Sent> {
        ensure!(
            !control.is_cancelled(),
            "GitHub action cancelled before submission"
        );
        match &action {
            Action::Reply { target, body } => {
                return self.reply_to_thread(target, body, control).map(Sent::plain);
            }
            Action::SetThreadResolved { target, resolved } => {
                return self
                    .set_thread_resolved(target, *resolved, control)
                    .map(Sent::plain);
            }
            _ => {}
        }
        let (endpoint, body) = match &action {
            Action::Create(p) => {
                ensure!(
                    p.expected_head.is_some() && p.expected_base.is_some(),
                    "Review the published head and base identities before creating this pull request"
                );
                let current = self.prepare_create(p.clone(), control)?;
                ensure!(
                    current.expected_head == p.expected_head
                        && current.expected_base == p.expected_base,
                    "The published head or base moved after review. Your draft is retained; review the new identities before creating the pull request"
                );
                validate_text(&p.title, true)?;
                validate_text(&p.body, false)?;
                ensure!(
                    !p.head.is_empty()
                        && !p.base.is_empty()
                        && p.head.len() <= 1024
                        && p.base.len() <= 1024,
                    "Choose the published head and destination base branch"
                );
                (
                    format!("{}/pulls", p.repository.endpoint()),
                    json!({ "title": p.title, "body": p.body, "head": p.head, "base": p.base, "draft": p.draft }),
                )
            }
            Action::Comment { pull, body } => {
                validate_text(body, true)?;
                self.revalidate(pull, control)?;
                (
                    format!(
                        "{}/issues/{}/comments",
                        pull.repository.endpoint(),
                        pull.number
                    ),
                    json!({ "body": body }),
                )
            }
            Action::Review(draft) => {
                validate_text(
                    &draft.body,
                    draft.event == ReviewEvent::RequestChanges
                        || (draft.event == ReviewEvent::Comment && draft.comments.is_empty()),
                )?;
                ensure!(
                    draft.composing.is_none(),
                    "Add or discard the unfinished inline comment before reviewing submission"
                );
                ensure!(
                    draft.comments.len() <= 100,
                    "A review supports at most 100 inline comments"
                );
                for comment in &draft.comments {
                    validate_text(&comment.body, true)?;
                    ensure!(
                        comment.line > 0
                            && safe_path(&comment.path)
                            && match (comment.start_line, comment.start_side) {
                                (None, None) => true,
                                (Some(start), Some(side)) =>
                                    start > 0 && start < comment.line && side == comment.side,
                                _ => false,
                            },
                        "Review comment position is invalid"
                    );
                }
                self.revalidate(&draft.pull, control)?;
                (
                    format!(
                        "{}/pulls/{}/reviews",
                        draft.pull.repository.endpoint(),
                        draft.pull.number
                    ),
                    json!({ "commit_id": draft.pull.head.sha, "body": draft.body, "event": draft.event.api(), "comments": draft.comments }),
                )
            }
            Action::Reply { .. } | Action::SetThreadResolved { .. } => unreachable!(),
        };
        ensure!(
            !control.is_cancelled(),
            "GitHub action cancelled before submission"
        );
        let response = self.0.request(Request { method: "POST", endpoint, body: Some(body) }, control).map_err(|error| anyhow!("{error}. The GitHub outcome may be uncertain. Check the destination before resubmitting; no automatic retry was made."))?;
        // Only a creation is refused this way; every other write keeps the one sentence.
        if matches!(action, Action::Create(_)) && response.pull_exists() {
            return Err(PullExists.into());
        }
        response.check(true)?;
        // A creation GitHub accepted was made, whatever its answer can tell of it: the pull
        // request as the list reads one, its number alone, or nothing. None of that is a
        // failure, and nothing is sent again.
        if let Action::Create(new) = &action {
            let value = response.json::<Value>().ok();
            let number = value
                .as_ref()
                .and_then(|value| value["number"].as_u64())
                .filter(|number| *number > 0);
            let pull = value.and_then(|value| serde_json::from_value::<PullRequest>(value).ok());
            let notice = match number {
                Some(number) => format!(
                    "GitHub accepted action {number} at {}",
                    action.destination()
                ),
                None => format!(
                    "GitHub accepted the pull request at {}; its answer could not be read",
                    action.destination()
                ),
            };
            return Ok(Sent {
                notice,
                created: Some(Created::read(pull, number, new)),
            });
        }
        let value: Value = response.json().map_err(|_| anyhow!("GitHub accepted the action but its response could not be read. Check the destination before resubmitting; no automatic retry was made."))?;
        let id = value["number"].as_u64().or_else(|| value["id"].as_u64()).ok_or_else(|| anyhow!("GitHub accepted the action but returned no identity. Check the destination before resubmitting."))?;
        Ok(Sent::plain(format!(
            "GitHub accepted action {id} at {}",
            action.destination()
        )))
    }
    fn revalidate(&mut self, captured: &CapturedPull, control: &OperationControl) -> Result<()> {
        let current = self.pull(&captured.repository, captured.number, control)?;
        ensure!(
            current.head == captured.head && current.base == captured.base,
            "This pull request head or base moved. Your draft is retained. Refresh and review the new comparison before submitting; outdated diff positions are not remapped automatically"
        );
        ensure!(
            current.state == "open",
            "This pull request is no longer open. Your draft is retained"
        );
        Ok(())
    }
}
#[derive(Clone, Debug)]
pub(crate) struct Template {
    pub path: String,
    pub revision: String,
    pub body: String,
}
/// The PR templates at `revision` in the local repository, read until
/// `cancellation` stops them, which ends the read with
/// [`gitturtle_core::ReadCancelled`].
pub(crate) fn local_templates(
    repo: &gitturtle_core::GitRepository,
    revision: &str,
    cancellation: &gitturtle_core::HistoryCancellation,
) -> Result<Vec<Template>> {
    let listing = repo.search_tracked_paths(
        &gitturtle_core::PathScope::Revision(revision.into()),
        "pull_request_template",
        cancellation,
    )?;
    read_templates(repo, listing, cancellation)
}
/// The templates `listing` names, each read only while `cancellation` lets
/// it: a cancel arriving after the listing stops before the next template.
fn read_templates(
    repo: &gitturtle_core::GitRepository,
    listing: gitturtle_core::TrackedPaths,
    cancellation: &gitturtle_core::HistoryCancellation,
) -> Result<Vec<Template>> {
    let revision = match listing.scope {
        gitturtle_core::PathScope::Revision(oid) => oid,
        _ => unreachable!(),
    };
    let mut templates = Vec::new();
    for entry in listing.entries {
        let Some(path) = entry.path.to_str() else {
            continue;
        };
        let lower = path.to_ascii_lowercase();
        let applicable = [
            "pull_request_template.md",
            "docs/pull_request_template.md",
            ".github/pull_request_template.md",
        ]
        .contains(&lower.as_str())
            || [
                "pull_request_template/",
                "docs/pull_request_template/",
                ".github/pull_request_template/",
            ]
            .iter()
            .any(|prefix| {
                lower
                    .strip_prefix(prefix)
                    .is_some_and(|rest| !rest.contains('/') && rest.ends_with(".md"))
            });
        if !applicable || !matches!(entry.mode.as_str(), "100644" | "100755") {
            continue;
        }
        if cancellation.is_cancelled() {
            return Err(gitturtle_core::ReadCancelled.into());
        }
        ensure!(
            templates.len() < 32,
            "More than 32 PR templates are present; choose a narrower supported template set"
        );
        ensure!(
            repo.blob_size(&entry.oid)? <= MAX_TEXT,
            "PR template exceeds the 256 KiB limit"
        );
        let body = String::from_utf8(repo.blob(&entry.oid)?)
            .map_err(|_| anyhow!("PR template is not UTF-8"))?;
        templates.push(Template {
            path: path.into(),
            revision: revision.clone(),
            body,
        });
    }
    Ok(templates)
}
fn encode_component(value: &str) -> String {
    value
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) {
                (byte as char).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}
fn valid_oid(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|b| b.is_ascii_hexdigit())
}
fn safe_path(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 4096
        && !value.starts_with('/')
        && !value.contains('\0')
        && !value
            .split('/')
            .any(|p| p == ".." || p == "." || p.is_empty())
}
pub(crate) fn validate_text(value: &str, required: bool) -> Result<()> {
    ensure!(
        value.len() <= MAX_TEXT && !value.contains('\0') && (!required || !value.trim().is_empty()),
        "Text is empty, contains NUL, or exceeds the 256 KiB limit"
    );
    Ok(())
}

/// What a `retry-after` header says to wait, as a sentence after the
/// refusal: its seconds agreeing with their noun ("Retry after 1 second.",
/// where it read "1 seconds"), a decimal number of them with its unit
/// ("Retry after 1.5 seconds.", where it read "Retry after 1.5."), written
/// as a number is ("5." as 5 seconds, ".5" as 0.5 seconds), whole seconds
/// beyond `u64` still in seconds, or the date and time the header can give
/// instead, as it gives them.
pub(crate) fn retry_after(value: &str) -> String {
    let value = value.trim();
    let digits = |part: &str| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit());
    let whole = |seconds: &str| match seconds.parse::<u64>() {
        Ok(seconds) => crate::counted(seconds, "second", "seconds"),
        // More seconds than a u64 holds are still seconds.
        Err(_) => format!("{seconds} seconds"),
    };
    // The header's seconds are whole, but a server that sends a fraction
    // still means seconds; a leading "+" is the number's sign, as the
    // integer parse once took it.
    let number = value.strip_prefix('+').unwrap_or(value);
    let wait = match number.split_once('.') {
        None if digits(number) => whole(number),
        Some((seconds, "")) if digits(seconds) => whole(seconds),
        Some((seconds, fraction))
            if (seconds.is_empty() || digits(seconds)) && digits(fraction) =>
        {
            let seconds = if seconds.is_empty() { "0" } else { seconds };
            format!("{seconds}.{fraction} seconds")
        }
        _ => value.to_owned(),
    };
    format!(" Retry after {wait}.")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    /// The wait a refusal names agrees with its noun, a decimal wait keeps
    /// its unit ("1.5" read "Retry after 1.5."), and a date stays a date.
    #[test]
    fn retry_after_counts_its_seconds() {
        assert_eq!(retry_after("1"), " Retry after 1 second.");
        assert_eq!(retry_after("30"), " Retry after 30 seconds.");
        assert_eq!(retry_after(" 1.5 "), " Retry after 1.5 seconds.");
        assert_eq!(retry_after("0.25"), " Retry after 0.25 seconds.");
        assert_eq!(retry_after("5."), " Retry after 5 seconds.");
        assert_eq!(retry_after("+5"), " Retry after 5 seconds.");
        assert_eq!(retry_after("+1.5"), " Retry after 1.5 seconds.");
        assert_eq!(retry_after("1."), " Retry after 1 second.");
        assert_eq!(retry_after(".5"), " Retry after 0.5 seconds.");
        assert_eq!(
            retry_after("99999999999999999999"),
            " Retry after 99999999999999999999 seconds."
        );
        assert_eq!(retry_after("."), " Retry after ..");
        assert_eq!(retry_after("1.2.3"), " Retry after 1.2.3.");
        assert_eq!(
            retry_after("Wed, 21 Oct 2026 07:28:00 GMT"),
            " Retry after Wed, 21 Oct 2026 07:28:00 GMT."
        );
    }
    #[derive(Default)]
    struct Mock {
        requests: Vec<Request>,
        responses: VecDeque<Result<Response>>,
    }
    impl Transport for Mock {
        fn request(&mut self, request: Request, _: &OperationControl) -> Result<Response> {
            self.requests.push(request);
            self.responses.pop_front().unwrap()
        }
    }
    fn pull() -> PullRequest {
        serde_json::from_value(json!({"number":7,"title":"Fixture PR","state":"open","draft":true,"head":{"sha":"1111111111111111111111111111111111111111","ref":"feature"},"base":{"sha":"2222222222222222222222222222222222222222","ref":"main"}})).unwrap()
    }
    fn response(value: Value) -> Result<Response> {
        Ok(Response {
            status: 200,
            headers: BTreeMap::new(),
            body: serde_json::to_vec(&value).unwrap(),
        })
    }
    fn repo() -> Repository {
        Repository::parse("git@github.com:fixture/repo.git").unwrap()
    }
    #[test]
    fn rejects_credentials_and_foreign_hosts() {
        for target in [
            "https://token@github.com/o/r",
            "https://evil.test/o/r",
            "o/../r",
            "o/r?q=x",
            "o/r#x",
        ] {
            assert!(Repository::parse(target).is_err());
        }
        assert_eq!(repo().label(), "fixture/repo");
    }
    #[test]
    fn paging_is_explicit_and_bounded() {
        let mut page = response(json!([pull()])).unwrap();
        page.headers.insert(
            "link".into(),
            "<https://evil.test/exfiltrate>; rel=\"next\"".into(),
        );
        let mut client = Client(Mock {
            responses: VecDeque::from([Ok(page)]),
            ..Default::default()
        });
        let result = client
            .list(&repo(), 2, &OperationControl::default())
            .unwrap();
        assert!(result.has_next);
        assert_eq!(result.page, 2);
        assert!(client.0.requests[0].endpoint.ends_with("page=2"));
        assert_eq!(client.0.requests.len(), 1);
    }
    #[test]
    fn moved_head_preserves_review_without_posting() {
        let old = pull();
        let mut moved = old.clone();
        moved.head.sha = "3333333333333333333333333333333333333333".into();
        let mut client = Client(Mock {
            responses: VecDeque::from([response(json!(moved))]),
            ..Default::default()
        });
        let result = client.execute(
            Action::Review(ReviewDraft {
                pull: old.capture(repo()),
                body: "Review".into(),
                event: ReviewEvent::Approve,
                comments: vec![],
                composing: None,
                discussion: false,
            }),
            &OperationControl::default(),
        );
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("head or base moved")
        );
        assert_eq!(client.0.requests.len(), 1);
        assert_eq!(client.0.requests[0].method, "GET");
    }
    #[test]
    fn review_posts_captured_commit_once_and_uncertainty_never_replays() {
        let original = pull();
        let mut client = Client(Mock {
            responses: VecDeque::from([response(json!(original)), Err(anyhow!("connection lost"))]),
            ..Default::default()
        });
        let error = client
            .execute(
                Action::Review(ReviewDraft {
                    pull: original.capture(repo()),
                    body: "Review".into(),
                    event: ReviewEvent::RequestChanges,
                    comments: vec![LineComment {
                        path: "src/a.rs".into(),
                        line: 8,
                        side: DiffSide::Right,
                        start_line: None,
                        start_side: None,
                        body: "Explain this".into(),
                    }],
                    composing: None,
                    discussion: false,
                }),
                &OperationControl::default(),
            )
            .unwrap_err();
        assert!(error.to_string().contains("uncertain"));
        assert_eq!(client.0.requests.len(), 2);
        assert_eq!(
            client.0.requests[1].body.as_ref().unwrap()["commit_id"],
            original.head.sha
        );
        assert_eq!(
            client.0.requests[1].body.as_ref().unwrap()["comments"][0]["side"],
            "RIGHT"
        );
    }
    #[test]
    fn cancellation_prevents_dispatch_and_rate_limit_does_not_retry() {
        let mut client = Client(Mock::default());
        let control = OperationControl::default();
        control.cancel();
        assert!(client.list(&repo(), 1, &control).is_err());
        assert!(client.0.requests.is_empty());
        let response = Response {
            status: 429,
            headers: BTreeMap::from([("retry-after".into(), "60".into())]),
            body: vec![],
        };
        client.0.responses.push_back(Ok(response));
        assert!(
            client
                .list(&repo(), 1, &OperationControl::default())
                .unwrap_err()
                .to_string()
                .contains("60 seconds")
        );
        assert_eq!(client.0.requests.len(), 1);
    }
    #[test]
    fn create_rechecks_published_identities_and_refuses_moved_base() {
        let initial = NewPull {
            repository: repo(),
            title: "Ready".into(),
            body: "Body".into(),
            head: "feature/slash".into(),
            base: "main".into(),
            draft: true,
            expected_head: None,
            expected_base: None,
        };
        let mut client = Client(Mock {
            responses: VecDeque::from([
                response(json!({"sha":"1".repeat(40)})),
                response(json!({"sha":"2".repeat(40)})),
                response(json!({"sha":"1".repeat(40)})),
                response(json!({"sha":"3".repeat(40)})),
            ]),
            ..Default::default()
        });
        let prepared = client
            .prepare_create(initial, &OperationControl::default())
            .unwrap();
        assert!(client.0.requests[0].endpoint.ends_with("feature%2Fslash"));
        assert!(
            client
                .execute(Action::Create(prepared), &OperationControl::default())
                .unwrap_err()
                .to_string()
                .contains("base moved")
        );
        assert!(
            client
                .0
                .requests
                .iter()
                .all(|request| request.method == "GET")
        );
    }
    #[test]
    fn expired_authorization_is_actionable_without_replay() {
        let mut client = Client(Mock {
            responses: VecDeque::from([Ok(Response {
                status: 401,
                headers: BTreeMap::new(),
                body: b"{}".to_vec(),
            })]),
            ..Default::default()
        });
        assert!(
            client
                .account(&OperationControl::default())
                .unwrap_err()
                .to_string()
                .contains("Reconnect")
        );
        assert_eq!(client.0.requests.len(), 1);
    }
    #[test]
    fn create_sends_draft_choice_after_two_identity_rechecks() {
        let pull = NewPull {
            repository: repo(),
            title: "Ready".into(),
            body: "Exact body\n".into(),
            head: "feature".into(),
            base: "main".into(),
            draft: false,
            expected_head: Some("1".repeat(40)),
            expected_base: Some("2".repeat(40)),
        };
        let mut client = Client(Mock {
            responses: VecDeque::from([
                response(json!({"sha":"1".repeat(40)})),
                response(json!({"sha":"2".repeat(40)})),
                response(json!({"number":9})),
            ]),
            ..Default::default()
        });
        client
            .execute(Action::Create(pull), &OperationControl::default())
            .unwrap();
        assert_eq!(client.0.requests.len(), 3);
        assert_eq!(client.0.requests[2].body.as_ref().unwrap()["draft"], false);
        assert_eq!(
            client.0.requests[2].body.as_ref().unwrap()["body"],
            "Exact body\n"
        );
    }
    /// What every other refused write says.
    const REJECTED: &str = "GitHub rejected the reviewed target or content. Refresh and review the branches, permissions, and diff positions. No automatic retry was made. Check GitHub before resubmitting.";
    fn reviewed() -> NewPull {
        NewPull {
            repository: repo(),
            title: "Ready".into(),
            body: "Body".into(),
            head: "feature".into(),
            base: "main".into(),
            draft: true,
            expected_head: Some("1".repeat(40)),
            expected_base: Some("2".repeat(40)),
        }
    }
    fn answer(status: u16, body: &str) -> Result<Response> {
        Ok(Response {
            status,
            headers: BTreeMap::new(),
            body: body.as_bytes().to_vec(),
        })
    }
    /// A client whose transport answers a creation's two identity rechecks, then `last`.
    fn creating(last: Result<Response>) -> Client<Mock> {
        Client(Mock {
            responses: VecDeque::from([
                response(json!({"sha":"1".repeat(40)})),
                response(json!({"sha":"2".repeat(40)})),
                last,
            ]),
            ..Default::default()
        })
    }
    /// Answers reads from the offline fixture and every mutation with the captured refusal.
    struct RefusingMutations(review::UiTransport);
    impl Transport for RefusingMutations {
        fn request(&mut self, request: Request, control: &OperationControl) -> Result<Response> {
            let mutation = request.body.as_ref().is_some_and(|body| {
                body["query"]
                    .as_str()
                    .is_some_and(|query| query.starts_with("mutation"))
            });
            if mutation {
                answer(422, EXISTS)
            } else {
                self.0.request(request, control)
            }
        }
    }
    /// A creation GitHub refuses because its head branch already has a pull request is told
    /// apart by the form of the answer captured on 2026-11-16 and carried typed, in the app's
    /// words and none of GitHub's. Every other refused write keeps the one sentence word for
    /// word: a 422 of another form, one whose body does not parse or is over the size limit, a
    /// 409, and the same answer to a discussion comment, a review and a reply. The refusal
    /// once read as any other, "GitHub rejected the reviewed target or content…", which sent
    /// the person to check branches, permissions and diff positions.
    #[test]
    fn only_a_creation_refused_for_an_existing_pull_request_is_told_apart() {
        let control = OperationControl::default();
        let refused = |last: Result<Response>| {
            let mut client = creating(last);
            let error = client
                .execute(Action::Create(reviewed()), &control)
                .unwrap_err();
            assert_eq!(client.0.requests.len(), 3, "one creation, sent once");
            error
        };
        let error = refused(answer(422, EXISTS));
        assert!(error.is::<PullExists>(), "{error:#}");
        assert_eq!(
            error.to_string(),
            "The head branch already has a pull request. Nothing was created."
        );
        assert!(
            !format!("{error:#} {error:?}").contains("fixture:feature"),
            "none of GitHub's text is kept"
        );
        let mut oversized = EXISTS.as_bytes().to_vec();
        oversized.resize(MAX_RESPONSE + 1, b' ');
        let other = |message: &str, resource: &str| {
            json!({"message":"Validation Failed","errors":[{"resource":resource,"code":"custom","message":message}]}).to_string()
        };
        for (name, status, body) in [
            (
                "another 422",
                422,
                other("No commits between main and feature", "PullRequest").into_bytes(),
            ),
            (
                "the sentence about another resource",
                422,
                other(
                    "A pull request already exists for fixture:feature.",
                    "Issue",
                )
                .into_bytes(),
            ),
            (
                "the sentence inside another message",
                422,
                other(
                    "Refused. A pull request already exists for fixture:feature.",
                    "PullRequest",
                )
                .into_bytes(),
            ),
            ("a 422 with no errors", 422, b"{}".to_vec()),
            ("a 422 that does not parse", 422, b"<html>".to_vec()),
            ("an empty 422", 422, Vec::new()),
            ("a 422 over the size limit", 422, oversized),
            ("a 409", 409, EXISTS.as_bytes().to_vec()),
        ] {
            let error = refused(Ok(Response {
                status,
                headers: BTreeMap::new(),
                body,
            }));
            assert!(!error.is::<PullExists>(), "{name}");
            assert_eq!(error.to_string(), REJECTED, "{name}");
        }
        // The same answer to any other write: a discussion comment, a review and a reply.
        let listed = pull();
        for action in [
            Action::Comment {
                pull: listed.capture(repo()),
                body: "Comment".into(),
            },
            Action::Review(ReviewDraft {
                pull: listed.capture(repo()),
                body: "Review".into(),
                event: ReviewEvent::Approve,
                comments: vec![],
                composing: None,
                discussion: false,
            }),
        ] {
            let mut client = Client(Mock {
                responses: VecDeque::from([response(json!(listed)), answer(422, EXISTS)]),
                ..Default::default()
            });
            let error = client.execute(action, &control).unwrap_err();
            assert!(!error.is::<PullExists>());
            assert_eq!(error.to_string(), REJECTED);
        }
        let fixture = || review::UiTransport::Fixture { moved: false };
        let identity = review::fixture_pull(false)
            .capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
        let threads = Client(fixture())
            .review_threads(&identity, None, &control)
            .unwrap();
        let reply = Action::Reply {
            target: threads.items[0].target.clone(),
            body: "Reply".into(),
        };
        let error = Client(RefusingMutations(fixture()))
            .execute(reply, &control)
            .unwrap_err();
        assert!(!error.is::<PullExists>());
        assert_eq!(error.to_string(), REJECTED);
    }
    /// A creation GitHub accepts returns the pull request its answer holds, as the list reads
    /// one, where the answer is the pull request that was sent; otherwise its number alone, or
    /// nothing. Whatever the answer, an accepted creation is never a failure and is sent once:
    /// it once kept only the number, and an answer it could not read ended as "GitHub accepted
    /// the action but its response could not be read". Every other write keeps those failures.
    #[test]
    fn an_accepted_creation_returns_its_pull_request_and_never_fails() {
        let control = OperationControl::default();
        let created = |status: u16, body: String| {
            let mut client = creating(answer(status, &body));
            let sent = client
                .execute(Action::Create(reviewed()), &control)
                .unwrap_or_else(|error| panic!("{body}: {error:#}"));
            assert_eq!(client.0.requests.len(), 3, "one creation, sent once");
            (sent.created.expect("a creation"), sent.notice)
        };
        let made = || {
            let mut made = json!(pull());
            made["number"] = json!(203);
            made
        };
        const ACCEPTED: &str = "GitHub accepted action 203 at github.com/fixture/repo · feature (1111111111111111111111111111111111111111) → main (2222222222222222222222222222222222222222) · draft PR";
        for status in [200, 201] {
            let (created, notice) = created(status, made().to_string());
            let Created::Pull(pull) = created else {
                panic!("{created:?}");
            };
            assert_eq!((pull.number, pull.draft), (203, true));
            assert_eq!(
                (pull.head.branch.as_str(), pull.base.branch.as_str()),
                ("feature", "main")
            );
            assert_eq!(notice, ACCEPTED);
        }
        let other = |change: &dyn Fn(&mut Value)| {
            let mut made = made();
            change(&mut made);
            made.to_string()
        };
        for (name, body) in [
            ("the number alone", json!({"number":203}).to_string()),
            (
                "another head",
                other(&|made| made["head"]["ref"] = json!("other")),
            ),
            (
                "another base",
                other(&|made| made["base"]["ref"] = json!("release")),
            ),
            (
                "a closed one",
                other(&|made| made["state"] = json!("closed")),
            ),
            (
                "an invalid identity",
                other(&|made| made["head"]["sha"] = json!("x")),
            ),
        ] {
            let (created, notice) = created(201, body);
            assert!(
                matches!(created, Created::Number(203)),
                "{name}: {created:?}"
            );
            assert_eq!(notice, ACCEPTED, "{name}");
        }
        for body in ["<html>", "", "{}", r#"{"number":0}"#, r#"{"id":9}"#] {
            let (created, notice) = created(201, body.into());
            assert!(matches!(created, Created::Unread), "{body}: {created:?}");
            assert!(
                notice.starts_with("GitHub accepted the pull request at github.com/fixture/repo")
            );
        }
        // A comment's accepted answer that cannot be read stays the failure it was.
        let listed = pull();
        for (body, said) in [
            ("<html>", "could not be read"),
            ("{}", "returned no identity"),
        ] {
            let mut client = Client(Mock {
                responses: VecDeque::from([response(json!(listed)), answer(201, body)]),
                ..Default::default()
            });
            let comment = Action::Comment {
                pull: listed.capture(repo()),
                body: "Comment".into(),
            };
            let error = client.execute(comment, &control).unwrap_err();
            assert!(error.to_string().contains(said), "{error:#}");
        }
    }
    #[cfg(unix)]
    #[test]
    fn templates_read_captured_blobs_and_ignore_stored_symlinks() {
        use std::process::Command;
        let temp = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| {
            let output = Command::new("git")
                .arg("-C")
                .arg(temp.path())
                .args(args)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_AUTHOR_NAME", "Fixture")
                .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
                .env("GIT_COMMITTER_NAME", "Fixture")
                .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        git(&["init", "-q", "-b", "main"]);
        std::fs::create_dir_all(temp.path().join(".github/PULL_REQUEST_TEMPLATE")).unwrap();
        std::fs::write(
            temp.path().join(".github/PULL_REQUEST_TEMPLATE/feature.md"),
            "Captured template\n",
        )
        .unwrap();
        std::os::unix::fs::symlink(
            "/etc/passwd",
            temp.path().join(".github/pull_request_template.md"),
        )
        .unwrap();
        git(&["add", "."]);
        git(&["commit", "-qm", "Fixture template"]);
        std::fs::write(
            temp.path().join(".github/PULL_REQUEST_TEMPLATE/feature.md"),
            "Working edit",
        )
        .unwrap();
        let repo = gitturtle_core::GitRepository::open(temp.path()).unwrap();
        let templates = local_templates(
            &repo,
            "main",
            &gitturtle_core::HistoryCancellation::default(),
        )
        .unwrap();
        assert_eq!(templates.len(), 1);
        assert_eq!(templates[0].body, "Captured template\n");
        assert_eq!(templates[0].revision.len(), 40);

        // A cancel arriving once the listing has named the templates stops the read
        // before the next one, as a cancel the person asked for: the panel's Cancel or
        // the operation bar's while the templates are read. The panel's own test
        // cancels before the read starts, which stops the listing instead.
        let cancellation = gitturtle_core::HistoryCancellation::default();
        let listing = repo
            .search_tracked_paths(
                &gitturtle_core::PathScope::Revision("main".into()),
                "pull_request_template",
                &cancellation,
            )
            .unwrap();
        assert!(
            !listing.entries.is_empty(),
            "the listing names the template"
        );
        cancellation.cancel();
        let error =
            read_templates(&repo, listing, &cancellation).expect_err("the cancel stops the read");
        assert!(
            error
                .downcast_ref::<gitturtle_core::ReadCancelled>()
                .is_some(),
            "{error:#}"
        );
    }
}
