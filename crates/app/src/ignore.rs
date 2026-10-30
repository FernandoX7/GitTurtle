//! Contextual ignore choices followed by an exact rule and destination review.
use crate::*;
use gitturtle_core::{IgnoreDestination, StatusEntry, WriteCommand};
use gpui_kit::{
    component::{WindowExt, checkbox::Checkbox, dialog::DialogButtonProps},
    prelude::FluentBuilder,
};

const PREPARING: &str = "Preparing ignore rule…";
/// Why Ignore… opens no form for a file that is now tracked, in the core's
/// words for the same refusal from Preview rule (`ignore_plan`).
const NO_LONGER_UNTRACKED: &str =
    "This file is no longer untracked. Refresh Changes; ignore rules never untrack files.";
/// Why Ignore… opens no form for a file that has left the working status,
/// in the words of Discard…'s review (`discard_plan`).
const NO_LONGER_LISTED: &str =
    "This file no longer appears in Changes. Refresh and review it again.";

use crate::native_accessibility::text as static_text;

#[derive(Default)]
pub(super) struct State {
    generation: u64,
    task: Option<Task<()>>,
}

/// Reads `path`'s status again, as Discard…'s review reads its row's
/// (`GitRepository::discard_plan`): the row Changes drew is the status read
/// last, which a change outside the app can have overtaken. Passive, on the
/// operation executor. Refuses, in the words a refusal is said in, a path
/// that is no longer listed or no longer untracked.
fn still_untracked(repo: &GitRepository, path: &std::path::Path) -> anyhow::Result<()> {
    let status = repo.status()?;
    let entry = status
        .entries
        .iter()
        .find(|entry| entry.path == path)
        .ok_or_else(|| anyhow::anyhow!(NO_LONGER_LISTED))?;
    anyhow::ensure!(entry.untracked, NO_LONGER_UNTRACKED);
    Ok(())
}

impl GitTurtle {
    pub(super) fn cancel_ignore_action(&mut self) {
        self.ignore_actions.generation = self.ignore_actions.generation.wrapping_add(1);
        self.ignore_actions.task = None;
        if self.operation_busy == Some(PREPARING) {
            self.operation_busy = None;
        }
    }

