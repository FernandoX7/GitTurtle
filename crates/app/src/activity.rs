//! Bounded app activity, separate from Git's reflog and external-tool history.
use crate::*;
use gpui_kit::component::WindowExt;
use gpui_kit::prelude::FluentBuilder;
use serde::{Deserialize, Serialize};
use std::{
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

const LIMIT: usize = 200;
const BYTE_LIMIT: u64 = 2 * 1024 * 1024;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
enum Outcome {
    Running,
    Success,
    Failure,
    Cancelled,
    Uncertain,
}
impl Outcome {
    fn label(&self) -> &'static str {
        match self {
            Self::Running => "In progress",
            Self::Success => "Succeeded",
            Self::Failure => "Failed",
            Self::Cancelled => "Cancelled · inspect state",
            Self::Uncertain => "Outcome uncertain",
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Entry {
    id: u64,
    time: i64,
    repository: String,
    operation: String,
    target: String,
    outcome: Outcome,
    explanation: String,
}
#[derive(Default)]
pub(super) struct State {
    entries: Vec<Entry>,
    pub error: Option<String>,
    /// Saves of the list, which share one job on the preference writer.
    saver: commit_drafts::CoalescingSaver<(), Vec<Entry>>,
    /// The wait before asking again after a full queue refused the job.
    retry: Option<Task<()>>,
}

fn safe(value: &str, limit: usize) -> String {
    gitturtle_core::redact_diagnostic(value)
        .chars()
        .take(limit)
        .collect()
}
fn retained_explanation(
    success: bool,
    cancelled: bool,
    uncertain: bool,
    diagnostic: &str,
) -> &'static str {
    if success {
        return "Git reported that the requested operation completed.";
    }
    if cancelled {
        return "Cancellation was requested. Some changes may have completed; inspect current state before another action.";
    }
    if uncertain {
        return "The operation ended without a confirmed result. Inspect local state; verifying a remote outcome requires an explicit network action.";
    }
    let diagnostic = diagnostic.to_ascii_lowercase();
    if diagnostic.contains("authenticat")
        || diagnostic.contains("credential")
        || diagnostic.contains("permission denied (publickey)")
    {
        "Authentication failed or was unavailable. Check the configured credential helper or SSH agent before another attempt."
    } else if diagnostic.contains("signing") || diagnostic.contains("gpg") {
        "Configured signing did not complete. Check the signing tool and key; GitTurtle did not fall back to an unsigned operation."
    } else if diagnostic.contains("conflict") {
        "Git reported conflicts or unresolved paths. Inspect Working Changes and the current operation before continuing."
    } else if diagnostic.contains("changed after")
        || diagnostic.contains("review again")
        || diagnostic.contains("stale")
        || diagnostic.contains("changed since")
    {
        "The reviewed target changed. Refresh and review the operation again."
    } else if diagnostic.contains("missing object")
        || diagnostic.contains("object is unavailable")
        || diagnostic.contains("object failed")
        || diagnostic.contains("verification")
    {
        "A required object was missing or failed verification. Inspect the selected object and source."
    } else if diagnostic.contains("permission denied") || diagnostic.contains("read-only file") {
        "Filesystem permissions prevented the operation. Inspect the destination and current repository state."
    } else {
        "Git reported a failure. Inspect current state and the repository's error details before retrying; GitTurtle does not retry writes automatically."
    }
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}
impl State {
    /// Called before constructing GPUI, alongside the preference store.
    pub(super) fn load() -> Self {
        match Self::read() {
            Ok(entries) => Self {
                entries,
                ..Self::default()
            },
            Err(error) => Self {
                error: Some(format!("Activity history unavailable: {error:#}")),
                ..Self::default()
            },
        }
    }
    fn read() -> anyhow::Result<Vec<Entry>> {
        let path = preferences::settings_path()?.with_file_name("activity.json");
        let bytes = match preferences::read_store(&path, BYTE_LIMIT) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
            Err(e) => return Err(e.into()),
        };
        let mut entries: Vec<Entry> = serde_json::from_slice(&bytes)?;
        if entries.len() > LIMIT {
            entries.drain(..entries.len() - LIMIT);
        }
        for entry in &mut entries {
            entry.repository = safe(&entry.repository, 4096);
            entry.operation = safe(&entry.operation, 160);
            entry.target = safe(&entry.target, 2048);
            entry.explanation = retained_explanation(
                entry.outcome == Outcome::Success,
                entry.outcome == Outcome::Cancelled,
                entry.outcome == Outcome::Uncertain,
                &entry.explanation,
            )
            .into();
            if entry.outcome == Outcome::Running {
                entry.outcome = Outcome::Uncertain;
                entry.explanation="The application ended before this operation's result was recorded. Inspect current local state; any remote verification needs an explicit network action.".into();
            }
        }
        Ok(entries)
    }
    pub(super) fn begin(&mut self, repository: &Path, operation: &str, target: &str) -> u64 {
        let id = now().max(
            self.entries
                .last()
                .map_or(0, |entry| entry.id.saturating_add(1)),
        );
        self.entries.push(Entry {
            id,
            time: (id / 1000) as i64,
            repository: safe(&repository.display().to_string(), 4096),
            operation: safe(operation.trim_end_matches('…'), 160),
            target: safe(target, 2048),
            outcome: Outcome::Running,
            explanation: "Submitted once to GitTurtle's serialized operation executor.".into(),
        });
        if self.entries.len() > LIMIT {
            self.entries.remove(0);
        }
        id
    }
    pub(super) fn finish(
        &mut self,
        id: u64,
        success: bool,
        cancelled: bool,
        uncertain: bool,
        message: &str,
    ) {
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.id == id) {
            entry.outcome = if success {
                Outcome::Success
            } else if cancelled {
                Outcome::Cancelled
            } else if uncertain {
                Outcome::Uncertain
            } else {
                Outcome::Failure
            };
            // Arbitrary hook/server diagnostics can echo commit content or
            // secrets not recognizable by a generic redactor. Persist only a
            // controlled category; detailed diagnostics stay in the live UI.
            entry.explanation = retained_explanation(success, cancelled, uncertain, message).into();
        }
    }
}

