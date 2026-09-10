//! Captured provider patches and inline discussions. Parsing and API calls run
//! on the operation worker; presentation consumes bounded, owned rows.
use super::*;

pub(crate) const REVIEW_PAGE_SIZE: usize = 100;
const MAX_PATCH_ROWS: usize = 20_000;

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct PullFile {
    pub filename: String,
    #[serde(default)]
    pub previous_filename: Option<String>,
    pub status: String,
    pub additions: u32,
    pub deletions: u32,
    #[serde(default)]
    pub patch: Option<String>,
    #[serde(skip)]
    pub rows: Vec<PatchRow>,
    #[serde(skip)]
    pub unavailable: Option<String>,
}
#[derive(Clone, Debug)]
pub(crate) struct PatchRow {
    pub old: Option<u32>,
    pub new: Option<u32>,
    pub kind: u8,
    pub text: String,
    pub hunk: usize,
}
impl PatchRow {
    pub fn position(&self) -> Option<(DiffSide, u32)> {
        match self.kind {
            b'+' => self.new.map(|n| (DiffSide::Right, n)),
            b'-' => self.old.map(|n| (DiffSide::Left, n)),
            _ => None,
        }
    }
}
impl PullFile {
    pub fn prepare(&mut self) -> Result<()> {
        ensure!(
            safe_path(&self.filename) && self.previous_filename.as_deref().is_none_or(safe_path),
            "GitHub returned an invalid changed-file path"
        );
        let Some(patch) = self.patch.as_deref() else {
            self.unavailable = Some("GitHub did not provide a text patch for this file. Binary, empty rename, or oversized changes can be inspected through the captured local comparison.".into());
            return Ok(());
        };
        ensure!(
            patch.len() <= MAX_RESPONSE,
            "GitHub file patch exceeds the preview limit"
        );
        match parse_patch(patch, self.additions, self.deletions) {
            Ok(rows) => self.rows = rows,
            Err(error) => {
                self.unavailable = Some(format!(
                    "Inline selection unavailable: {error}. The exact supplied patch remains available in Source."
                ))
            }
        }
        Ok(())
    }
    pub fn selection(&self, anchor: usize, end: usize) -> Result<LineComment> {
        ensure!(
            self.unavailable.is_none(),
            "This patch cannot safely anchor inline comments"
        );
        let first = anchor.min(end);
        let last = anchor.max(end);
        let rows = self
            .rows
            .get(first..=last)
            .ok_or_else(|| anyhow!("Choose a changed line in the captured patch"))?;
        let (side, start) = rows[0]
            .position()
            .ok_or_else(|| anyhow!("Choose an added or deleted line"))?;
        for (offset, row) in rows.iter().enumerate() {
            ensure!(
                row.hunk == rows[0].hunk && row.position() == Some((side, start + offset as u32)),
                "A supported range contains consecutive changed lines on one side of one hunk. Choose additions or deletions separately"
            );
        }
        Ok(LineComment {
            path: self.filename.clone(),
            line: start + (last - first) as u32,
            side,
            start_line: (first != last).then_some(start),
            start_side: (first != last).then_some(side),
            body: String::new(),
        })
    }
}
fn parse_patch(patch: &str, additions: u32, deletions: u32) -> Result<Vec<PatchRow>> {
    let mut rows = Vec::new();
    let (mut old, mut new) = (0u32, 0u32);
    let (mut remaining_old, mut remaining_new) = (0u32, 0u32);
    let (mut added, mut deleted, mut hunk) = (0u32, 0u32, 0usize);
    for line in patch.lines() {
        ensure!(
            rows.len() < MAX_PATCH_ROWS,
            "Patch exceeds the 20,000-row inline preview limit"
        );
        if let Some(header) = line.strip_prefix("@@ ") {
            ensure!(
                remaining_old == 0 && remaining_new == 0,
                "GitHub supplied an incomplete diff hunk"
            );
            let mut parts = header.split_whitespace();
            let a = parts
                .next()
                .and_then(|s| s.strip_prefix('-'))
                .ok_or_else(|| anyhow!("Invalid old hunk range"))?;
            let b = parts
                .next()
                .and_then(|s| s.strip_prefix('+'))
                .ok_or_else(|| anyhow!("Invalid new hunk range"))?;
            ensure!(parts.next() == Some("@@"), "Invalid hunk header");
            let range = |value: &str| -> Result<(u32, u32)> {
                let (start, count) = value.split_once(',').unwrap_or((value, "1"));
                Ok((start.parse()?, count.parse()?))
            };
            (old, remaining_old) = range(a)?;
            (new, remaining_new) = range(b)?;
            ensure!(
                old.checked_add(remaining_old).is_some()
                    && new.checked_add(remaining_new).is_some(),
                "Hunk line range exceeds the supported limit"
            );
            hunk += 1;
            rows.push(PatchRow {
                old: None,
                new: None,
                kind: b'@',
                text: line.into(),
                hunk,
            });
            continue;
        }
        if line.starts_with("\\ No newline at end of file") {
            rows.push(PatchRow {
                old: None,
                new: None,
                kind: b'\\',
                text: line.into(),
                hunk,
            });
            continue;
        }
        ensure!(hunk > 0, "Patch contains text outside a diff hunk");
        let kind = *line
            .as_bytes()
            .first()
            .ok_or_else(|| anyhow!("Patch contains an unmarked line"))?;
        let (old_number, new_number) = match kind {
            b'+' => {
                ensure!(
                    remaining_new > 0 && new > 0,
                    "Patch new-line count is inconsistent"
                );
                remaining_new -= 1;
                let n = new;
                new += 1;
                added += 1;
                (None, Some(n))
            }
            b'-' => {
                ensure!(
                    remaining_old > 0 && old > 0,
                    "Patch old-line count is inconsistent"
                );
                remaining_old -= 1;
                let n = old;
                old += 1;
                deleted += 1;
                (Some(n), None)
            }
            b' ' => {
                ensure!(
                    remaining_old > 0 && remaining_new > 0 && old > 0 && new > 0,
                    "Patch context count is inconsistent"
                );
                remaining_old -= 1;
                remaining_new -= 1;
                let pair = (Some(old), Some(new));
                old += 1;
                new += 1;
                pair
            }
            _ => bail!("Patch contains an unsupported line marker"),
        };
        rows.push(PatchRow {
            old: old_number,
            new: new_number,
            kind,
            text: line.into(),
            hunk,
        });
    }
    ensure!(
        hunk > 0
            && remaining_old == 0
            && remaining_new == 0
            && added == additions
            && deleted == deletions,
        "The supplied patch does not cover the complete reported changes"
    );
    Ok(rows)
}

