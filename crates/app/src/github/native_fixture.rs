//! Stateful, synthetic GitHub conversations for explicit native QA. No network,
//! credentials, filesystem, or fallback transport exists here.
use super::*;
use std::sync::{Arc, Mutex, OnceLock};

const REPOSITORY_ID: &str = "R_GitTurtleFixture";
const PULL_ID: &str = "PR_GitTurtleFixture42";
const HEAD: &str = "1111111111111111111111111111111111111111";

#[derive(Default)]
pub(crate) struct Session {
    replies: BTreeMap<String, Vec<Value>>,
    resolved: BTreeMap<String, bool>,
    next_reply: u64,
}
pub(crate) fn session() -> Arc<Mutex<Session>> {
    static SESSION: OnceLock<Arc<Mutex<Session>>> = OnceLock::new();
    SESSION
        .get_or_init(|| Arc::new(Mutex::new(Session::default())))
        .clone()
}

fn repository() -> Value {
    json!({"id":REPOSITORY_ID,"nameWithOwner":"gitturtle-fixture/native-review"})
}
fn pull(moved: bool) -> Value {
    let pull = review::fixture_pull(moved);
    json!({"id":PULL_ID,"number":42,"state":"OPEN","headRefOid":pull.head.sha,
        "baseRefOid":pull.base.sha,"headRefName":pull.head.branch,"baseRefName":pull.base.branch})
}
fn page(nodes: Vec<Value>, total: usize, next: Option<&str>) -> Value {
    json!({"nodes":nodes,"totalCount":total,"pageInfo":{"hasNextPage":next.is_some(),"endCursor":next}})
}
fn comment(id: &str, author: &str, body: &str, root: Option<&str>, old: bool) -> Value {
    let oid = if old {
        "0000000000000000000000000000000000000000"
    } else {
        HEAD
    };
    json!({"id":id,"author":{"login":author},"body":body,"createdAt":"2026-09-10T14:00:00Z",
        "url":format!("https://github.com/gitturtle-fixture/native-review/pull/42#discussion_{id}"),
        "replyTo":root.map(|id|json!({"id":id})),"commit":{"oid":oid},"originalCommit":{"oid":oid}})
}
impl Session {
    fn thread(&self, id: &str, moved: bool, after: Option<&str>, initial: bool) -> Result<Value> {
        ensure!(
            [
                "PRRT_current",
                "PRRT_outdated",
                "PRRT_incomplete",
                "PRRT_resolved",
                "PRRT_paginated",
                "PRRT_refused"
            ]
            .contains(&id),
            "Unknown offline conversation"
        );
        ensure!(
            after.is_none() || after == Some("fixture-comments-20"),
            "Unsupported offline comment cursor"
        );
        let old = id == "PRRT_outdated";
        let root_id = format!("{id}_root");
        let opening = match id {
            "PRRT_current" => "Could we preserve the exact draft before switching repositories?",
            "PRRT_outdated" => {
                "Earlier approach: remap comments when the head changes. This original position remains available for context."
            }
            "PRRT_incomplete" => {
                "The opening comment is unavailable; keep replies readable without inventing a target."
            }
            "PRRT_resolved" => {
                "The saved draft now restores to its original conversation. Thanks for keeping the context together."
            }
            "PRRT_paginated" => {
                "A longer conversation exercises explicit comment pagination and bounded native scrolling."
            }
            _ => {
                "This conversation is readable, but this account cannot reply or change its resolution."
            }
        };
        let root = comment(&root_id, "avery", opening, None, old);
        let mut comments = vec![root.clone()];
        if id == "PRRT_current" {
            comments.push(comment(
                "PRRC_current_reply",
                "river-chen",
                "Yes — the captured snapshot and exact text survive navigation.",
                Some(&root_id),
                false,
            ));
        }
        if id == "PRRT_paginated" {
            for n in 1..=22 {
                comments.push(comment(
                    &format!("PRRC_page_{n}"),
                    "sam",
                    &format!("Context note {n}: keep the reply attached to this conversation."),
                    Some(&root_id),
                    false,
                ));
            }
        }
        if let Some(replies) = self.replies.get(id) {
            comments.extend(replies.iter().cloned());
        }
        if id == "PRRT_incomplete" {
            comments = vec![comment(
                "PRRC_orphan",
                "morgan",
                opening,
                Some(&root_id),
                false,
            )];
        }
        let total = comments.len();
        let (visible, next) = if after.is_some() {
            (comments.into_iter().skip(20).collect(), None)
        } else if initial && total > 20 {
            (
                comments.into_iter().take(20).collect(),
                Some("fixture-comments-20"),
            )
        } else {
            (comments, None)
        };
        let resolved = self
            .resolved
            .get(id)
            .copied()
            .unwrap_or(id == "PRRT_resolved");
        let allowed = id != "PRRT_refused";
        Ok(
            json!({"id":id,"path":if id=="PRRT_incomplete" {"docs/review/界面-guide.md"} else {"src/review/session.rs"},
            "diffSide":if old {"LEFT"}else{"RIGHT"},"startDiffSide":null,"line":if old {None}else{Some(2)},"startLine":null,
            "originalLine":if old {21}else{2},"originalStartLine":null,"isResolved":resolved,"isOutdated":old,
            "resolvedBy":if resolved {Some(json!({"login":"offline-reviewer"}))}else{None},
            "viewerCanReply":allowed,"viewerCanResolve":allowed&&!resolved,"viewerCanUnresolve":allowed&&resolved,
            "repository":repository(),"pullRequest":pull(moved),
            "root":page(if id=="PRRT_incomplete" {vec![Value::Null]}else{vec![root]},total,if total>1{Some("fixture-root-next")}else{None}),
            "comments":page(visible,total,next)}),
        )
    }

