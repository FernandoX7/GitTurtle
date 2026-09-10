//! Test-only probe compiled against copied GitHub modules and exact persistence
//! functions. The Transport below accepts only synthetic GET requests.
use super::*;
use crate::github::review::{PullFile, threads};
use std::{collections::HashSet, time::Instant};

const FILES: usize = 300;
const ROOT_COMMENTS: usize = 1000;
const COMMENTS: usize = ROOT_COMMENTS * 2;
const PATCH_LINES: u32 = 200;

#[derive(Default)]
struct OfflinePages {
    requests: Vec<String>,
    moved: bool,
    move_after_payload: bool,
    oversized_page: bool,
    truncated_payload: bool,
}

fn captured() -> CapturedPull {
    super::super::review::fixture_pull(false)
        .capture(Repository::parse("gitturtle-fixture/large-offline-review").unwrap())
}

fn patch(index: usize) -> String {
    let mut patch = format!("@@ -1,{PATCH_LINES} +1,{PATCH_LINES} @@\n");
    for sign in ['-', '+'] {
        for line in 1..=PATCH_LINES {
            patch.push_str(&format!(
                "{sign}file {index:03} line {line:03} {}\n",
                "x".repeat(48)
            ));
        }
    }
    patch
}

impl Transport for OfflinePages {
    fn request(&mut self, request: Request, control: &OperationControl) -> Result<Response> {
        ensure!(!control.is_cancelled(), "Offline probe cancelled");
        ensure!(
            request.method == "GET" && request.body.is_none(),
            "Probe prohibits writes"
        );
        let root = "repos/gitturtle-fixture/large-offline-review/pulls/42";
        ensure!(
            request.endpoint == root || request.endpoint.starts_with(&format!("{root}/")),
            "Unexpected probe destination"
        );
        self.requests.push(request.endpoint.clone());
        let mut headers = BTreeMap::new();
        let value = if request.endpoint == root {
            let mut pull = super::super::review::fixture_pull(false);
            if self.moved {
                pull.head.sha = "3".repeat(40);
            }
            serde_json::to_value(pull)?
        } else {
            let page: usize = request
                .endpoint
                .split("&page=")
                .nth(1)
                .and_then(|tail| tail.split('&').next())
                .ok_or_else(|| anyhow!("Missing page"))?
                .parse()?;
            ensure!(page > 0, "Invalid page");
            let files = request.endpoint.starts_with(&format!("{root}/files?"));
            ensure!(
                files || request.endpoint.starts_with(&format!("{root}/comments?")),
                "Unexpected read"
            );
            let total = if files { FILES } else { COMMENTS };
            ensure!(page <= total / 100, "Unexpected extra page");
            if page * 100 < total {
                // The real client may only treat this as a next-page indicator;
                // it must reconstruct its own endpoint, never follow this URL.
                headers.insert(
                    "link".into(),
                    "<https://invalid.example/ignored>; rel=\"next\"".into(),
                );
            }
            let start = (page - 1) * 100;
            let count = if self.oversized_page { 101 } else { 100 };
            let items: Vec<Value> = (start..start + count).map(|index| {
                if files {
                    json!({"filename":format!("src/group-{}/file-{index:03}-🐢.rs",index / 100),"status":"modified","additions":PATCH_LINES,"deletions":PATCH_LINES,"patch":patch(index)})
                } else {
                    let root_id = index % ROOT_COMMENTS + 1;
                    json!({"id":index + 1,"path":format!("src/file-{:03}.rs", index % FILES),"body":format!("Comment {index}: {}", "review text ".repeat(80)),"user":{"login":"offline-probe"},"in_reply_to_id":if index >= ROOT_COMMENTS {Some(root_id)} else {None},"line":2,"side":"RIGHT","commit_id":"1".repeat(40),"original_commit_id":"1".repeat(40)})
                }
            }).collect();
            if self.move_after_payload {
                self.moved = true;
            }
            if self.truncated_payload {
                return Ok(Response {
                    status: 200,
                    headers,
                    body: b"[{\"filename\":".to_vec(),
                });
            }
            Value::Array(items)
        };
        let body = serde_json::to_vec(&value)?;
        ensure!(
            body.len() <= MAX_RESPONSE,
            "Probe fixture exceeded response budget"
        );
        Ok(Response {
            status: 200,
            headers,
            body,
        })
    }
}