    /// Reviews the untracked file `entry` names again before its form opens,
    /// as Discard… reviews its row before its confirmation: `entry` is the
    /// row Changes drew from the status it read last, and a file staged or
    /// removed outside the app since then is no longer one an ignore rule
    /// is for. The form then opens, or, refused, it does not, and the
    /// window's error says why, as it says Discard…'s refusal, a fresh
    /// alert each time it is asked (`error_refusals`). The form once opened
    /// for whatever the row last said; Preview rule and the write still
    /// refuse a file that becomes tracked while the form is open
    /// (`ignore_plan`, `execute_ignore`).
    ///
    /// The review is this action's preparation: it holds the operation
    /// executor and the window's busy state as Preview rule's does, and
    /// leaving the page cancels it (`cancel_ignore_action`).
    pub(super) fn open_ignore(
        &mut self,
        entry: StatusEntry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.operation_busy.is_some() || self.page != AppPage::Repository || !entry.untracked {
            return;
        }
        let Some(repo) = self.repository.clone() else {
            return;
        };
        let repository = repo.path().to_owned();
        self.cancel_ignore_action();
        let generation = self.ignore_actions.generation;
        self.operation_busy = Some(PREPARING);
        self.operation_error = None;
        let path = entry.path;
        let checked = path.clone();
        let response = self
            .operations
            .submit_read(move || still_untracked(&repo, &checked));
        self.ignore_actions.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = response.await.unwrap_or_else(|_| {
                Err(anyhow::anyhow!(
                    "Ignore preparation was interrupted. Choose Ignore… again."
                ))
            });
            let _ = this.update_in(cx, |this, window, cx| {
                if this.ignore_actions.generation != generation {
                    return;
                }
                this.ignore_actions.task = None;
                if this.operation_busy == Some(PREPARING) {
                    this.operation_busy = None;
                }
                this.read_deferred_working(window, cx);
                if this.path.as_ref() != Some(&repository) || this.page != AppPage::Repository {
                    cx.notify();
                    return;
                }
                match result {
                    Ok(()) => this.show_ignore_form(path, window, cx),
                    Err(error) => {
                        this.operation_error = Some(format!("{error:#}"));
                        // A fresh node, heard again however often it refuses.
                        this.error_refusals = this.error_refusals.wrapping_add(1);
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// Opens the Ignore form for the untracked file `path`, reviewed again
    /// just now ([`Self::open_ignore`]).
    fn show_ignore_form(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let owner = cx.entity().downgrade();
        let repository = self.path.clone();
        let form = cx.new(|_| IgnoreForm {
            owner,
            repository,
            path,
            directory: false,
            local: false,
            preparing: None,
            error: None,
        });
        window.open_alert_dialog(cx, move |dialog, window, _| {
            let submit = form.clone();
            let cancel = form.clone();
            dialog
                .title(native_accessibility::dialog_title(
                    "ignore-dialog-title",
                    "Ignore untracked content",
                ))
                .width(appearance::dialog_width(px(560.), window))
                .max_h(appearance::dialog_max_height(window))
                .child(form.clone())
                .button_props(
                    DialogButtonProps::default()
                        .ok_text("Preview rule")
                        .cancel_text("Cancel")
                        .show_cancel(true),
                )
                .on_ok(move |_, window, cx| {
                    submit.update(cx, |form, cx| form.submit(window, cx));
                    false
                })
                .on_cancel(move |_, window, cx| {
                    cancel.update(cx, |form, cx| {
                        form.preparing = None;
                        let _ = form.owner.update(cx, |this, cx| {
                            this.cancel_ignore_action();
                            // The cancelled preparation's end never runs.
                            this.read_deferred_working(window, cx);
                        });
                    });
                    true
                })
        });
    }
}

struct IgnoreForm {
    owner: WeakEntity<GitTurtle>,
    repository: Option<PathBuf>,
    path: PathBuf,
    directory: bool,
    local: bool,
    /// The generation of the window's ignore read this form started
    /// (`IgnoreForm::pending`).
    preparing: Option<u64>,
    error: Option<String>,
}
impl IgnoreForm {
    /// Whether the read this form started still runs: the window's ignore
    /// read of its generation, which ends when it lands or is cancelled, as
    /// a page switch cancels it (`GitTurtle::end_preparations`), as
    /// Create tag's form reads its own (`TagForm::pending`). A flag only the
    /// read's reply cleared stayed set once Settings cancelled the read, so
    /// Preview rule refused every later press. A form whose window has gone
    /// keeps what it started.
    fn pending(&self, cx: &App) -> bool {
        self.preparing.is_some_and(|generation| {
            self.owner.upgrade().is_none_or(|owner| {
                let owner = owner.read(cx);
                owner.ignore_actions.generation == generation
                    && owner.operation_busy == Some(PREPARING)
            })
        })
    }
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending(cx) {
            return;
        }
        let path = self.path.clone();
        let directory = self.directory;
        let destination = if self.local {
            IgnoreDestination::Local
        } else {
            IgnoreDestination::Shared
        };
        let form = cx.entity().downgrade();
        let accepted = self.owner.update(cx, |this, cx| {
            if this.path != self.repository || this.operation_busy.is_some() || this.page != AppPage::Repository { return None; }
            let repo = this.repository.clone()?;
            let repository = repo.path().to_owned(); this.cancel_ignore_action();
            let generation = this.ignore_actions.generation;
            this.operation_busy = Some(PREPARING); this.operation_error = None;
            let response = this.operations.submit_read(move || repo.ignore_plan(&path, directory, destination));
            this.ignore_actions.task = Some(cx.spawn_in(window, async move |this, cx| {
                let result = response.await.unwrap_or_else(|_| Err(anyhow::anyhow!("Ignore preparation was interrupted; review the rule again.")));
                let _ = this.update_in(cx, |this, window, cx| {
                    if this.ignore_actions.generation != generation { return; }
                    if this.operation_busy == Some(PREPARING) { this.operation_busy = None; }
                    this.read_deferred_working(window, cx);
                    if this.path.as_ref() != Some(&repository) || this.page != AppPage::Repository { cx.notify(); return; }
                    let _ = form.update(cx, |form, cx| { form.preparing = None; cx.notify(); });
                    match result {
                        Ok(plan) => {
                            window.close_dialog(cx);
                            let scope = match plan.destination { IgnoreDestination::Shared => "Shared .gitignore: this rule can be committed and shared with collaborators. The file is not staged automatically.", IgnoreDestination::Local => "Repository-local excludes: this rule remains in Git metadata and is not committed. Linked worktrees share this excludes file." };
                            let explanation = format!("{}\n\nDestination:\n{}\n\nExact rule{}:\n{}\n\n{}{}\n\nExisting content and line endings are preserved. No files are untracked, deleted, or staged. If the destination changes, this action requires a fresh preview.", scope, plan.destination_path.display(), if plan.rule_is_utf8() { "" } else { " (non-UTF-8 bytes shown as \\xHH)" }, plan.rule_display(), if plan.directory { "This directory rule applies to its untracked descendants. " } else { "This rule names only this repository-relative path. " }, if plan.tracked_paths > 0 { format!("{} tracked paths remain tracked and continue to appear when modified.", plan.tracked_paths) } else { "Tracked files remain tracked even when an ignore rule matches them.".into() });
                            this.confirm_git_write("Apply ignore rule".into(), explanation, "Add ignore rule", WriteCommand::Ignore(Arc::new(plan)), window, cx);
                        }
                        Err(error) => { let _ = form.update(cx, |form, cx| { form.error = Some(format!("{error:#}")); cx.notify(); }); }
                    }
                    cx.notify();
                });
            })); cx.notify(); Some(generation)
        }).ok().flatten();
        self.preparing = accepted;
        if accepted.is_none() {
            self.error = Some("The repository changed or another operation is running. Reopen Ignore after it finishes.".into());
        }
        cx.notify();
    }
}
impl Render for IgnoreForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let pending = self.pending(cx);
        let parent = self
            .path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty());
        div().flex().flex_col().gap_3()
            .child(static_text("ignore-selected-path", format!("Selected untracked file: {}", self.path.display())).text_size(crate::appearance::ui_text(12.)))
            .child(static_text("ignore-destination-label", "Save rule in").text_size(crate::appearance::ui_text(12.)).font_weight(FontWeight::MEDIUM))
            .child(div().flex().gap_2().children([(false, "Shared .gitignore"), (true, "Local excludes")].map(|(local, label)| button(label, label, "", self.local == local).toggled(self.local == local).disabled(pending).on_click(cx.listener(move |this, _, _, cx| { this.local = local; this.error = None; cx.notify(); })))))
            .child(static_text("ignore-destination-help", if self.local { "For this repository's local setup. Stored in Git's info/exclude, including linked worktrees." } else { "For patterns the project should share. Changes to .gitignore remain unstaged for review." }).text_size(crate::appearance::ui_text(11.)).text_color(rgb(p.muted)))
            .when_some(parent, |element, parent| element.child(Checkbox::new("ignore-containing-directory").label(format!("Ignore containing directory: {}/", parent.display())).checked(self.directory).disabled(pending).on_click(cx.listener(|this, checked, _, cx| { this.directory = *checked; this.error = None; cx.notify(); }))))
            .child(static_text("ignore-tracked-explanation", "The next step previews the exact escaped rule and destination. Ignore rules affect untracked content; they never remove tracked files from Git.").text_size(crate::appearance::ui_text(11.)).text_color(rgb(p.muted)))
            .when(pending, |element| element.child(static_text("ignore-preparation-status", "Preparing exact rule and checking current content…").text_size(crate::appearance::ui_text(12.))))
            .children(self.error.as_ref().map(|error| static_text("ignore-preparation-error", error.clone()).text_size(crate::appearance::ui_text(12.)).text_color(rgb(p.warning))))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use core::prelude::v1::test;
    use std::{cell::RefCell, rc::Rc};

    /// The ignore form between two tab stops the probe owns.
    struct Probe {
        before: FocusHandle,
        form: Entity<IgnoreForm>,
        after: FocusHandle,
    }

    impl Render for Probe {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .flex()
                .flex_col()
                .child(
                    div()
                        .id("probe-before")
                        .track_focus(&self.before.clone().tab_stop(true))
                        .size(px(20.)),
                )
                .child(self.form.clone())
                .child(
                    div()
                        .id("probe-after")
                        .track_focus(&self.after.clone().tab_stop(true))
                        .size(px(20.)),
                )
        }
    }

    /// The containing-directory Checkbox turns disabled while it holds focus
    /// once the rule's preparation starts, as `IgnoreForm::submit` sets
    /// `preparing`, with the destination buttons before it. Tab and Shift+Tab,
    /// as keystrokes through the window, still leave it for the nearest tab
    /// stops that stay enabled, and skip it while it is disabled.
    #[gpui::test]
    fn tab_and_shift_tab_leave_the_directory_checkbox_while_the_rule_is_prepared(
        cx: &mut TestAppContext,
    ) {
        cx.update(gpui_kit::init);
        let captured: Rc<RefCell<Option<Entity<Probe>>>> = Default::default();
        let output = captured.clone();
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let probe = cx.new(|cx| Probe {
                before: cx.focus_handle(),
                form: cx.new(|_| IgnoreForm {
                    owner: WeakEntity::new_invalid(),
                    repository: None,
                    path: PathBuf::from("build/output.log"),
                    directory: false,
                    local: false,
                    preparing: None,
                    error: None,
                }),
                after: cx.focus_handle(),
            });
            *output.borrow_mut() = Some(probe.clone());
            gpui_kit::component::Root::new(probe, window, cx)
        });
        let probe = captured.borrow().as_ref().unwrap().clone();
        let draw = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| window.draw(cx).clear(cx));
            cx.run_until_parked();
        };
        let focused = |cx: &mut VisualTestContext| cx.update(|window, cx| window.focused(cx));
        let press = |cx: &mut VisualTestContext, keys: &str| {
            cx.simulate_keystrokes(keys);
            draw(cx);
            focused(cx).expect("a focused control")
        };
        let (before, form, after) = cx.read(|cx| {
            let probe = probe.read(cx);
            (
                probe.before.clone(),
                probe.form.clone(),
                probe.after.clone(),
            )
        });
        let pending = |cx: &mut VisualTestContext, pending: bool| {
            cx.update(|_, cx| {
                form.update(cx, |form, cx| {
                    form.preparing = pending.then_some(0);
                    cx.notify();
                })
            });
            draw(cx);
        };
        let focus = |cx: &mut VisualTestContext, handle: &FocusHandle| {
            cx.update(|window, cx| window.focus(handle, cx));
            draw(cx);
        };

        // Enabled, the form reads: Shared .gitignore, Local excludes, Checkbox.
        draw(cx);
        focus(cx, &before);
        let shared = press(cx, "tab");
        let local = press(cx, "tab");
        let checkbox = press(cx, "tab");
        let stops = [&before, &shared, &local, &checkbox, &after];
        for (index, stop) in stops.iter().enumerate() {
            assert!(
                stops[index + 1..].iter().all(|other| other != stop),
                "five distinct tab stops"
            );
        }
        assert_eq!(
            press(cx, "tab"),
            after,
            "the Checkbox precedes the after stop"
        );
        assert_eq!(press(cx, "shift-tab"), checkbox);

        pending(cx, true);
        assert_eq!(
            focused(cx),
            Some(checkbox.clone()),
            "the disabled Checkbox keeps focus until the keyboard moves it"
        );
        assert_eq!(press(cx, "tab"), after, "Tab leaves the disabled Checkbox");
        assert_eq!(
            press(cx, "shift-tab"),
            before,
            "Shift+Tab passes over the disabled Checkbox and buttons"
        );

        pending(cx, false);
        focus(cx, &local);
        assert_eq!(press(cx, "tab"), checkbox);
        pending(cx, true);
        assert_eq!(
            press(cx, "shift-tab"),
            before,
            "Shift+Tab leaves the disabled Checkbox past the disabled buttons"
        );
    }

    /// `checked_out`-like Changes: a repository with an untracked
    /// `scratch.txt`, read, and its entry.
    pub(super) async fn untracked_scratch(
        cx: &mut TestAppContext,
    ) -> (
        tempfile::TempDir,
        Entity<GitTurtle>,
        &mut VisualTestContext,
        PathBuf,
        StatusEntry,
    ) {
        use crate::views::tests::{history_script, history_window_from, settle};
        let (fixture, app, cx) = history_window_from(cx, history_script(2, 3)).await;
        let root = app
            .read_with(cx, |app, _| app.path.clone())
            .expect("a repository");
        std::fs::write(root.join("scratch.txt"), "scratch\n").unwrap();
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_working(window, cx)));
        settle(&app, cx).await;
        crate::path_filter::filters_land(&app, cx);
        let scratch = app.read_with(cx, |app, _| {
            app.work_status
                .as_ref()
                .and_then(|status| {
                    status
                        .entries
                        .iter()
                        .find(|entry| entry.path == std::path::Path::new("scratch.txt"))
                        .cloned()
                })
                .expect("scratch.txt is listed")
        });
        assert!(scratch.untracked);
        (fixture, app, cx, root, scratch)
    }

    /// Asks the open Ignore form for Preview rule, as its OK does, or
    /// confirms the review it opened.
    fn preview_rule(cx: &mut VisualTestContext) {
        use gpui_kit::component::dialog::Confirm;
        cx.update(|window, cx| window.dispatch_action(Box::new(Confirm { secondary: false }), cx));
    }

    /// Lets what Ignore… asked of the operation executor answer, its review
    /// of the file before the form opens included, and draws the outcome.
    /// It runs on the executor's thread, which `run_until_parked` cannot
    /// wait for.
    pub(crate) fn ignore_lands(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
        app.read_with(cx, |app, _| app.operations.drain());
        crate::views::tests::draw(cx);
    }

    /// Preview rule asked for again once Settings has cancelled the rule's
    /// preparation (`GitTurtle::end_preparations`) previews the rule: the
    /// form reads its preparation from the window's, which the cancel
    /// ended (`IgnoreForm::pending`). A flag only the read's reply cleared
    /// kept the form saying it was preparing, and refusing every later
    /// Preview rule, as Create tag's form once did.
    #[gpui::test]
    async fn preview_rule_works_again_once_settings_cancels_it(cx: &mut TestAppContext) {
        use crate::views::tests::{draw, settle};
        let (_fixture, app, cx, _, scratch) = untracked_scratch(cx).await;
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_ignore(scratch, window, cx)));
        ignore_lands(&app, cx);
        // Preview rule, its read held as a slow disk holds it, and Settings.
        let held = app.update(cx, |app, _| app.operations.hold());
        preview_rule(cx);
        draw(cx);
        assert!(
            cx.debug_bounds("ignore-preparation-status").is_some(),
            "the rule is prepared"
        );
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_settings(window, cx)));
        drop(held);
        app.read_with(cx, |app, _| app.operations.drain());
        draw(cx);
        assert!(
            cx.update(|window, cx| window.has_active_dialog(cx)),
            "the form stays open"
        );
        assert!(
            cx.debug_bounds("ignore-preparation-status").is_none(),
            "Settings ended the form's preparation"
        );
        cx.update(|window, cx| app.update(cx, |app, cx| app.return_from_page(window, cx)));
        draw(cx);
        preview_rule(cx);
        app.read_with(cx, |app, _| app.operations.drain());
        settle(&app, cx).await;
        draw(cx);
        assert!(
            cx.debug_bounds("operation-consequences").is_some(),
            "Preview rule previews the rule"
        );
    }

    /// Ignore… reads the file's status again before its form opens, as
    /// Discard… reviews its row (`GitTurtle::open_ignore`): a file staged
    /// outside the app since Changes read it opens no form, and the
    /// window's error says why, in the words Preview rule refuses it in, an
    /// alert heard at once and again each time it is asked. The form trusted
    /// the row Changes had drawn, and opened for a file now tracked. Still
    /// untracked, the file opens the form; staged once the form is open, it
    /// is refused by Preview rule, beneath the form's fields, and staged
    /// once the rule is reviewed, by the write, which leaves `.gitignore` as
    /// it was.
    #[gpui::test]
    async fn ignore_reviews_the_file_again_before_its_form_opens(cx: &mut TestAppContext) {
        use crate::native_accessibility::written;
        use crate::views::tests::{fixture_git, settle};
        let (_fixture, app, cx, root, scratch) = untracked_scratch(cx).await;
        let dialog =
            |cx: &mut VisualTestContext| cx.update(|window, cx| window.has_active_dialog(cx));
        let open = |cx: &mut VisualTestContext| {
            let scratch = scratch.clone();
            cx.update(|window, cx| app.update(cx, |app, cx| app.open_ignore(scratch, window, cx)));
        };
        let ignore_file = root.join(".gitignore");
        let ignored_before = std::fs::read(&ignore_file).ok();

        // Staged outside the app: Changes still draws it untracked.
        fixture_git(&root, &["add", "scratch.txt"]);
        let mut nodes = Vec::new();
        for press in ["Ignore…", "Ignore… again"] {
            open(cx);
            assert_eq!(
                app.read_with(cx, |app, _| app.operation_busy),
                Some(PREPARING),
                "{press}: the file is reviewed again"
            );
            ignore_lands(&app, cx);
            assert!(!dialog(cx), "{press}: no form opens");
            assert_eq!(
                app.read_with(cx, |app, _| (
                    app.operation_busy,
                    app.operation_error.clone()
                )),
                (None, Some(NO_LONGER_UNTRACKED.to_owned())),
                "{press}: the window's error says why"
            );
            assert!(
                cx.debug_bounds("operation-error-summary").is_some(),
                "{press}: drawn"
            );
            let mut announced = written::announced_from("operation-error-summary-");
            assert_eq!(
                announced.len(),
                1,
                "{press}: one refusal announced: {announced:?}"
            );
            let (node, how) = announced.pop().expect("the refusal");
            assert_eq!(how, written::Announced::ASSERTIVE, "{press}: at once");
            nodes.push(node);
        }
        assert_ne!(
            nodes[0], nodes[1],
            "the refusal asked for again is a fresh node, heard again"
        );

        // Untracked again: the form opens.
        fixture_git(&root, &["reset", "-q", "scratch.txt"]);
        app.update(cx, |app, _| app.operation_error = None);
        open(cx);
        ignore_lands(&app, cx);
        assert!(dialog(cx), "the form opens for an untracked file");
        assert_eq!(
            written::get("ignore-selected-path")
                .and_then(|text| text.node.value().map(str::to_owned))
                .as_deref(),
            Some("Selected untracked file: scratch.txt")
        );

        // Staged once the form is open: Preview rule refuses it there.
        fixture_git(&root, &["add", "scratch.txt"]);
        preview_rule(cx);
        ignore_lands(&app, cx);
        assert_eq!(
            written::get("ignore-preparation-error")
                .and_then(|text| text.node.value().map(str::to_owned))
                .as_deref(),
            Some(NO_LONGER_UNTRACKED),
            "Preview rule says why beneath the fields"
        );
        assert!(cx.debug_bounds("operation-consequences").is_none());

        // Untracked again, the rule is reviewed, and staged before the
        // write: the write refuses it and leaves the ignore file alone.
        fixture_git(&root, &["reset", "-q", "scratch.txt"]);
        preview_rule(cx);
        ignore_lands(&app, cx);
        assert!(
            cx.debug_bounds("operation-consequences").is_some(),
            "the rule is reviewed"
        );
        fixture_git(&root, &["add", "scratch.txt"]);
        preview_rule(cx);
        ignore_lands(&app, cx);
        settle(&app, cx).await;
        assert!(!dialog(cx), "the review closes as the write runs");
        assert_eq!(
            app.read_with(cx, |app, _| (
                app.operation_busy,
                app.operation_error.clone()
            )),
            (None, Some(NO_LONGER_UNTRACKED.to_owned())),
            "the write refuses the file staged since its review, as Preview rule does"
        );
        assert_eq!(
            std::fs::read(&ignore_file).ok(),
            ignored_before,
            "the ignore file is as it was"
        );
    }
}