pub(crate) fn position_label(comment: &LineComment) -> String {
    format!(
        "{} · {} {}{}",
        comment.path,
        if comment.side == DiffSide::Left {
            "Before"
        } else {
            "After"
        },
        comment
            .start_line
            .map(|n| format!("{n}–"))
            .unwrap_or_default(),
        comment.line
    )
}

impl<T: Transport> Client<T> {
    pub fn files(
        &mut self,
        pull: &CapturedPull,
        page: u32,
        control: &OperationControl,
    ) -> Result<Page<PullFile>> {
        ensure!(
            (1..=30).contains(&page),
            "GitHub exposes at most 3,000 changed files in 100-file pages"
        );
        self.revalidate(pull, control)?;
        let response = self.get(
            format!(
                "{}/pulls/{}/files?per_page={REVIEW_PAGE_SIZE}&page={page}",
                pull.repository.endpoint(),
                pull.number
            ),
            control,
        )?;
        let items: Vec<PullFile> = response.json()?;
        ensure!(
            items.len() <= REVIEW_PAGE_SIZE,
            "Too many changed files in one page"
        );
        for item in &items {
            ensure!(
                safe_path(&item.filename)
                    && item.previous_filename.as_deref().is_none_or(safe_path),
                "GitHub returned an invalid changed-file path"
            );
        }
        self.revalidate(pull, control)?;
        Ok(Page {
            items,
            page,
            has_next: response.next_page() && page < 30,
        })
    }
}