pub(super) fn target(command: &gitturtle_core::WriteCommand, fallback: &str) -> String {
    use gitturtle_core::*;
    match command {
        WriteCommand::DownloadLfs(plan) => format!(
            "{} · sha256:{} · {} from remote {}",
            plan.target.path.display(),
            plan.oid,
            crate::counted(plan.size, "byte", "bytes"),
            plan.remote
        ),
        WriteCommand::Stage { paths } | WriteCommand::Unstage { paths } => format!(
            "{} path arguments: {}",
            paths.len(),
            paths
                .iter()
                .take(12)
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ),
        WriteCommand::StageAll => "All unstaged working changes".into(),
        WriteCommand::Discard(plan) => plan
            .paths()
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(" ← "),
        WriteCommand::UnstageAll => "All staged changes".into(),
        WriteCommand::ApplyPartial { diff, .. } => {
            format!("{} · exact reviewed partial diff", diff.path.display())
        }
        WriteCommand::Checkout { branch } | WriteCommand::CreateBranch { name: branch, .. } => {
            branch.clone()
        }
        WriteCommand::Fetch { remote } => remote.clone(),
        WriteCommand::Pull { remote, branch } => format!("{remote}/{branch}"),
        WriteCommand::Push {
            remote,
            local_branch,
            remote_branch,
        } => format!("{local_branch} → {remote}/{remote_branch}"),
        WriteCommand::PublishRewrite(plan) => format!(
            "{} → {}:{} · expected {}",
            plan.new_oid, plan.remote, plan.remote_ref, plan.expected_remote_oid
        ),
        WriteCommand::Worktree(command) => match command.as_ref() {
            WorktreeCommand::Create(plan) => {
                format!("{} · {}", plan.branch, plan.destination.display())
            }
            WorktreeCommand::Remove(plan) | WorktreeCommand::ForceRemove(plan) => {
                plan.tree.path.display().to_string()
            }
        },
        WriteCommand::RecoverReflog(plan) => format!("{} at {}", plan.branch, plan.commit.oid),
        WriteCommand::InteractiveRebase(command) => match command.as_ref() {
            InteractiveRebaseCommand::Start { plan, .. } => {
                format!("{} · {} through {}", plan.branch, plan.base, plan.head)
            }
            InteractiveRebaseCommand::Continue { expected, .. } => format!(
                "{} · {}",
                expected.operation.branch, expected.operation.target_label
            ),
        },
        WriteCommand::Tag(command) => match command.as_ref() {
            TagCommand::Create(plan) => format!("{} at {}", plan.name, plan.target_oid),
            TagCommand::Delete(tag) => format!("{} · {}", tag.name, tag.oid),
            TagCommand::Push { tag, remote } => format!("{} → {}", tag.name, remote.name),
        },
        WriteCommand::Integration(command) => match command {
            IntegrationCommand::Resolve { expected, .. } => expected.path.display().to_string(),
            IntegrationCommand::Continue { expected }
            | IntegrationCommand::Abort { expected }
            | IntegrationCommand::Quit { expected } => {
                format!("{} · {}", expected.branch, expected.target_label)
            }
            IntegrationCommand::Merge { plan } | IntegrationCommand::Rebase { plan } => {
                format!("{} · {}", plan.branch, plan.target_label)
            }
        },
        WriteCommand::Recovery(command) => match command.as_ref() {
            RecoveryCommand::Amend { plan, .. }
            | RecoveryCommand::Undo { plan }
            | RecoveryCommand::Revert { plan }
            | RecoveryCommand::CherryPick { plan } => plan.target.oid.clone(),
            RecoveryCommand::DropStash { stash } => stash.oid.clone(),
            _ => fallback.into(),
        },
        // Commit messages, identity fields, URLs, credentials and manual result
        // content are deliberately excluded from retained activity.
        _ => fallback.into(),
    }
}