#[test]
fn large_offline_pr_probe() -> Result<()> {
    let control = OperationControl::default();
    let pull = captured();
    let mut client = Client(OfflinePages::default());
    let start = Instant::now();
    let mut names = HashSet::new();
    let mut prepared_rows = 0;
    let mut max_page_payload_bytes = 0;
    for number in 1..=3 {
        let before = client.0.requests.len();
        let mut page = client.files(&pull, number, &control)?;
        assert_eq!(
            client.0.requests.len() - before,
            3,
            "one page with pre/post identity checks"
        );
        assert_eq!(page.items.len(), 100);
        assert_eq!(page.has_next, number < 3);
        assert!(page.items.iter().all(|file| file.rows.is_empty()));
        max_page_payload_bytes = max_page_payload_bytes.max(
            page.items
                .iter()
                .map(|file| file.patch.as_ref().unwrap().len())
                .sum::<usize>(),
        );
        for file in &mut page.items {
            assert!(names.insert(file.filename.clone()));
            file.prepare()?;
            assert!(file.unavailable.is_none());
            assert_eq!(file.rows.len(), PATCH_LINES as usize * 2 + 1);
            let selected = file.selection(PATCH_LINES as usize + 1, PATCH_LINES as usize * 2)?;
            assert_eq!(
                (selected.start_line, selected.line, selected.side),
                (Some(1), PATCH_LINES, DiffSide::Right)
            );
            assert!(
                file.selection(PATCH_LINES as usize, PATCH_LINES as usize + 1)
                    .is_err()
            );
            prepared_rows += file.rows.len();
        }
        // Match the live panel's per-page replacement, not an all-files cache.
        drop(page);
    }
    assert_eq!(names.len(), FILES);
    assert_eq!(prepared_rows, FILES * (PATCH_LINES as usize * 2 + 1));
    let file_ms = start.elapsed().as_secs_f64() * 1000.;

    let start = Instant::now();
    let mut all_comments = Vec::new();
    let mut missing_root_pages = 0;
    for number in 1..=20 {
        let before = client.0.requests.len();
        let page = client.comments(&pull, number, &control)?;
        assert_eq!(client.0.requests.len() - before, 3);
        assert_eq!(page.items.len(), 100);
        assert_eq!(page.has_next, number < 20);
        let grouped = threads(&page.items);
        assert_eq!(grouped.len(), 100);
        if number > 10 {
            assert!(grouped.iter().all(|thread| thread.missing_root));
            missing_root_pages += 1;
        } else {
            assert!(grouped.iter().all(|thread| !thread.missing_root));
        }
        all_comments.extend(page.items);
    }
    assert_eq!(all_comments.len(), COMMENTS);
    assert_eq!(
        all_comments
            .iter()
            .map(|comment| comment.id)
            .collect::<HashSet<_>>()
            .len(),
        COMMENTS
    );
    // This additionally stresses the production grouping helper with the full
    // corpus; live UI retains only one 100-comment page at a time.
    let grouped = threads(&all_comments);
    assert_eq!(grouped.len(), ROOT_COMMENTS);
    assert!(
        grouped
            .iter()
            .all(|thread| !thread.missing_root && thread.comments.len() == 2)
    );
    let comment_ms = start.elapsed().as_secs_f64() * 1000.;
    assert_eq!(client.0.requests.len(), (3 + 20) * 3);

    let before = client.0.requests.len();
    assert!(client.files(&pull, 31, &control).is_err());
    assert_eq!(client.0.requests.len(), before);
    let cancelled = OperationControl::default();
    cancelled.cancel();
    assert!(client.comments(&pull, 1, &cancelled).is_err());
    assert_eq!(client.0.requests.len(), before);
    for transport in [
        OfflinePages {
            move_after_payload: true,
            ..Default::default()
        },
        OfflinePages {
            oversized_page: true,
            ..Default::default()
        },
        OfflinePages {
            truncated_payload: true,
            ..Default::default()
        },
    ] {
        let mut client = Client(transport);
        assert!(client.files(&pull, 1, &control).is_err());
        assert!(client.0.requests.len() <= 3, "failed reads must not retry");
    }
    let mut oversized_patch: PullFile = serde_json::from_value(
        json!({"filename":"oversized.rs","status":"modified","additions":20_000,"deletions":0,"patch":format!("@@ -0,0 +1,20000 @@\n{}", "+x\n".repeat(20_000))}),
    )?;
    oversized_patch.prepare()?;
    assert!(
        oversized_patch
            .unavailable
            .as_deref()
            .is_some_and(|message| message.contains("20,000"))
    );
    assert!(oversized_patch.rows.is_empty());

    let directory = tempfile::tempdir()?;
    let path = directory.path().join("isolated-github-drafts.json");
    let create = |index: usize| {
        let mut captured = pull.clone();
        captured.number = index as u64 + 1;
        Draft::Review(ReviewDraft {
            pull: captured,
            body: format!("Exact draft {index}\n{}\nEnd 🐢", "b".repeat(4096)),
            event: ReviewEvent::Comment,
            comments: (0..100)
                .map(|line| LineComment {
                    path: format!("src/draft-{index:03}.rs"),
                    line: line + 1,
                    side: DiffSide::Right,
                    start_line: None,
                    start_side: None,
                    body: format!("Comment {line}\n{}\nEnd 🐢", "c".repeat(512)),
                })
                .collect(),
            composing: None,
            discussion: false,
        })
    };
    let drafts = (0..128).map(create).collect::<Vec<_>>();
    let start = Instant::now();
    save_batch_at(&path, drafts.iter().cloned())?;
    let original = std::fs::read(&path)?;
    assert!(original.len() < MAX_STORE);
    let restored = load_at(&path)?;
    assert_eq!(restored.entries.len(), 128);
    assert_eq!(
        serde_json::to_value(&restored.entries)?,
        serde_json::to_value(&drafts)?
    );
    let mut changed = create(0);
    let Draft::Review(review) = &mut changed else {
        unreachable!()
    };
    review.body = "Latest exact draft\n🧪\n".into();
    assert!(save_batch_at(&path, [changed.clone(), create(128)]).is_err());
    assert_eq!(
        std::fs::read(&path)?,
        original,
        "overflow must preserve earlier entries and the attempted update"
    );
    save_batch_at(&path, [changed.clone()])?;
    let final_store = load_at(&path)?;
    assert_eq!(final_store.entries.len(), 128);
    assert_eq!(
        serde_json::to_value(&final_store.entries[0])?,
        serde_json::to_value(changed)?
    );
    assert_eq!(
        serde_json::to_value(&final_store.entries[127])?,
        serde_json::to_value(&drafts[127])?
    );
    let draft_ms = start.elapsed().as_secs_f64() * 1000.;
    println!(
        "LARGE_PR_PROBE_RESULT={}",
        json!({
            "files": FILES, "file_pages": 3, "prepared_patch_rows": prepared_rows,
            "max_page_patch_bytes": max_page_payload_bytes, "comments": COMMENTS,
            "comment_pages": 20, "aggregate_threads": grouped.len(),
            "pages_with_missing_roots": missing_root_pages, "successful_get_requests": client.0.requests.len(),
            "saved_drafts": 128, "inline_comments_per_draft": 100, "original_store_bytes": original.len(),
            "file_work_ms": file_ms, "comment_work_ms": comment_ms, "draft_work_ms": draft_ms,
            "network_requests": 0, "post_requests": 0
        })
    );
    Ok(())
}
