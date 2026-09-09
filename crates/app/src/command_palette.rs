//! A bounded registry of native workflows. Highlighting a command is inert;
//! activation rechecks the current context before invoking its existing handler.
use crate::*;
use gpui_kit::{
    component::{WindowExt, dialog::DialogFooter},
    prelude::FluentBuilder,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CommandId {
    Projects,
    Open,
    History,
    Changes,
    QuickOpen,
    Compare,
    Branch,
    Worktrees,
    Tags,
    Reflog,
    Rebase,
    RewriteReview,
    FileHistory,
    Blame,
    Profiles,
    Appearance,
    Settings,
    RecoveryDrafts,
    Activity,
    Refresh,
    Search,
    Sidebar,
    Editor,
    Reveal,
    Help,
}
#[derive(Clone, Copy)]
pub(super) struct CommandSpec {
    pub id: CommandId,
    pub label: &'static str,
    keywords: &'static str,
    shortcut: &'static str,
}
macro_rules! command {
    ($id:ident, $label:literal, $keywords:literal, $shortcut:literal) => {
        CommandSpec {
            id: CommandId::$id,
            label: $label,
            keywords: $keywords,
            shortcut: $shortcut,
        }
    };
}
pub(super) const COMMANDS: &[CommandSpec] = &[
    command!(
        Projects,
        "Go to Projects",
        "hub home repositories recent",
        "⇧O"
    ),
    command!(
        Open,
        "Open a repository…",
        "folder project choose browse",
        "O"
    ),
    command!(History, "Go to History", "commits graph log", "1"),
    command!(
        Changes,
        "Go to Working Changes",
        "status stage unstage commit composer",
        "2"
    ),
    command!(
        QuickOpen,
        "Quick Open File…",
        "find path source tracked",
        "P"
    ),
    command!(
        Compare,
        "Compare revisions…",
        "diff branch tag commit before after",
        "⇧C"
    ),
    command!(
        Branch,
        "Manage current branch…",
        "rename delete upstream tracking checkout switch",
        ""
    ),
    command!(
        Worktrees,
        "Manage worktrees…",
        "linked create add remove open",
        ""
    ),
    command!(
        Tags,
        "Browse and manage tags…",
        "annotate release create delete push",
        ""
    ),
    command!(
        Reflog,
        "Browse reflog and recover a commit…",
        "lost undo history recovery",
        ""
    ),
    command!(
        Rebase,
        "Review an interactive rebase…",
        "rewrite reorder squash fixup drop reword",
        ""
    ),
    command!(
        RewriteReview,
        "Review rewritten branch series…",
        "rebase original changed commits publish force lease push",
        ""
    ),
    command!(
        FileHistory,
        "Open selected file history",
        "path log rename previous revisions",
        ""
    ),
    command!(
        Blame,
        "Show selected file attribution",
        "blame author annotate line history",
        ""
    ),
    command!(
        Profiles,
        "Manage Git profiles…",
        "personal work author name email identity signing",
        ""
    ),
    command!(
        Appearance,
        "Change appearance and themes…",
        "braden daylight light dark density interface code text size",
        ""
    ),
    command!(
        Settings,
        "Open Settings",
        "preferences editor default branch configuration",
        ","
    ),
    command!(
        RecoveryDrafts,
        "Recover saved conflict and rebase drafts…",
        "unfinished text restart restore copy",
        ""
    ),
    command!(
        Activity,
        "Show GitTurtle activity…",
        "operations errors cancelled outcomes",
        "⇧A"
    ),
    command!(
        Refresh,
        "Refresh local repository state",
        "reload status refs files",
        "R"
    ),
    command!(
        Search,
        "Search commits",
        "find message author email hash",
        "F"
    ),
    command!(
        Sidebar,
        "Toggle repository sidebar",
        "navigation branches references hide show",
        "B"
    ),
    command!(
        Editor,
        "Open repository in configured editor",
        "external ide code",
        ""
    ),
    command!(
        Reveal,
        "Reveal repository in file manager",
        "finder folder external",
        ""
    ),
    command!(Help, "Show keyboard shortcuts", "help keys commands", "⇧/"),
];

#[derive(Clone, Default)]
struct Availability {
    repository: bool,
    workspace: bool,
    busy: bool,
    branch: bool,
    file: bool,
    working_file: bool,
    working: bool,
    file_history: bool,
    blame: bool,
    integration: bool,
    profile_saving: bool,
}
impl CommandId {
    fn workspace(self) -> bool {
        matches!(
            self,
            Self::QuickOpen
                | Self::Compare
                | Self::Branch
                | Self::Worktrees
                | Self::Tags
                | Self::Reflog
                | Self::Rebase
                | Self::RewriteReview
                | Self::FileHistory
                | Self::Blame
                | Self::Refresh
                | Self::Search
                | Self::Sidebar
        )
    }
    fn selected_file(self) -> bool {
        matches!(self, Self::FileHistory | Self::Blame)
    }
    fn reason(self, context: &Availability) -> Option<&'static str> {
        if context.busy && !matches!(self, Self::Activity | Self::Help) {
            return Some("Wait for the current Git operation to finish");
        }
        if (self.workspace()
            || matches!(
                self,
                Self::History | Self::Changes | Self::Editor | Self::Reveal
            ))
            && !context.repository
        {
            return Some("Open a repository first");
        }
        if self.workspace() && !context.workspace {
            return Some("Return to the repository workspace first");
        }
        if self == Self::Branch && !context.branch {
            return Some("Check out a named local branch first");
        }
        if self == Self::Rebase && context.integration {
            return Some("Finish or abort the current Git operation first");
        }
        if self == Self::FileHistory && (context.working || context.file_history || !context.file) {
            return Some("Select a history file or open a tracked file first");
        }
        if self == Self::Blame && (context.blame || !(context.file || context.working_file)) {
            return Some("Select a file whose attribution is not already open");
        }
        if self == Self::Profiles && context.profile_saving {
            return Some("Wait for the profile save to finish");
        }
        None
    }
}
fn matches(query: &str) -> Vec<usize> {
    if query.len() > 4096 {
        return Vec::new();
    }
    let query = query.trim().to_lowercase();
    let terms: Vec<_> = query.split_whitespace().collect();
    let mut matches: Vec<_> = COMMANDS
        .iter()
        .enumerate()
        .filter_map(|(index, spec)| {
            let name = spec.label.to_lowercase();
            let haystack = format!("{name} {}", spec.keywords);
            terms.iter().all(|term| haystack.contains(term)).then_some((
                if name.starts_with(&query) {
                    0
                } else if name.contains(&query) {
                    1
                } else {
                    2
                },
                index,
            ))
        })
        .collect();
    matches.sort_unstable();
    matches
        .into_iter()
        .map(|(_, index)| index)
        .take(40)
        .collect()
}
#[derive(Default)]
struct Selection {
    index: usize,
    closed: bool,
}
impl Selection {
    fn move_by(&mut self, delta: i32, count: usize) {
        self.index = self
            .index
            .saturating_add_signed(delta as isize)
            .min(count.saturating_sub(1));
    }
    fn claim(&mut self, allowed: bool) -> bool {
        if self.closed || !allowed {
            return false;
        }
        self.closed = true;
        true
    }
}
impl GitTurtle {
    fn palette_availability(&self) -> Availability {
        Availability {
            repository: self.repository.is_some(),
            workspace: self.page == AppPage::Repository,
            busy: self.operation_busy.is_some(),
            branch: self
                .work_status
                .as_ref()
                .is_some_and(|status| status.branch.is_some()),
            file: self.selected_file.is_some(),
            working_file: self.working_selected.is_some(),
            working: self.mode == WorkspaceMode::Working,
            file_history: self.file_history.is_active(),
            blame: self.blame.is_visible(),
            integration: self.integration_state.is_some(),
            profile_saving: self.profile_save_pending(),
        }
    }
    pub(super) fn open_command_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if window.has_active_dialog(cx) || window.has_active_sheet(cx) {
            return;
        }
        let owner = cx.entity().downgrade();
        let path = self.path.clone();
        let return_focus = window.focused(cx);
        let form = cx.new(|cx| Palette::new(owner, path, return_focus, window, cx));
        let focus_form = form.downgrade();
        window.open_alert_dialog(cx, move |dialog, _, cx| {
            let cancel = form.clone();
            let cancel_action = form.clone();
            let activate = form.clone();
            let submit = form.clone();
            dialog
                .title("Command palette")
                .width(px(660.))
                .child(form.clone())
                .footer(
                    DialogFooter::new()
                        .child(
                            button("cancel-command-palette", "Cancel", "", false).on_click(
                                move |_, window, cx| {
                                    cancel.update(cx, |form, cx| form.cancel(window, cx))
                                },
                            ),
                        )
                        .child(
                            button("run-palette-command", "Open command", "", true)
                                .disabled(!form.read(cx).can_activate(cx))
                                .on_click(move |_, window, cx| {
                                    activate.update(cx, |form, cx| form.activate(window, cx))
                                }),
                        ),
                )
                .on_ok(move |_, window, cx| {
                    submit.update(cx, |form, cx| form.activate(window, cx));
                    false
                })
                .on_cancel(move |_, window, cx| {
                    cancel_action.update(cx, |form, cx| form.cancel(window, cx));
                    false
                })
        });
        window.refresh();
        window.on_next_frame(move |window, cx| {
            let _ = focus_form.update(cx, |form, cx| {
                if !form.selection.closed {
                    form.query.read(cx).focus_handle(cx).focus(window, cx);
                }
            });
        });
    }
    fn run_palette_command(&mut self, id: CommandId, window: &mut Window, cx: &mut Context<Self>) {
        match id {
            CommandId::Projects => self.show_projects(window, cx),
            CommandId::Open => self.choose_repository(&OpenRepository, window, cx),
            CommandId::History => self.show_history(window, cx),
            CommandId::Changes => self.show_working(window, cx),
            CommandId::QuickOpen => self.open_quick_file(window, cx),
            CommandId::Compare => self.open_revision_comparison(window, cx),
            CommandId::Branch => {
                if let Some(branch) = self
                    .work_status
                    .as_ref()
                    .and_then(|status| status.branch.clone())
                {
                    self.open_contextual_branch(branch, false, window, cx);
                }
            }
            CommandId::Worktrees => self.open_worktree_manager(window, cx),
            CommandId::Tags => self.open_tags(window, cx),
            CommandId::Reflog => self.open_reflog_browser(window, cx),
            CommandId::Rebase => self.open_interactive_rebase(None, window, cx),
            CommandId::RewriteReview => self.open_rewrite_review(window, cx),
            CommandId::FileHistory => {
                if let Some(index) = self.selected_file {
                    self.open_file_history(index, window, cx);
                }
            }
            CommandId::Blame => {
                if let Some(index) = if self.mode == WorkspaceMode::Working {
                    self.working_selected.map(|(index, _)| index)
                } else {
                    self.selected_file
                } {
                    self.open_blame(index, window, cx);
                }
            }
            CommandId::Profiles => self.open_profiles(window, cx),
            CommandId::Appearance | CommandId::Settings => self.show_settings(window, cx),
            CommandId::RecoveryDrafts => self.open_recovery_drafts(window, cx),
            CommandId::Activity => self.open_activity(window, cx),
            CommandId::Refresh => self.refresh(&Refresh, window, cx),
            CommandId::Search => {
                self.show_history(window, cx);
                self.search(&Search, window, cx);
            }
            CommandId::Sidebar => {
                if self.mode != WorkspaceMode::History {
                    self.back_to_history(window, cx);
                } else {
                    self.sidebar = !self.sidebar;
                    self.history_sidebar = self.sidebar;
                    cx.notify();
                }
            }
            CommandId::Editor => self.open_external_editor(window, cx),
            CommandId::Reveal => {
                if let Some(repo) = &self.repository {
                    cx.reveal_path(repo.path());
                }
            }
            CommandId::Help => self.shortcut_help(window, cx),
        }
        window.refresh();
    }
}
struct Palette {
    owner: WeakEntity<GitTurtle>,
    path: Option<PathBuf>,
    return_focus: Option<FocusHandle>,
    query: Entity<InputState>,
    results: Vec<usize>,
    selection: Selection,
    scroll: UniformListScrollHandle,
    error: Option<String>,
    _subscription: Subscription,
}
impl Palette {
    fn new(
        owner: WeakEntity<GitTurtle>,
        path: Option<PathBuf>,
        return_focus: Option<FocusHandle>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let query = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Search commands, such as profiles, blame, or themes…")
        });
        let subscription =
            cx.subscribe_in(&query, window, |this, _, event, window, cx| match event {
                InputEvent::Change => {
                    this.results = matches(&this.query.read(cx).value());
                    this.selection.index = 0;
                    this.error = None;
                    this.scroll.scroll_to_item(0, ScrollStrategy::Top);
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => this.activate(window, cx),
                _ => {}
            });
        Self {
            owner,
            path,
            return_focus,
            query,
            results: matches(""),
            selection: Selection::default(),
            scroll: UniformListScrollHandle::new(),
            error: None,
            _subscription: subscription,
        }
    }
    fn can_activate(&self, cx: &App) -> bool {
        self.results.get(self.selection.index).is_some_and(|index| {
            self.owner.upgrade().is_some_and(|owner| {
                let owner = owner.read(cx);
                self.path == owner.path
                    && COMMANDS[*index]
                        .id
                        .reason(&owner.palette_availability())
                        .is_none()
            })
        })
    }
    fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.selection.claim(true) {
            return;
        }
        window.close_dialog(cx);
        window.refresh();
        if let Some(focus) = &self.return_focus {
            focus.focus(window, cx);
        }
    }
    fn activate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.selection.closed {
            return;
        }
        let Some(index) = self.results.get(self.selection.index).copied() else {
            return;
        };
        let command = COMMANDS[index].id;
        let _ = self.owner.update(cx, |owner, cx| {
            if owner.path != self.path {
                self.error =
                    Some("The repository changed. Close and reopen the command palette.".into());
                return;
            }
            if let Some(reason) = command.reason(&owner.palette_availability()) {
                self.error = Some(reason.into());
                return;
            }
            if !self.selection.claim(true) {
                return;
            }
            window.close_dialog(cx);
            window.refresh();
            if let Some(focus) = &self.return_focus {
                focus.focus(window, cx);
            }
            owner.run_palette_command(command, window, cx);
        });
        cx.notify();
    }
}
impl Render for Palette {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let count = self.results.len();
        let context = self
            .owner
            .upgrade()
            .map(|owner| owner.read(cx).palette_availability())
            .unwrap_or_default();
        let scope = self
            .owner
            .upgrade()
            .map(|owner| {
                let owner = owner.read(cx);
                owner
                    .repository
                    .as_ref()
                    .map(|repo| {
                        format!(
                            "{} · {}",
                            repo.name(),
                            if owner.page == AppPage::Repository {
                                "current workspace"
                            } else {
                                "retained repository"
                            }
                        )
                    })
                    .unwrap_or_else(|| "Application · no repository open".into())
            })
            .unwrap_or_default();
        let list = uniform_list(
            "command-palette-list",
            count,
            cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                let context = this
                    .owner
                    .upgrade()
                    .map(|owner| owner.read(cx).palette_availability())
                    .unwrap_or_default();
                range
                    .map(|index| {
                        let colors = palette(cx);
                        let spec = COMMANDS[this.results[index]];
                        let disabled = spec.id.reason(&context);
                        let selected = this.selection.index == index;
                        let shortcut = if spec.shortcut.is_empty() {
                            String::new()
                        } else {
                            format!("{}{}", primary_label(), spec.shortcut)
                        };
                        div()
                            .id(("palette-command", index))
                            .role(Role::ListBoxOption)
                            .aria_label(format!(
                                "{}{}",
                                spec.label,
                                disabled
                                    .map(|reason| format!(". Unavailable: {reason}"))
                                    .unwrap_or_default()
                            ))
                            .aria_selected(selected)
                            .w_full()
                            .h(crate::appearance::ui_size(54.))
                            .px_3()
                            .py_2()
                            .flex()
                            .flex_col()
                            .justify_center()
                            .gap_1()
                            .bg(rgb(if selected {
                                palette(cx).selected
                            } else {
                                palette(cx).panel
                            }))
                            .text_color(rgb(if disabled.is_some() {
                                palette(cx).muted
                            } else {
                                palette(cx).text
                            }))
                            .when(disabled.is_none(), |element| {
                                element
                                    .cursor_pointer()
                                    .hover(move |style| style.bg(rgb(colors.hover)))
                            })
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_3()
                                    .child(
                                        div()
                                            .min_w_0()
                                            .flex_1()
                                            .truncate()
                                            .font_weight(FontWeight::MEDIUM)
                                            .child(spec.label),
                                    )
                                    .child(
                                        div()
                                            .flex_shrink_0()
                                            .text_size(crate::appearance::ui_text(11.))
                                            .text_color(rgb(palette(cx).muted))
                                            .child(shortcut),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(crate::appearance::ui_text(11.))
                                    .text_color(rgb(palette(cx).muted))
                                    .truncate()
                                    .child(disabled.unwrap_or(if spec.id.selected_file() {
                                        "Selected file · opens its existing inspection workflow"
                                    } else if spec.id.workspace() {
                                        "Current repository · opens the existing workflow"
                                    } else {
                                        "Application action"
                                    })),
                            )
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.selection.index = index;
                                this.activate(window, cx);
                            }))
                            .into_any_element()
                    })
                    .collect()
            }),
        )
        .track_scroll(&self.scroll)
        .size_full();
        let reason = self
            .results
            .get(self.selection.index)
            .and_then(|index| COMMANDS[*index].id.reason(&context));
        div()
            .id("command-palette-form")
            .key_context("GitTurtlePalette")
            .flex()
            .flex_col()
            .gap_3()
            .on_action(
                cx.listener(|this, _: &gpui_kit::component::input::Escape, window, cx| {
                    this.cancel(window, cx);
                    cx.stop_propagation();
                }),
            )
            .on_action(cx.listener(|this, _: &ClearSearch, window, cx| {
                this.cancel(window, cx);
                cx.stop_propagation();
            }))
            .on_action(cx.listener(
                |this, _: &gpui_kit::component::dialog::Cancel, window, cx| {
                    this.cancel(window, cx);
                    cx.stop_propagation();
                },
            ))
            .on_action(cx.listener(|this, _: &ShowCommandPalette, window, cx| {
                this.query.read(cx).focus_handle(cx).focus(window, cx);
                cx.stop_propagation();
            }))
            .on_action(|_: &QuickOpenFile, _, cx| cx.stop_propagation())
            .on_action(|_: &ShowProjects, _, cx| cx.stop_propagation())
            .on_action(|_: &ShowHistory, _, cx| cx.stop_propagation())
            .on_action(|_: &ShowChanges, _, cx| cx.stop_propagation())
            .on_action(|_: &ShowSettings, _, cx| cx.stop_propagation())
            .on_action(|_: &Refresh, _, cx| cx.stop_propagation())
            .on_action(|_: &CompareRevisions, _, cx| cx.stop_propagation())
            .on_action(|_: &ShowActivity, _, cx| cx.stop_propagation())
            .on_action(|_: &OpenRepository, _, cx| cx.stop_propagation())
            .on_action(|_: &BackHistory, _, cx| cx.stop_propagation())
            .on_action(|_: &ToggleSidebar, _, cx| cx.stop_propagation())
            .on_action(|_: &Search, _, cx| cx.stop_propagation())
            .on_action(|_: &ShortcutHelp, _, cx| cx.stop_propagation())
            .on_action(|_: &RevealRepository, _, cx| cx.stop_propagation())
            .on_action(|_: &OpenEditor, _, cx| cx.stop_propagation())
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                let delta = match event.keystroke.key.as_str() {
                    "down" => 1,
                    "up" => -1,
                    _ => return,
                };
                this.selection.move_by(delta, this.results.len());
                this.scroll
                    .scroll_to_item(this.selection.index, ScrollStrategy::Center);
                cx.stop_propagation();
                cx.notify();
            }))
            .child(
                Input::new(&self.query)
                    .aria_label("Search app commands")
                    .cleanable(true),
            )
            .child(
                div()
                    .text_size(crate::appearance::ui_text(12.))
                    .text_color(rgb(p.muted))
                    .truncate()
                    .child(scope),
            )
            .child(
                div()
                    .id("command-palette-results")
                    .role(Role::ListBox)
                    .aria_label("Matching commands")
                    .h((window.viewport_size().height - px(300.)).clamp(px(150.), px(360.)))
                    .overflow_hidden()
                    .border_1()
                    .border_color(rgb(p.border))
                    .rounded(px(6.))
                    .when(count > 0, |element| element.child(list))
                    .when(count == 0, |element| {
                        element.child(div().p_4().text_color(rgb(p.muted)).child(
                            if self.query.read(cx).value().len() > 4096 {
                                "The search is too long. Use up to 4,096 bytes."
                            } else {
                                "No matching commands. Try profiles, compare, branches, or themes."
                            },
                        ))
                    }),
            )
            .child(
                div()
                    .text_size(crate::appearance::ui_text(11.))
                    .text_color(rgb(p.muted))
                    .child(reason.unwrap_or(
                        "↑ / ↓ select · Return opens · Escape returns to your previous focus",
                    )),
            )
            .children(
                self.error
                    .as_ref()
                    .map(|error| div().text_color(rgb(p.warning)).child(error.clone())),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::{Availability, COMMANDS, CommandId, Selection, matches};
    #[test]
    fn search_finds_synonyms_and_respects_all_terms_and_bounds() {
        for (query, expected) in [
            ("personal", CommandId::Profiles),
            ("squash", CommandId::Rebase),
            ("blame", CommandId::Blame),
            ("light dark", CommandId::Appearance),
            ("lost commit", CommandId::Reflog),
        ] {
            assert!(
                matches(query)
                    .iter()
                    .any(|index| COMMANDS[*index].id == expected)
            );
        }
        assert!(matches("profiles totallyunrelatedterm").is_empty());
        assert!(matches(&"a".repeat(4097)).is_empty());
        assert!(matches("").len() <= 40);
    }
    #[test]
    fn availability_distinguishes_hub_busy_selection_and_recovery_states() {
        let mut context = Availability::default();
        assert!(CommandId::Profiles.reason(&context).is_none());
        assert!(CommandId::QuickOpen.reason(&context).is_some());
        context.repository = true;
        assert!(CommandId::History.reason(&context).is_none());
        assert!(CommandId::QuickOpen.reason(&context).is_some());
        context.workspace = true;
        assert!(CommandId::QuickOpen.reason(&context).is_none());
        assert!(CommandId::Blame.reason(&context).is_some());
        context.working_file = true;
        assert!(CommandId::Blame.reason(&context).is_none());
        context.integration = true;
        assert!(CommandId::Rebase.reason(&context).is_some());
        context.busy = true;
        assert!(CommandId::Profiles.reason(&context).is_some());
        assert!(CommandId::Activity.reason(&context).is_none());
    }
    #[test]
    fn selection_is_bounded_and_enter_or_cancel_can_only_claim_once() {
        let mut selection = Selection::default();
        selection.move_by(-1, 0);
        assert_eq!(selection.index, 0);
        selection.move_by(10, 3);
        assert_eq!(selection.index, 2);
        assert!(!selection.claim(false));
        assert!(selection.claim(true));
        assert!(!selection.claim(true));
    }
}
