//! Contextual ignore choices followed by an exact rule and destination review.
use crate::*;
use gitturtle_core::{IgnoreDestination, StatusEntry, WriteCommand};
use gpui_kit::{
    component::{WindowExt, checkbox::Checkbox, dialog::DialogButtonProps},
    prelude::FluentBuilder,
};

const PREPARING: &str = "Preparing ignore rule…";

fn static_text(id: &'static str, value: impl Into<SharedString>) -> Stateful<Div> {
    let value = value.into();
    div()
        .id(id)
        .role(Role::Label)
        .aria_label(value.clone())
        .child(value)
}

#[derive(Default)]
pub(super) struct State {
    generation: u64,
    task: Option<Task<()>>,
}

impl GitTurtle {
    pub(super) fn cancel_ignore_action(&mut self) {
        self.ignore_actions.generation = self.ignore_actions.generation.wrapping_add(1);
        self.ignore_actions.task = None;
        if self.operation_busy == Some(PREPARING) {
            self.operation_busy = None;
        }
    }
    pub(super) fn open_ignore(
        &mut self,
        entry: StatusEntry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.operation_busy.is_some() || !entry.untracked {
            return;
        }
        let owner = cx.entity().downgrade();
        let path = self.path.clone();
        let form = cx.new(|_| IgnoreForm {
            owner,
            repository: path,
            path: entry.path,
            directory: false,
            local: false,
            pending: false,
            error: None,
        });
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let submit = form.clone();
            let cancel = form.clone();
            dialog
                .title(static_text(
                    "ignore-dialog-title",
                    "Ignore untracked content",
                ))
                .width(px(560.))
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
                .on_cancel(move |_, _, cx| {
                    cancel.update(cx, |form, cx| {
                        form.pending = false;
                        let _ = form.owner.update(cx, |this, _| this.cancel_ignore_action());
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
    pending: bool,
    error: Option<String>,
}
impl IgnoreForm {
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending {
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
            if this.path != self.repository || this.operation_busy.is_some() || this.page != AppPage::Repository { return false; }
            let Some(repo) = this.repository.clone() else { return false; };
            let repository = repo.path().to_owned(); this.cancel_ignore_action();
            let generation = this.ignore_actions.generation;
            this.operation_busy = Some(PREPARING); this.operation_error = None;
            let response = this.operations.submit_read(move || repo.ignore_plan(&path, directory, destination));
            this.ignore_actions.task = Some(cx.spawn_in(window, async move |this, cx| {
                let result = response.await.unwrap_or_else(|_| Err(anyhow::anyhow!("Ignore preparation was interrupted; review the rule again.")));
                let _ = this.update_in(cx, |this, window, cx| {
                    if this.ignore_actions.generation != generation { return; }
                    if this.operation_busy == Some(PREPARING) { this.operation_busy = None; }
                    if this.path.as_ref() != Some(&repository) || this.page != AppPage::Repository { cx.notify(); return; }
                    let _ = form.update(cx, |form, cx| { form.pending = false; cx.notify(); });
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
            })); cx.notify(); true
        }).unwrap_or(false);
        self.pending = accepted;
        if !accepted {
            self.error = Some("The repository changed or another operation is running. Reopen Ignore after it finishes.".into());
        }
        cx.notify();
    }
}
impl Render for IgnoreForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let parent = self
            .path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty());
        div().flex().flex_col().gap_3()
            .child(static_text("ignore-selected-path", format!("Selected untracked file: {}", self.path.display())).text_size(crate::appearance::ui_text(12.)))
            .child(static_text("ignore-destination-label", "Save rule in").text_size(crate::appearance::ui_text(12.)).font_weight(FontWeight::MEDIUM))
            .child(div().flex().gap_2().children([(false, "Shared .gitignore"), (true, "Local excludes")].map(|(local, label)| button(label, label, "", self.local == local).toggled(self.local == local).disabled(self.pending).on_click(cx.listener(move |this, _, _, cx| { this.local = local; this.error = None; cx.notify(); })))))
            .child(static_text("ignore-destination-help", if self.local { "For this repository's local setup. Stored in Git's info/exclude, including linked worktrees." } else { "For patterns the project should share. Changes to .gitignore remain unstaged for review." }).text_size(crate::appearance::ui_text(11.)).text_color(rgb(p.muted)))
            .when_some(parent, |element, parent| element.child(Checkbox::new("ignore-containing-directory").label(format!("Ignore containing directory: {}/", parent.display())).checked(self.directory).disabled(self.pending).on_click(cx.listener(|this, checked, _, cx| { this.directory = *checked; this.error = None; cx.notify(); }))))
            .child(static_text("ignore-tracked-explanation", "The next step previews the exact escaped rule and destination. Ignore rules affect untracked content; they never remove tracked files from Git.").text_size(crate::appearance::ui_text(11.)).text_color(rgb(p.muted)))
            .when(self.pending, |element| element.child(static_text("ignore-preparation-status", "Preparing exact rule and checking current content…").text_size(crate::appearance::ui_text(12.))))
            .children(self.error.as_ref().map(|error| static_text("ignore-preparation-error", error.clone()).text_size(crate::appearance::ui_text(12.)).text_color(rgb(p.warning))))
    }
}
