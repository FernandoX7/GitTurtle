//! Explicit GitHub collaboration. Call from the serialized operation executor;
//! opening the panel and ordinary local refresh never dispatch network traffic.
pub(crate) mod drafts;
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
}
#[derive(Clone, Debug)]
pub(crate) enum Action {
    Create(NewPull),
    Comment { pull: CapturedPull, body: String },
    Review(ReviewDraft),
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
                    .map(|v| format!(" Retry after {v} seconds."))
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
    fn next_page(&self) -> bool {
        self.headers
            .get("link")
            .is_some_and(|v| v.split(',').any(|p| p.contains("rel=\"next\"")))
    }
}
pub(crate) trait Transport {
    fn request(&mut self, request: Request, control: &OperationControl) -> Result<Response>;
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
    pub fn execute(&mut self, action: Action, control: &OperationControl) -> Result<String> {
        ensure!(
            !control.is_cancelled(),
            "GitHub action cancelled before submission"
        );
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
        };
        ensure!(
            !control.is_cancelled(),
            "GitHub action cancelled before submission"
        );
        let response = self.0.request(Request { method: "POST", endpoint, body: Some(body) }, control).map_err(|error| anyhow!("{error}. The GitHub outcome may be uncertain. Check the destination before resubmitting; no automatic retry was made."))?;
        response.check(true)?;
        let value: Value = response.json().map_err(|_| anyhow!("GitHub accepted the action but its response could not be read. Check the destination before resubmitting; no automatic retry was made."))?;
        let id = value["number"].as_u64().or_else(|| value["id"].as_u64()).ok_or_else(|| anyhow!("GitHub accepted the action but returned no identity. Check the destination before resubmitting."))?;
        Ok(format!(
            "GitHub accepted action {id} at {}",
            action.destination()
        ))
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
pub(crate) fn local_templates(
    repo: &gitturtle_core::GitRepository,
    revision: &str,
) -> Result<Vec<Template>> {
    let cancellation = gitturtle_core::HistoryCancellation::default();
    let listing = repo.search_tracked_paths(
        &gitturtle_core::PathScope::Revision(revision.into()),
        "pull_request_template",
        &cancellation,
    )?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
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
        let templates = local_templates(&repo, "main").unwrap();
        assert_eq!(templates.len(), 1);
        assert_eq!(templates[0].body, "Captured template\n");
        assert_eq!(templates[0].revision.len(), 40);
    }
}