impl GitTurtle {
    /// Saves the whole list, each write asking at its start and its end. The
    /// person asked for none of these, so a writer that is merely behind is
    /// not a failure: they share one job, and one that a full queue refuses
    /// asks again until there is room, where a refusal once left "Could not
    /// save activity" in the Activity browser for the window's life and the
    /// newest list unsaved until the next write.
    pub(super) fn save_activity(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .activity
            .error
            .as_ref()
            .is_some_and(|error| error.starts_with("Activity history unavailable:"))
        {
            return;
        }
        let entries = self.activity.entries.clone();
        let response =
            self.activity
                .saver
                .queue_with(&self.preferences_writer, (), entries, |entries| {
                    let Some(entries) = entries.get(&()) else {
                        return Ok(());
                    };
                    let path = preferences::settings_path()?.with_file_name("activity.json");
                    let bytes = serde_json::to_vec(entries)?;
                    anyhow::ensure!(
                        bytes.len() as u64 <= BYTE_LIMIT,
                        "Activity history exceeds its byte limit"
                    );
                    preferences::atomic_write(&path, &bytes)
                });
        let Some(response) = response else {
            return;
        };
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Err(error)) = response.await {
                let _ = this.update_in(cx, |this, window, cx| {
                    if error.is::<operations::Busy>() {
                        this.ask_again_to_save_activity(window, cx);
                        return;
                    }
                    this.activity.error = Some(format!("Could not save activity: {error:#}"));
                    cx.notify();
                });
            }
        })
        .detach();
    }
    /// Asks again, after the wait, to save the list a full queue refused.
    fn ask_again_to_save_activity(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.activity.retry.is_some() {
            return;
        }
        let retry = self.retry_refused_save(window, cx, |this, window, cx| {
            this.activity.retry = None;
            this.save_activity(window, cx);
        });
        self.activity.retry = Some(retry);
    }
    pub(super) fn open_activity(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let owner = cx.entity();
        let browser = cx.new(|cx| ActivityBrowser {
            owner: owner.downgrade(),
            _observer: cx.observe(&owner, |_, _, cx| cx.notify()),
            body: crate::focus_reveal::FocusReveal::new(crate::focus_reveal::Trigger::Keyboard),
        });
        window.open_alert_dialog(cx, move |dialog, window, cx| {
            let shown = browser.clone();
            // In the dialog's content, as the profile form is: the body
            // takes what the cap leaves it and scrolls the rest, revealing
            // the action Tab moves onto, with the ring's room below it,
            // which the footer's gap gives back, rather than rely on the
            // dialog's own scrolling body, which reveals nothing.
            dialog
                .title("GitTurtle activity")
                .width(appearance::dialog_width(px(760.), window))
                .max_h(appearance::dialog_max_height(window))
                .gap(crate::tags::DIALOG_FOOTER_GAP - appearance::button_ring_room(cx))
                .content(move |content, _, _| content.min_h_0().child(shown.clone()))
                .footer(crate::tags::dialog_footer("Done", None))
        });
    }
}
struct ActivityBrowser {
    owner: WeakEntity<GitTurtle>,
    _observer: Subscription,
    /// The body's scroll, which reveals the action Tab moves onto: below
    /// its fold in a half tile at 18 pt.
    body: crate::focus_reveal::FocusReveal,
}
impl Render for ActivityBrowser {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let body_height = (window.viewport_size().height - px(240.)).max(px(120.));
        let list_height = appearance::ui_size(430.).min((body_height - px(160.)).max(px(100.)));
        let Some(owner) = self.owner.upgrade() else {
            return div().into_any_element();
        };
        let app = owner.read(cx);
        let progress = app.operation_progress();
        let entries = app.activity.entries.clone();
        let error = app.activity.error.clone();
        // Its actions, ghost Buttons at the dialog's text edge, stand the
        // label of each line's first on that edge, their frames their pull
        // before it (DESIGN.md, "One left edge"); the body keeps the pull
        // with the ring's room on its left, and the ring's room on its
        // right, given back through its margins, so its clip cuts neither.
        // It keeps the ring's room above and below them too, and a wrapper
        // keeps the room below inside the dialog's clip.
        let room = appearance::button_ring_room(cx);
        let lead = crate::ghost_pull(window) + room;
        // An action Tab moves onto scrolls into view with its ring's room
        // (`focus_reveal`).
        self.body.reveal(window, cx);
        let [refresh, reflog] =
            ["activity-refresh", "activity-reflog"].map(|id| self.body.focus(id, cx));
        let cue = crate::tags::ScrollCue::revealing(&self.body, room);
        div().flex().flex_col().min_h_0().pb(room).child(cue.list(div().on_children_prepainted(cue.observe(self.body.observe())).id("activity-browser-content").max_h(body_height + room * 2.).min_h_0().overflow_y_scroll().track_scroll(self.body.scroll()).p(room).m(-room).pl(lead).ml(-lead).flex().flex_col().gap_2().text_size(appearance::ui_text(12.))
            .child(div().text_color(rgb(p.muted)).child("The latest 200 operations submitted by GitTurtle, across repositories. This is not a complete record of work performed by Git or other tools. Reflog recovery is available separately."))
            .child(crate::edge_row(window).on_children_prepainted(self.body.controls(vec![Some(refresh.clone()), Some(reflog.clone())], room))
                .child(crate::workspace::refusing(crate::edge_button("activity-refresh","Refresh current repository").track_focus(&refresh).debug_selector(||"activity-refresh".into()),||"activity-refresh".into(),app.operation_busy.is_some(),app.operation_shown().is_some()).on_click({let owner=self.owner.clone();move |_,window,cx|{let _=owner.update(cx,|app,cx|{
                    // Refused at once while a write runs, before it looks so.
                    if app.operation_busy.is_some() { return; }
                    window.close_dialog(cx);app.refresh_worktree(window,cx);});}}))
                .child(crate::workspace::refusing(crate::edge_button("activity-reflog","Browse reflog…").track_focus(&reflog).debug_selector(||"activity-reflog".into()),||"activity-reflog".into(),app.operation_busy.is_some()||app.repository.is_none(),app.operation_shown().is_some()||app.repository.is_none()).on_click({let owner=self.owner.clone();move |_,window,cx|{let _=owner.update(cx,|app,cx|{
                    // Refused at once while a write runs, before it looks so,
                    // and Activity stays open: `open_reflog_browser` refuses
                    // then, and closing first left neither dialog.
                    if app.operation_busy.is_some() { return; }
                    window.close_dialog(cx);app.open_reflog_browser(window,cx);});}})))
            .children(error.map(|error|div().text_color(rgb(p.warning)).child(error)))
            .child(crate::native_accessibility::clips_children(div().id("activity-entries").h(list_height).flex_shrink_0().overflow_y_scroll().flex().flex_col().gap_2()
                .children(entries.into_iter().rev().map(|entry|{
                    let timestamp=chrono::DateTime::from_timestamp(entry.time,0).map(|t|t.with_timezone(&chrono::Local).format("%b %d %H:%M:%S").to_string()).unwrap_or_default();
                    let heading=format!("{} · {} · {}",entry.operation,entry.outcome.label(),timestamp);
                    let description = format!("{heading}. Repository: {}. Target: {}. {}", entry.repository, entry.target, if entry.outcome == Outcome::Running { progress.as_deref().unwrap_or(&entry.explanation) } else { &entry.explanation });
                    div().id(("activity-entry",entry.id)).role(Role::Group).aria_label(description).p_3().border_1().border_color(rgb(p.border)).rounded(px(6.)).flex().flex_col().gap_1()
                        .child(div().font_weight(FontWeight::SEMIBOLD).child(heading))
                        .child(div().child(format!("{} · {}",entry.repository,entry.target)))
                        .child(div().text_color(rgb(p.muted)).child(if entry.outcome==Outcome::Running{progress.clone().unwrap_or(entry.explanation)}else{entry.explanation}))
                }))
                .when(app.activity.entries.is_empty(),|el|el.child(div().p_3().child("No GitTurtle operations recorded yet."))))), "activity-scrollbar").min_h_0()).into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{LIMIT, Outcome, State, target};
    use crate::{GitTurtle, workspace::REFUSED_SAVE_RETRY};
    use gpui_kit::{Entity, VisualTestContext, gpui};
    use std::path::Path;