/// Explicit built-in native QA fixture. This transport has no credential,
/// process, URL-following or network path, including when a fixture request fails.
pub(crate) enum UiTransport {
    Live(super::transport::GhTransport),
    #[cfg(test)]
    Fixture {
        moved: bool,
    },
    NativeFixture {
        moved: bool,
        session: std::sync::Arc<std::sync::Mutex<super::native_fixture::Session>>,
    },
}
impl UiTransport {
    pub fn open(fixture: bool, moved: bool) -> Result<Self> {
        if fixture {
            Ok(Self::NativeFixture {
                moved,
                session: super::native_fixture::session(),
            })
        } else {
            Ok(Self::Live(super::transport::GhTransport::stored()?))
        }
    }
    pub fn login(&self) -> &str {
        match self {
            Self::Live(transport) => transport.login(),
            #[cfg(test)]
            Self::Fixture { .. } => "offline-reviewer",
            Self::NativeFixture { .. } => "offline-reviewer",
        }
    }
}
impl Transport for UiTransport {
    fn request(&mut self, request: Request, control: &OperationControl) -> Result<Response> {
        match self {
            Self::Live(transport) => transport.request(request, control),
            #[cfg(test)]
            Self::Fixture { moved } => fixture_response(request, *moved, control),
            Self::NativeFixture { moved, session } => {
                if request.endpoint == "graphql" {
                    session
                        .lock()
                        .map_err(|_| anyhow!("Offline fixture session is unavailable"))?
                        .request(request, *moved, control)
                } else {
                    fixture_response(request, *moved, control)
                }
            }
        }
    }
}
pub(crate) fn fixture_pull(moved: bool) -> PullRequest {
    serde_json::from_value(json!({"number":42,"title":"Keep review context through repository changes","body":"## Preserve the review you are doing\n\nKeeps the selected file, inline comments, and captured commit identities together.\n\n- Retain exact draft text\n- Make moved heads explicit\n- Keep all review interaction native","state":"open","draft":false,"user":{"login":"river-chen"},"head":{"sha":if moved {"3333333333333333333333333333333333333333"} else {"1111111111111111111111111111111111111111"},"ref":"feature/preserve-review-context","label":"river-chen:feature/preserve-review-context"},"base":{"sha":"2222222222222222222222222222222222222222","ref":"main","label":"gitturtle-fixture:main"}})).expect("built-in fixture identity")
}
fn fixture_response(request: Request, moved: bool, control: &OperationControl) -> Result<Response> {
    ensure!(!control.is_cancelled(), "Offline fixture request cancelled");
    if request.endpoint == "graphql" {
        return super::native_fixture::Session::default().request(request, moved, control);
    }
    let root = "repos/gitturtle-fixture/native-review";
    ensure!(
        request.endpoint == "user" || request.endpoint.starts_with(&format!("{root}/")),
        "Offline fixture supports only gitturtle-fixture/native-review; no network request was made"
    );
    let value = if request.method == "POST" {
        ensure!(
            [
                format!("{root}/pulls/42/reviews"),
                format!("{root}/issues/42/comments")
            ]
            .contains(&request.endpoint),
            "This action is outside the offline fixture"
        );
        json!({"id":4242})
    } else if request.endpoint == "user" {
        json!({"login":"offline-reviewer"})
    } else if request.endpoint.contains("/files?") {
        json!([
            {"filename":"src/review/session.rs","status":"modified","additions":5,"deletions":2,"patch":"@@ -1,5 +1,7 @@\n pub fn switch_repository() {\n-    draft.clear();\n+    draft.flush_captured();\n+    selection.retain();\n+    focus.restore();\n     tabs.activate();\n }\n \n@@ -20,3 +22,4 @@\n fn refresh_review() {\n-    comments.remap();\n+    head.revalidate();\n+    comments.keep_original_positions();\n }"},
            {"filename":"docs/review/界面-guide.md","previous_filename":"docs/review-guide.md","status":"renamed","additions":2,"deletions":1,"patch":"@@ -1,3 +1,4 @@\n # Review context\n-Comments follow the current branch.\n+Comments belong to the captured commit.\n+Review the new head before moving any text.\n Keep the exact source available."},
            {"filename":"assets/review-workflow.png","status":"modified","additions":0,"deletions":0},
            {"filename":"src/oversized_snapshot.rs","status":"modified","additions":200,"deletions":100,"patch":"@@ -1,2 +1,3 @@\n unchanged\n-old\n+new"}
        ])
    } else if request.endpoint.contains("/comments?") {
        json!([
            {"id":100,"path":"src/review/session.rs","body":"Could we preserve the exact draft before switching repositories?","user":{"login":"avery"},"line":2,"side":"RIGHT","commit_id":"1111111111111111111111111111111111111111","original_commit_id":"1111111111111111111111111111111111111111"},
            {"id":101,"in_reply_to_id":100,"path":"src/review/session.rs","body":"Yes — this now flushes the captured snapshot and restores useful focus.","user":{"login":"river-chen"},"line":2,"side":"RIGHT","commit_id":"1111111111111111111111111111111111111111"},
            {"id":102,"path":"src/review/session.rs","body":"Earlier approach: remap comments when the head changes. Retained here for context; this position is outdated.","user":{"login":"sam"},"line":null,"original_line":21,"side":"LEFT","commit_id":"0000000000000000000000000000000000000000","original_commit_id":"0000000000000000000000000000000000000000"},
            {"id":104,"in_reply_to_id":99,"path":"docs/review/界面-guide.md","body":"This reply remains readable when its opening comment is on another page.","user":{"login":"morgan"},"line":3,"side":"RIGHT","commit_id":"1111111111111111111111111111111111111111"}
        ])
    } else if request.endpoint.contains("/reviews?") {
        json!([{"user":{"login":"avery"},"state":"COMMENTED","commit_id":"1111111111111111111111111111111111111111"}])
    } else if request.endpoint.contains("/check-runs?") {
        json!({"check_runs":[{"name":"macOS · native tests","conclusion":"success"},{"name":"Linux · portable core","status":"in_progress"}]})
    } else if request.endpoint.contains("/status?") {
        json!({"statuses":[{"context":"Formatting","state":"success"}]})
    } else if request.endpoint.contains("/pulls?") {
        json!([fixture_pull(moved)])
    } else if request.endpoint == format!("{root}/pulls/42") {
        json!(fixture_pull(moved))
    } else {
        bail!("Unsupported offline fixture request; no network request was made")
    };
    Ok(Response {
        status: 200,
        headers: BTreeMap::new(),
        body: serde_json::to_vec(&value)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn files() -> Vec<PullFile> {
        let mut files = Client(UiTransport::Fixture { moved: false })
            .files(
                &fixture_pull(false)
                    .capture(Repository::parse("gitturtle-fixture/native-review").unwrap()),
                1,
                &OperationControl::default(),
            )
            .unwrap()
            .items;
        assert!(
            files.iter().all(|file| file.rows.is_empty()),
            "metadata loading must not eagerly prepare file rows"
        );
        for file in &mut files {
            file.prepare().unwrap();
        }
        files
    }

    #[test]
    fn exact_patch_ranges_refuse_opposite_sides_hunk_boundaries_and_incomplete_content() {
        let files = files();
        let file = &files[0];
        let selected = file.selection(3, 5).unwrap();
        assert_eq!(
            (selected.start_line, selected.line, selected.side),
            (Some(2), 4, DiffSide::Right)
        );
        assert_eq!(file.selection(5, 3).unwrap(), selected);
        assert!(file.selection(2, 3).is_err());
        assert!(file.selection(3, 11).is_err());
        assert!(files[2].unavailable.is_some());
        assert!(files[3].unavailable.is_some());
        assert_eq!(
            files[1].selection(3, 4).unwrap().path,
            "docs/review/界面-guide.md"
        );
    }
    #[test]
    fn fixture_never_falls_through_to_network_and_moved_head_refuses_read() {
        let pull = fixture_pull(false)
            .capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
        let mut client = Client(UiTransport::Fixture { moved: true });
        assert!(
            client
                .files(&pull, 1, &OperationControl::default())
                .unwrap_err()
                .to_string()
                .contains("moved")
        );
        assert!(
            client
                .list(
                    &Repository::parse("other/repo").unwrap(),
                    1,
                    &OperationControl::default()
                )
                .is_err()
        );
    }
    #[test]
    fn same_side_range_serializes_and_old_single_line_drafts_remain_readable() {
        let old: LineComment = serde_json::from_value(
            json!({"path":"a","line":2,"side":"RIGHT","body":"Exact text\n"}),
        )
        .unwrap();
        assert!(old.start_line.is_none());
        let mut review = ReviewDraft {
            pull: fixture_pull(false)
                .capture(Repository::parse("gitturtle-fixture/native-review").unwrap()),
            body: String::new(),
            event: ReviewEvent::Comment,
            comments: vec![files()[0].selection(3, 5).unwrap()],
            composing: None,
            discussion: false,
        };
        review.comments[0].body = "Inspect these together".into();
        let payload = serde_json::to_value(&review.comments).unwrap();
        assert_eq!(payload[0]["start_line"], 2);
        assert_eq!(payload[0]["line"], 4);
        assert_eq!(payload[0]["start_side"], "RIGHT");
        Client(UiTransport::Fixture { moved: false })
            .execute(Action::Review(review), &OperationControl::default())
            .unwrap();
    }
    #[test]
    fn head_movement_during_file_read_rejects_the_page_without_posting() {
        struct Moving {
            requests: Vec<Request>,
        }
        impl Transport for Moving {
            fn request(
                &mut self,
                request: Request,
                control: &OperationControl,
            ) -> Result<Response> {
                self.requests.push(request.clone());
                fixture_response(request, self.requests.len() == 3, control)
            }
        }
        let mut client = Client(Moving { requests: vec![] });
        let pull = fixture_pull(false)
            .capture(Repository::parse("gitturtle-fixture/native-review").unwrap());
        assert!(
            client
                .files(&pull, 1, &OperationControl::default())
                .unwrap_err()
                .to_string()
                .contains("moved")
        );
        assert_eq!(client.0.requests.len(), 3);
        assert!(
            client
                .0
                .requests
                .iter()
                .all(|request| request.method == "GET")
        );
    }
    #[test]
    fn malformed_range_and_unfinished_composer_never_dispatch() {
        struct Never;
        impl Transport for Never {
            fn request(&mut self, _: Request, _: &OperationControl) -> Result<Response> {
                panic!("invalid review must not dispatch")
            }
        }
        let mut comment = files()[0].selection(3, 5).unwrap();
        comment.body = "Keep exact text".into();
        let mut draft = ReviewDraft {
            pull: fixture_pull(false)
                .capture(Repository::parse("gitturtle-fixture/native-review").unwrap()),
            body: "Summary".into(),
            event: ReviewEvent::Comment,
            comments: vec![comment.clone()],
            composing: None,
            discussion: false,
        };
        draft.comments[0].start_side = Some(DiffSide::Left);
        assert!(
            Client(Never)
                .execute(Action::Review(draft.clone()), &OperationControl::default())
                .unwrap_err()
                .to_string()
                .contains("position")
        );
        draft.comments[0] = comment.clone();
        draft.composing = Some(comment);
        assert!(
            Client(Never)
                .execute(Action::Review(draft), &OperationControl::default())
                .unwrap_err()
                .to_string()
                .contains("unfinished")
        );
    }
}