    pub(crate) fn request(
        &mut self,
        request: Request,
        moved: bool,
        control: &OperationControl,
    ) -> Result<Response> {
        ensure!(!control.is_cancelled(), "Offline fixture request cancelled");
        ensure!(
            request.endpoint == "graphql" && request.method == "POST",
            "Unsupported offline conversation request"
        );
        let body = request
            .body
            .ok_or_else(|| anyhow!("Missing offline query"))?;
        let query = body["query"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing offline query"))?;
        let variables = &body["variables"];
        let viewer = json!({"id":"U_OfflineReviewer","login":"offline-reviewer"});
        let data = if query.starts_with("query GitTurtleReviewThreads") {
            ensure!(
                variables["owner"] == "gitturtle-fixture"
                    && variables["name"] == "native-review"
                    && variables["number"] == 42,
                "Offline fixture refuses another repository or PR"
            );
            let after = variables["after"].as_str();
            ensure!(
                after.is_none() || after == Some("fixture-threads-3"),
                "Unsupported offline conversation cursor"
            );
            let ids = if after.is_none() {
                ["PRRT_current", "PRRT_outdated", "PRRT_incomplete"]
            } else {
                ["PRRT_resolved", "PRRT_paginated", "PRRT_refused"]
            };
            let threads = ids
                .into_iter()
                .map(|id| self.thread(id, moved, None, true))
                .collect::<Result<Vec<_>>>()?;
            let mut pr = pull(moved);
            pr["reviewThreads"] = page(
                threads,
                6,
                if after.is_none() {
                    Some("fixture-threads-3")
                } else {
                    None
                },
            );
            let mut repo = repository();
            repo["pullRequest"] = pr;
            json!({"viewer":viewer,"repository":repo})
        } else {
            let id = variables["thread"]
                .as_str()
                .ok_or_else(|| anyhow!("Missing offline conversation identity"))?;
            if query.starts_with("query GitTurtleThreadComments") {
                json!({"viewer":viewer,"node":self.thread(id,moved,variables["after"].as_str(),false)?})
            } else if query.starts_with("mutation GitTurtleReply") {
                let thread = self.thread(id, moved, None, false)?;
                ensure!(
                    thread["viewerCanReply"] == true && id != "PRRT_incomplete",
                    "Offline fixture refuses this reply"
                );
                let text = variables["body"]
                    .as_str()
                    .ok_or_else(|| anyhow!("Missing offline reply text"))?;
                validate_text(text, true)?;
                ensure!(
                    self.replies.get(id).map_or(0, Vec::len) < 75
                        && self
                            .replies
                            .values()
                            .flatten()
                            .map(|reply| reply["body"].as_str().map_or(0, str::len))
                            .sum::<usize>()
                            + text.len()
                            <= 4 * 1024 * 1024,
                    "Offline fixture reply capacity reached; restart the fixture to reset its synthetic conversations"
                );
                self.next_reply += 1;
                let root = format!("{id}_root");
                let mut reply = comment(
                    &format!("PRRC_native_{}", self.next_reply),
                    "offline-reviewer",
                    text,
                    Some(&root),
                    false,
                );
                self.replies
                    .entry(id.into())
                    .or_default()
                    .push(reply.clone());
                let mut pr = pull(moved);
                pr["repository"] = repository();
                reply["pullRequest"] = pr;
                json!({"addPullRequestReviewThreadReply":{"comment":reply}})
            } else if query.starts_with("mutation GitTurtleThreadResolution") {
                let thread = self.thread(id, moved, None, false)?;
                let resolved = !query.contains("unresolveReviewThread");
                ensure!(
                    thread[if resolved {
                        "viewerCanResolve"
                    } else {
                        "viewerCanUnresolve"
                    }] == true,
                    "Offline fixture refuses this resolution change"
                );
                self.resolved.insert(id.into(), resolved);
                let key = if resolved {
                    "resolveReviewThread"
                } else {
                    "unresolveReviewThread"
                };
                json!({key:{"thread":self.thread(id,moved,None,false)?}})
            } else {
                bail!("Unsupported offline query; no network request was made");
            }
        };
        Ok(Response {
            status: 200,
            headers: BTreeMap::new(),
            body: serde_json::to_vec(&json!({"data":data}))?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Fixture {
        session: Session,
        moved: bool,
        writes: usize,
    }
    impl Transport for Fixture {
        fn request(&mut self, request: Request, control: &OperationControl) -> Result<Response> {
            if request
                .body
                .as_ref()
                .and_then(|body| body["query"].as_str())
                .is_some_and(|query| query.starts_with("mutation"))
            {
                self.writes += 1;
            }
            self.session.request(request, self.moved, control)
        }
    }
    fn captured() -> CapturedPull {
        review::fixture_pull(false)
            .capture(Repository::parse("gitturtle-fixture/native-review").unwrap())
    }
    #[test]
    fn native_fixture_conversation_pages_replies_and_resolution_are_authoritative() {
        let mut client = Client(Fixture::default());
        let control = OperationControl::default();
        let first = client.review_threads(&captured(), None, &control).unwrap();
        assert_eq!(first.total_count, 6);
        assert_eq!(first.items.len(), 3);
        assert!(first.items[1].is_outdated);
        assert!(first.items[2].context_incomplete);
        assert!(!first.items[2].reply_available());
        let target = first.items[0].target.clone();
        let body = "  Exact reply — 界面\nSecond line\n";
        client
            .execute(
                Action::Reply {
                    target: target.clone(),
                    body: body.into(),
                },
                &control,
            )
            .unwrap();
        let updated = client.thread_comments(&target, None, &control).unwrap();
        assert_eq!(updated.comments.last().unwrap().body, body);
        client
            .execute(
                Action::SetThreadResolved {
                    target: target.clone(),
                    resolved: true,
                },
                &control,
            )
            .unwrap();
        assert!(
            client
                .thread_comments(&target, None, &control)
                .unwrap()
                .is_resolved
        );
        client
            .execute(
                Action::SetThreadResolved {
                    target: target.clone(),
                    resolved: false,
                },
                &control,
            )
            .unwrap();
        assert!(
            !client
                .thread_comments(&target, None, &control)
                .unwrap()
                .is_resolved
        );
        assert_eq!(client.0.writes, 3);
        let second = client
            .review_threads(&captured(), first.next_cursor.as_deref(), &control)
            .unwrap();
        assert!(second.next_cursor.is_none());
        assert!(second.items[0].is_resolved);
        let long = &second.items[1];
        assert_eq!(long.comments.len(), 20);
        let remaining = client
            .thread_comments(&long.target, long.next_comments_cursor.as_deref(), &control)
            .unwrap();
        assert_eq!(remaining.comments.len(), 3);
        assert_eq!(remaining.target, long.target);
        assert!(remaining.next_comments_cursor.is_none());
        assert!(
            client
                .execute(
                    Action::Reply {
                        target: second.items[2].target.clone(),
                        body: "refused".into()
                    },
                    &control
                )
                .is_err()
        );
        assert_eq!(client.0.writes, 3);
    }
    #[test]
    fn native_fixture_refuses_stale_context_other_destinations_and_cancelled_writes() {
        let mut client = Client(Fixture::default());
        let control = OperationControl::default();
        let target = client
            .review_threads(&captured(), None, &control)
            .unwrap()
            .items
            .remove(0)
            .target;
        client.0.moved = true;
        assert!(
            client
                .execute(
                    Action::Reply {
                        target: target.clone(),
                        body: "retained".into()
                    },
                    &control
                )
                .is_err()
        );
        let mut other = captured();
        other.repository = Repository::parse("other/repository").unwrap();
        assert!(client.review_threads(&other, None, &control).is_err());
        control.cancel();
        assert!(
            client
                .execute(
                    Action::SetThreadResolved {
                        target,
                        resolved: true
                    },
                    &control
                )
                .is_err()
        );
        assert_eq!(client.0.writes, 0);
    }
}