    /// Records an operation in the list and asks for the list to be saved.
    fn recorded(app: &Entity<GitTurtle>, operation: &str, cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.activity.begin(Path::new("/fixture"), operation, "main");
                app.save_activity(window, cx);
            })
        });
        cx.executor().run_until_parked();
    }

    fn writer_answers(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
        app.read_with(cx, |app, _| app.preferences_writer.drain());
        cx.executor().run_until_parked();
    }

    /// How many entries of the stored list record `operation`.
    fn stored(operation: &str) -> usize {
        State::read()
            .unwrap()
            .iter()
            .filter(|entry| entry.operation == operation)
            .count()
    }

    /// Each write asks twice for the list to be saved, and the person asked
    /// for neither, so a preference writer that is merely behind is not a
    /// failure: a queue other saves have filled refuses the job in silence
    /// and it asks again until the newest list is stored, and any number of
    /// saves behind a stalled writer keep one slot of the queue. A refusal
    /// once left "Could not save activity" in the Activity browser for the
    /// window's life.
    #[gpui::test]
    async fn a_writer_that_is_behind_never_fails_an_activity_save(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        let (_fixture, app, cx) = crate::views::tests::history_window(cx).await;
        writer_answers(&app, cx);
        let error = |app: &Entity<GitTurtle>, cx: &mut VisualTestContext| {
            app.read_with(cx, |app, _| app.activity.error.clone())
        };

        let held = app.read_with(cx, |app, _| app.preferences_writer.hold_reached());
        let fillers: Vec<_> = (0..8)
            .map(|_| app.read_with(cx, |app, _| app.preferences_writer.submit(|| Ok(()))))
            .collect();
        for mut filler in fillers {
            assert!(matches!(filler.try_recv(), Ok(None)), "the queue fills");
        }
        recorded(&app, "Refused", cx);
        assert_eq!(error(&app, cx), None);
        // Still full: refused again, and still nothing to report.
        cx.executor().advance_clock(REFUSED_SAVE_RETRY);
        cx.executor().run_until_parked();
        assert_eq!(error(&app, cx), None);

        drop(held);
        writer_answers(&app, cx);
        assert_eq!(stored("Refused"), 0, "only asking again brings it");
        cx.executor().advance_clock(REFUSED_SAVE_RETRY);
        cx.executor().run_until_parked();
        writer_answers(&app, cx);
        assert_eq!(stored("Refused"), 1, "the newest list reaches the store");
        assert_eq!(error(&app, cx), None);

        let held = app.read_with(cx, |app, _| app.preferences_writer.hold_reached());
        for _ in 0..9 {
            recorded(&app, "Behind", cx);
        }
        let others: Vec<_> = (0..7)
            .map(|_| app.read_with(cx, |app, _| app.preferences_writer.submit(|| Ok(()))))
            .collect();
        for mut other in others {
            assert!(
                matches!(other.try_recv(), Ok(None)),
                "the saves keep one slot of the queue"
            );
        }
        drop(held);
        writer_answers(&app, cx);
        assert_eq!(stored("Behind"), 9, "the newest list reaches the store");
        assert_eq!(error(&app, cx), None);
    }

    #[test]
    fn activity_is_bounded_and_does_not_retain_commit_messages() {
        let mut state = State::default();
        for i in 0..250 {
            let id = state.begin(Path::new("/fixture"), "Commit", &format!("branch-{i}"));
            state.finish(id, true, false, false, "Completed");
        }
        assert_eq!(state.entries.len(), LIMIT);
        let command = gitturtle_core::WriteCommand::Commit {
            message: "secret message never retained".into(),
        };
        assert_eq!(target(&command, "main"), "main");
    }
    #[test]
    fn activity_distinguishes_cancellation_failure_and_lost_result() {
        let mut s = State::default();
        let a = s.begin(Path::new("/fixture"), "Fetch", "origin");
        s.finish(a, false, true, true, "Cancelled");
        assert_eq!(s.entries[0].outcome, Outcome::Cancelled);
        let b = s.begin(Path::new("/fixture"), "Stage", "one");
        s.finish(b, false, false, false, "Refused");
        assert_eq!(s.entries[1].outcome, Outcome::Failure);
        let c = s.begin(Path::new("/fixture"), "Push", "origin/main");
        s.finish(c, false, false, true, "No result");
        assert_eq!(s.entries[2].outcome, Outcome::Uncertain);
    }
    #[test]
    fn arbitrary_hook_and_server_output_is_not_retained() {
        let mut state = State::default();
        let id = state.begin(Path::new("/fixture"), "Commit", "main");
        state.finish(
            id,
            false,
            false,
            false,
            "hook output: unrecognizable-private-value and a confidential commit body",
        );
        let saved = serde_json::to_string(&state.entries).unwrap();
        assert!(!saved.contains("unrecognizable-private-value"));
        assert!(!saved.contains("confidential commit body"));
        assert!(saved.contains("Git reported a failure"));
    }
}
