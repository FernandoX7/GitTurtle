//! Transient native branch and remote workflows. All Git preparation runs on
//! the serialized worker; confirmations retain the exact repository and plan.
use crate::*;
use focus_reveal::{FocusReveal, Trigger};
use gitturtle_core::{BranchCommand, BranchPlan, RemoteConfig, WriteCommand};
use gpui_kit::component::{
    WindowExt,
    checkbox::Checkbox,
    menu::{PopupMenu, PopupMenuItem},
};
use gpui_kit::prelude::FluentBuilder;

pub(crate) const PREPARING: &str = "Preparing branch action…";
const CHOICE_LIMIT: usize = 40;
type PreparationFailure = Box<dyn FnOnce(String, &mut Window, &mut Context<GitTurtle>)>;

#[cfg(test)]
thread_local! {
    /// The label the branch picker last drew, for a test to read back.
    pub(crate) static PICKER_LABEL: std::cell::RefCell<Option<String>> =
        const { std::cell::RefCell::new(None) };
    /// Each item the branch picker's menu was last built with, in order
    /// ([`picker_item`]), for a test to walk every item it refuses.
    pub(crate) static PICKER_ITEMS: std::cell::RefCell<Vec<PickerItem>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// What a test reads of an item of the branch picker's menu
/// ([`PICKER_ITEMS`]).
#[cfg(test)]
#[derive(Clone, Debug)]
pub(crate) struct PickerItem {
    /// The command it draws.
    pub(crate) label: String,
    /// The name assistive technology reads, where it says more.
    pub(crate) name: Option<String>,
    pub(crate) disabled: bool,
    /// Whether it states why it is refused, so the menu keys rest on it.
    pub(crate) reason_stated: bool,
    /// Whether it is a check item, and ticked.
    pub(crate) checked: Option<bool>,
}

/// What the branch picker's text ([`PickerText`]) is made of, borrowed from
/// the window's state, so a frame compares it with what each kept text was
/// built from without building anything: the picker cloned the branch's
/// name and formatted its label, accessible name and tooltip in every
/// frame, only to draw the same text. The text is built again only when one
/// of these changes ([`GitTurtle::branch_picker`]); it holds every form the
/// bars draw ([`PickerForm`]), so a resize, a text size or the compact bar
/// builds nothing. These are everything [`PickerText::build`] reads: a
/// detached HEAD's commit decides the picker's edge, as
/// `RepositoryEdge::of` finds a detached HEAD exactly where no branch is
/// checked out and HEAD has a commit, and no other edge changes its text.
#[derive(PartialEq)]
struct PickerInputs<'a> {
    /// The branch checked out, if any.
    branch: Option<Cow<'a, str>>,
    /// HEAD's commit where no branch is checked out.
    detached: Option<Cow<'a, str>>,
    /// The word a switch chosen from the menu, or a tracking branch created
    /// from it, confirms with while it confirms
    /// ([`GitTurtle::picker_confirmation`]).
    done: Option<&'static str>,
}

impl PickerInputs<'_> {
    /// These inputs as a key a cache keeps, owning what they borrowed.
    fn owned(&self) -> PickerInputs<'static> {
        let text = |text: &Cow<str>| Cow::Owned(String::from(&**text));
        PickerInputs {
            branch: self.branch.as_ref().map(text),
            detached: self.detached.as_ref().map(text),
            done: self.done,
        }
    }
}

/// The forms the bars draw the branch picker in, by the index of its label
/// in [`PickerText::labels`].
#[derive(Clone, Copy)]
enum PickerForm {
    /// The full action bar.
    Full,
    /// The compact bar's row.
    Compact,
    /// The compact bar's row where its repository edge has no room for its
    /// line (`empty_state::EdgeForm::glyph`).
    Narrow,
}

/// The branch picker's text, as [`PickerInputs`] make it, each string
/// shared, so a frame hands it to the picker without copying it.
struct PickerText {
    /// The label in each [`PickerForm`].
    labels: [SharedString; 3],
    /// The accessible name, the same in every form.
    name: SharedString,
    /// The tooltip, and in the [`PickerForm::Narrow`] form, where a detached
    /// HEAD's line leaves for the picker, which carries the line's advice
    /// after its name.
    tooltip: SharedString,
    narrow_tooltip: SharedString,
}

impl PickerText {
    /// The picker's text for `inputs` in every form, built only when they
    /// change ([`PICKER_TEXTS`]).
    fn build(inputs: &PickerInputs) -> Self {
        #[cfg(test)]
        PICKER_TEXTS_BUILT.with(|built| built.set(built.get() + 1));
        let branch: SharedString =
            String::from(inputs.branch.as_deref().unwrap_or("Detached HEAD")).into();
        // Detached, HEAD is a commit, and the picker says which, as
        // "Detached HEAD at" its short ID. In the compact bar, whose line
        // then says only "Detached", its label is that ID, whole where
        // "Detached HEAD" would be cut, and where even that line has no
        // room the picker says "Detached" in its place: one commit glyph and
        // the word (`empty_state::RepositoryEdge::compact_branch`).
        let edge =
            (inputs.detached.as_deref()).map(|oid| crate::empty_state::RepositoryEdge::Detached {
                oid: oid.to_owned(),
            });
        let named: SharedString = match inputs.detached.as_deref() {
            Some(oid) => format!("Detached HEAD at {}", short_oid(oid)).into(),
            None => branch.clone(),
        };
        let label = |narrow| {
            (edge.as_ref())
                .and_then(|edge| edge.compact_branch(narrow))
                .map_or_else(|| branch.clone(), SharedString::from)
        };
        // Where the line leaves for the picker, which says its word, the
        // picker's tooltip carries the line's advice after its name.
        let advice = (edge.as_ref())
            .filter(|edge| edge.glyph().is_none())
            .map(|edge| edge.advice());
        let tooltip = |advice: Option<&str>| -> SharedString {
            match (inputs.done, advice) {
                (Some(done), _) => format!("{done} · {branch}"),
                (None, Some(advice)) => {
                    format!("{named} · {advice} · Switch branch and repository actions")
                }
                (None, None) => format!("{named} · Switch branch and repository actions"),
            }
            .into()
        };
        Self {
            labels: [branch.clone(), label(false), label(true)],
            name: format!("Current branch: {named}. Switch or manage branches").into(),
            tooltip: tooltip(None),
            narrow_tooltip: tooltip(advice),
        }
    }
}

/// The branch picker's text as drawn lately, by what it is made of.
type PickerTexts = std::cell::RefCell<crate::views::Measured<PickerInputs<'static>, PickerText>>;

thread_local! {
    /// The branch picker's text as drawn lately, by what it is made of
    /// ([`GitTurtle::branch_picker`]).
    static PICKER_TEXTS: PickerTexts = const { std::cell::RefCell::new(crate::views::Measured::new()) };
}

#[cfg(test)]
thread_local! {
    /// How many times the branch picker's text was built
    /// ([`PickerText::build`]), for a test.
    pub(crate) static PICKER_TEXTS_BUILT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[derive(Default)]
pub(super) struct State {
    generation: u64,
    task: Option<Task<()>>,
    /// Whether the running preparation's outcome is no longer wanted
    /// ([`GitTurtle::dismiss_branch_action`]).
    dismissed: bool,
    /// Whether Changes asked for the running preparation, which History
    /// shown again drops rather than wants again
    /// ([`GitTurtle::branch_action_asked_in_changes`]).
    asked_in_changes: bool,
    /// The list of the chooser or remote manager opened last, for tests.
    #[cfg(test)]
    list: Option<FocusReveal>,
}

#[derive(Clone)]
enum ChoicePurpose {
    Track,
    Manage,
    Integrate { rebase: bool },
    Upstream(Arc<BranchPlan>),
}

impl ChoicePurpose {
    fn title(&self) -> String {
        match self {
            Self::Track => "Create a tracking branch".into(),
            Self::Manage => "Manage a local branch".into(),
            Self::Integrate { rebase: false } => "Choose a branch to merge".into(),
            Self::Integrate { rebase: true } => "Choose a new base".into(),
            Self::Upstream(branch) => format!("Upstream for {}", branch.name),
        }
    }
}

struct BranchChoice {
    name: String,
    reference: String,
    oid: String,
    remote: bool,
    search: String,
}

struct RemoteChoice {
    config: Arc<RemoteConfig>,
    search: String,
}

enum Prepared {
    Choices {
        purpose: ChoicePurpose,
        choices: Arc<Vec<BranchChoice>>,
    },
    Local(Arc<BranchPlan>),
    Remote {
        reference: String,
        oid: String,
    },
    Remotes(Arc<Vec<RemoteChoice>>),
    Confirmation {
        title: String,
        explanation: String,
        action: &'static str,
        command: WriteCommand,
    },
}

#[derive(Clone, Copy)]
enum MenuAction {
    ManageCurrent,
    ManageLocal,
    Track,
    Merge,
    Rebase,
    InteractiveRebase,
    Remotes,
    Tags,
    Worktrees,
    CompareRevisions,
    QuickFile,
    Activity,
    Reflog,
}

impl GitTurtle {
    pub(super) fn cancel_branch_action(&mut self) {
        self.branch_actions.generation = self.branch_actions.generation.wrapping_add(1);
        self.branch_actions.task = None;
        self.branch_actions.dismissed = false;
        if self.operation_busy == Some(PREPARING) {
            self.operation_busy = None;
        }
    }

    /// Drops what the running preparation brings while its read still ends,
    /// as Changes opens over the view that asked for it, or wants it again
    /// (`dismissed` false) as that view shows again
    /// ([`Self::dismiss_recovery_read`]).
    pub(super) fn dismiss_branch_action(&mut self, dismissed: bool) {
        self.branch_actions.dismissed = dismissed && self.branch_actions.task.is_some();
    }

    /// Whether the running preparation is one Changes asked for, as its
    /// branch picker's actions ask: History shown again drops it rather
    /// than want what it brings, which would open over History
    /// ([`Self::recovery_asked_in_changes`]).
    pub(super) fn branch_action_asked_in_changes(&self) -> bool {
        self.branch_actions.task.is_some() && self.branch_actions.asked_in_changes
    }

    /// The branch picker's control, without its menu
    /// ([`Self::render_branch_picker`]): its glyph, label, name and tooltip,
    /// which a test reads as the window writes them. `compact`, in the
    /// action bar's compact row, it is a 28 px control that gives up its
    /// width, truncating the branch name, before its row wraps, and at most
    /// `most` wide where given: the room the ahead and behind counts leave
    /// it (`GitTurtle::sync_fit`). `narrow` where the bar's repository edge
    /// has no room for its line (`empty_state::EdgeForm::glyph`).
    pub(crate) fn branch_picker(
        &self,
        compact: bool,
        narrow: bool,
        most: Option<Pixels>,
        cx: &mut Context<Self>,
    ) -> Button {
        let status = self.work_status.as_deref();
        let inputs = PickerInputs {
            branch: status
                .and_then(|status| status.branch.as_deref())
                .map(Cow::Borrowed),
            detached: status
                .filter(|status| status.branch.is_none())
                .and_then(|status| status.head.as_deref())
                .map(Cow::Borrowed),
            done: self.picker_confirmation(cx),
        };
        let form = match (compact, narrow) {
            (false, _) => PickerForm::Full,
            (true, false) => PickerForm::Compact,
            (true, true) => PickerForm::Narrow,
        };
        // The picker's text, built only when what it is made of changes
        // ([`PickerInputs`]), in every form the bars draw.
        let (label, name, tooltip) = PICKER_TEXTS.with_borrow_mut(|texts| {
            let text =
                texts.get_where(|kept| *kept == inputs, || inputs.owned(), PickerText::build);
            let tooltip = match form {
                PickerForm::Narrow => &text.narrow_tooltip,
                PickerForm::Full | PickerForm::Compact => &text.tooltip,
            };
            (
                text.labels[form as usize].clone(),
                text.name.clone(),
                tooltip.clone(),
            )
        });
        // Detached, HEAD is a commit: the picker draws the commit glyph in
        // both bars, where the full bar drew the branch glyph beside
        // "Detached HEAD" ([`PickerText::build`]).
        let symbol = if inputs.done.is_some() {
            "icons/check.svg"
        } else if inputs.detached.is_some() {
            "icons/commit.svg"
        } else {
            "icons/branch.svg"
        };
        #[cfg(test)]
        PICKER_LABEL.with(|drawn| drawn.replace(Some(label.to_string())));
        crate::views::note_control_icon(&"current-branch".into(), symbol);
        crate::views::note_control_tooltip(&"current-branch".into(), &tooltip);
        Button::new("current-branch")
            .debug_selector(|| "current-branch".into())
            .secondary()
            .h(crate::appearance::ui_size(if compact { 28. } else { 34. }))
            .when(compact, |button| button.flex_shrink(1.))
            .px_3()
            .max_w(most.map_or(px(280.), |most| most.min(px(280.))))
            .text_size(crate::appearance::ui_text(12.))
            .font_weight(FontWeight::MEDIUM)
            .label(label)
            .icon(Icon::default().path(symbol).size(px(16.)))
            .dropdown_caret(true)
            // Its menu opens until a write looks busy, offering none of the
            // items the write refuses; only then does the picker look
            // unavailable (`workspace::BusyLook`). It dimmed the moment any
            // write started, a quick Stage's too.
            .map(|picker| {
                workspace::refusing(
                    picker,
                    || "current-branch".into(),
                    false,
                    self.operation_shown().is_some(),
                )
            })
            .accessibility_label(name)
            .tooltip(tooltip)
    }

    /// The word the branch picker confirms with while a switch chosen from
    /// its menu confirms, once the picker names the branch it switched to:
    /// the check takes the branch icon's place and the tooltip says
    /// Switched, while the label, the width and the name stay
    /// (`copy_feedback`). A tracking branch created and switched to
    /// confirms the same way, its tooltip saying Created. Each mark hashes
    /// what it confirms, so a frame that asks allocates nothing.
    fn picker_confirmation(&self, cx: &App) -> Option<&'static str> {
        let branch = (self.work_status.as_ref())
            .and_then(|status| status.branch.as_deref())
            .unwrap_or("Detached HEAD");
        [
            (
                workspace::confirmations::picker_created(&self.path, branch),
                workspace::confirmations::CREATE,
            ),
            (
                workspace::confirmations::picker(&self.path, branch),
                workspace::confirmations::SWITCH,
            ),
        ]
        .into_iter()
        .find_map(|(mark, action)| copy_feedback::confirmed(&mark, cx).then_some(action.done))
    }

    /// Replaces the existing picker ([`Self::branch_picker`]); its frequent
    /// switch actions remain first.
    pub(super) fn render_branch_picker(
        &self,
        compact: bool,
        narrow: bool,
        most: Option<Pixels>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let owner = cx.entity().downgrade();
        self.branch_picker(compact, narrow, most, cx)
            .app_menu(move |mut menu, _, cx| {
                let Some(view) = owner.upgrade() else {
                    return menu;
                };
                let this = view.read(cx);
                let path = this.path.clone();
                // Read as it opens: a write refuses every item but Activity
                // until its reload lands, and each item it refuses says why
                // in the room its command leaves, as every menu that follows
                // writes does (`native_accessibility::waiting_item`). They
                // once refused with nothing to say why.
                let wait = this.menu_wait();
                let current = this
                    .work_status
                    .as_ref()
                    .and_then(|status| status.branch.as_deref());
                let query = this.branch_name.read(cx).value().trim().to_lowercase();
                menu = menu
                    .label(if query.is_empty() {
                        "Switch branch".to_owned()
                    } else {
                        format!("Branches matching “{query}”")
                    })
                    .max_h(px(430.))
                    .scrollable(true);
                let find_owner = owner.clone();
                let find_path = path.clone();
                #[cfg(test)]
                PICKER_ITEMS.with(|items| items.borrow_mut().clear());
                menu = picker_item(
                    menu,
                    native_accessibility::waiting_item("Find or create a branch…", false, wait)
                        .on_click(move |_, window, cx| {
                            let _ = find_owner.update(cx, |this, cx| {
                                if this.path == find_path
                                    && this.page == AppPage::Repository
                                    && this.operation_busy.is_none()
                                {
                                    this.git_actions_open = true;
                                    this.branch_name.read(cx).focus_handle(cx).focus(window, cx);
                                    cx.notify();
                                }
                            });
                        }),
                )
                .separator();
                let candidates: Vec<_> = this
                    .branches
                    .iter()
                    .filter(|branch| {
                        !branch.remote
                            && Some(branch.name.as_str()) != current
                            && (query.is_empty() || branch.name.to_lowercase().contains(&query))
                    })
                    .take(CHOICE_LIMIT + 1)
                    .collect();
                // The current branch is ticked and cannot be switched to,
                // being the state already, and says so beside its name,
                // "Current branch", heard as "main, current branch", so the
                // menu keys rest on it (`native_accessibility::detailed_item`).
                // Ticked and refused with nothing to say why, it was passed
                // over, and a screen reader never heard which branch was
                // current.
                if let Some(current) = current {
                    menu = picker_item(
                        menu,
                        native_accessibility::detailed_item(current.to_owned(), "Current branch")
                            .checked(true),
                    );
                }
                for branch in candidates.iter().take(CHOICE_LIMIT) {
                    let name = branch.name.clone();
                    let occupied = this
                        .worktrees
                        .iter()
                        .any(|tree| tree.branch.as_deref() == Some(name.as_str()));
                    let owner = owner.clone();
                    let path = path.clone();
                    // A branch another worktree has checked out cannot be
                    // switched to here, and its row says so beside its name,
                    // "Another worktree", which no write's end changes; the
                    // menu keys rest on it, so its reason is heard
                    // (`native_accessibility::detailed_item`). Drawn as
                    // "<name> · another worktree", one disabled label, it
                    // was passed over, and a screen reader never reached it.
                    // Its name adds the way to that branch: the worktree
                    // that has it, which Worktrees… opens.
                    let item = if occupied {
                        native_accessibility::detailed_item(name.clone(), "Another worktree")
                            .accessibility_label(format!(
                                "{name}, another worktree. {OCCUPIED_REMEDY}"
                            ))
                    } else {
                        native_accessibility::waiting_item(name.clone(), false, wait)
                    };
                    // Choices beside the ticked current branch, announced as
                    // unticked check items.
                    menu = picker_item(
                        menu,
                        item.checked(false).on_click(move |_, window, cx| {
                            let _ = owner.update(cx, |this, cx| {
                                if this.path == path && this.page == AppPage::Repository {
                                    let origin =
                                        workspace::confirmations::picker(&this.path, &name);
                                    this.write_from(
                                        WriteCommand::Checkout {
                                            branch: name.clone(),
                                        },
                                        "Switching branch…",
                                        Some(origin.into()),
                                        window,
                                        cx,
                                    );
                                }
                            });
                        }),
                    );
                }
                if candidates.len() > CHOICE_LIMIT {
                    menu = menu.label("Showing 40 branches · Find to narrow the list");
                }
                if candidates.is_empty() {
                    menu = menu.label("No other matching local branches");
                }
                menu = menu.separator();
                if current.is_some() {
                    menu = add_menu_action(
                        menu,
                        "Current branch settings…",
                        MenuAction::ManageCurrent,
                        &owner,
                        &path,
                        wait,
                    );
                }
                menu = add_menu_action(
                    menu,
                    "Manage another branch…",
                    MenuAction::ManageLocal,
                    &owner,
                    &path,
                    wait,
                );
                menu = add_menu_action(
                    menu,
                    "Create from a remote branch…",
                    MenuAction::Track,
                    &owner,
                    &path,
                    wait,
                );
                if current.is_some() && this.integration_state.is_none() {
                    menu = add_menu_action(
                        menu.separator(),
                        "Merge a branch…",
                        MenuAction::Merge,
                        &owner,
                        &path,
                        wait,
                    );
                    menu = add_menu_action(
                        menu,
                        "Rebase onto a branch…",
                        MenuAction::Rebase,
                        &owner,
                        &path,
                        wait,
                    );
                    menu = add_menu_action(
                        menu,
                        "Edit local commits…",
                        MenuAction::InteractiveRebase,
                        &owner,
                        &path,
                        wait,
                    );
                }
                menu = add_menu_action(
                    menu.separator(),
                    "Tags…",
                    MenuAction::Tags,
                    &owner,
                    &path,
                    wait,
                );
                menu = add_menu_action(
                    menu,
                    "GitTurtle activity…",
                    MenuAction::Activity,
                    &owner,
                    &path,
                    wait,
                );
                menu = add_menu_action(
                    menu,
                    "Compare revisions…",
                    MenuAction::CompareRevisions,
                    &owner,
                    &path,
                    wait,
                );
                menu = add_menu_action(
                    menu,
                    "Quick Open File…",
                    MenuAction::QuickFile,
                    &owner,
                    &path,
                    wait,
                );
                menu = add_menu_action(
                    menu,
                    "Worktrees…",
                    MenuAction::Worktrees,
                    &owner,
                    &path,
                    wait,
                );
                menu = add_menu_action(
                    menu,
                    "Local reflog & recovery…",
                    MenuAction::Reflog,
                    &owner,
                    &path,
                    wait,
                );
                add_menu_action(
                    menu,
                    "Manage remotes…",
                    MenuAction::Remotes,
                    &owner,
                    &path,
                    wait,
                )
            })
            // A write's start or end builds the open menu again.
            .follows_writes(cx.entity().downgrade())
            .into_any_element()
    }

    fn branch_menu_action(
        &mut self,
        action: MenuAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            MenuAction::ManageCurrent => {
                if let Some(name) = self
                    .work_status
                    .as_ref()
                    .and_then(|status| status.branch.clone())
                {
                    self.open_contextual_branch(name, false, window, cx);
                }
            }
            MenuAction::ManageLocal => self.choose_branch(ChoicePurpose::Manage, window, cx),
            MenuAction::Track => self.choose_branch(ChoicePurpose::Track, window, cx),
            MenuAction::Merge => {
                self.choose_branch(ChoicePurpose::Integrate { rebase: false }, window, cx)
            }
            MenuAction::Rebase => {
                self.choose_branch(ChoicePurpose::Integrate { rebase: true }, window, cx)
            }
            MenuAction::InteractiveRebase => self.open_interactive_rebase(None, window, cx),
            MenuAction::Remotes => self.open_remote_manager(window, cx),
            MenuAction::Tags => self.open_tags(window, cx),
            MenuAction::Activity => self.open_activity(window, cx),
            MenuAction::CompareRevisions => self.open_revision_comparison(window, cx),
            MenuAction::QuickFile => self.open_quick_file(window, cx),
            MenuAction::Worktrees => self.open_worktree_manager(window, cx),
            MenuAction::Reflog => {
                self.open_reflog_browser(window, cx);
            }
        }
    }

    pub(super) fn open_contextual_branch(
        &mut self,
        name: String,
        remote: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.read_branch_action(
            move |repo| {
                if remote {
                    let branch = repo
                        .branches()?
                        .into_iter()
                        .find(|branch| branch.remote && branch.name == name)
                        .ok_or_else(|| {
                            anyhow::anyhow!("This remote branch changed; refresh the branch list.")
                        })?;
                    Ok(Prepared::Remote {
                        reference: format!("refs/remotes/{}", branch.name),
                        oid: branch.oid,
                    })
                } else {
                    Ok(Prepared::Local(Arc::new(repo.branch_plan(&name)?)))
                }
            },
            window,
            cx,
        );
    }

    fn choose_branch(
        &mut self,
        purpose: ChoicePurpose,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.read_branch_action(
            move |repo| {
                let current = repo.status()?.branch;
                let choices = repo
                    .branches()?
                    .into_iter()
                    .filter(|branch| match &purpose {
                        ChoicePurpose::Track => branch.remote,
                        ChoicePurpose::Manage => !branch.remote,
                        ChoicePurpose::Integrate { .. } => {
                            branch.remote || Some(&branch.name) != current.as_ref()
                        }
                        ChoicePurpose::Upstream(selected) => {
                            branch.remote || branch.name != selected.name
                        }
                    })
                    .map(|branch| BranchChoice {
                        reference: format!(
                            "refs/{}/{}",
                            if branch.remote { "remotes" } else { "heads" },
                            branch.name
                        ),
                        search: branch.name.to_lowercase(),
                        name: branch.name,
                        oid: branch.oid,
                        remote: branch.remote,
                    })
                    .collect();
                Ok(Prepared::Choices {
                    purpose,
                    choices: Arc::new(choices),
                })
            },
            window,
            cx,
        );
    }

    /// Opens the remote manager, or with `remotes` false the branch
    /// manager's chooser, as the Branch menu does, and hands back the read
    /// that fills its list, for tests outside this module to await.
    #[cfg(test)]
    pub(crate) fn open_manager(
        &mut self,
        remotes: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Task<()>> {
        if remotes {
            self.open_remote_manager(window, cx);
        } else {
            self.choose_branch(ChoicePurpose::Manage, window, cx);
        }
        self.branch_actions.task.take()
    }

    fn open_remote_manager(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.read_branch_action(
            |repo| {
                Ok(Prepared::Remotes(Arc::new(
                    repo.remote_configs()?
                        .into_iter()
                        .map(|config| RemoteChoice {
                            search: config.name.to_lowercase(),
                            config: Arc::new(config),
                        })
                        .collect(),
                )))
            },
            window,
            cx,
        );
    }

    fn read_branch_action(
        &mut self,
        prepare: impl FnOnce(GitRepository) -> anyhow::Result<Prepared> + Send + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.read_branch_action_recovering(prepare, None, window, cx)
    }

    fn read_branch_action_recovering(
        &mut self,
        prepare: impl FnOnce(GitRepository) -> anyhow::Result<Prepared> + Send + 'static,
        on_failure: Option<PreparationFailure>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.operation_busy.is_some() || self.page != AppPage::Repository {
            return false;
        }
        let Some(repo) = self.repository.clone() else {
            return false;
        };
        let path = repo.path().to_owned();
        self.cancel_branch_action();
        let generation = self.branch_actions.generation;
        self.branch_actions.asked_in_changes = self.mode == WorkspaceMode::Working;
        self.operation_busy = Some(PREPARING);
        self.operation_error = None;
        let response = self.operations.submit_read(move || prepare(repo));
        self.branch_actions.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = response.await.unwrap_or_else(|_| {
                Err(anyhow::anyhow!(
                    "Branch preparation was interrupted. Open the action again."
                ))
            });
            let _ = this.update_in(cx, |this, window, cx| {
                if this.branch_actions.generation != generation {
                    return;
                }
                // Ended, it no longer runs: Changes opened after it drops no
                // plan (`dismiss_branch_action`), as a recovery's end does.
                this.branch_actions.task = None;
                if this.operation_busy == Some(PREPARING) {
                    this.operation_busy = None;
                }
                this.read_deferred_working(window, cx);
                if this.path.as_ref() != Some(&path)
                    || this.page != AppPage::Repository
                    || this.branch_actions.dismissed
                {
                    cx.notify();
                    return;
                }
                match result {
                    Ok(prepared) => this.show_prepared_branch_action(prepared, window, cx),
                    Err(error) => {
                        let message = format!("{error:#}");
                        if let Some(on_failure) = on_failure {
                            on_failure(message, window, cx);
                        } else {
                            this.operation_error = Some(message);
                        }
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
        true
    }

    fn show_prepared_branch_action(
        &mut self,
        prepared: Prepared,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match prepared {
            Prepared::Confirmation {
                title,
                explanation,
                action,
                command,
            } => self.confirm_git_write(title, explanation, action, command, window, cx),
            Prepared::Local(plan) => self.show_local_branch(plan, window, cx),
            Prepared::Remote { reference, oid } => {
                self.show_remote_branch(reference, oid, window, cx)
            }
            Prepared::Choices { purpose, choices } => {
                let owner = cx.entity().downgrade();
                let path = self.path.clone();
                let title = purpose.title();
                let chooser =
                    cx.new(|cx| BranchChooser::new(owner, path, purpose, choices, window, cx));
                #[cfg(test)]
                {
                    self.branch_actions.list = Some(chooser.read(cx).list.clone());
                }
                window.open_alert_dialog(cx, move |dialog, window, cx| {
                    let chooser_ok = chooser.clone();
                    let shown = chooser.clone();
                    // The chooser lies in the dialog's content rather than
                    // its scrolling body, whose scroller lays what it holds
                    // out at full height: its list takes what the dialog's
                    // cap leaves it, so a row the list reveals is shown. The
                    // dialog clips its content, and the chooser keeps the
                    // focus ring's room inside it below its last control.
                    // The footer's gap gives the room back, so the footer
                    // stays put.
                    dialog
                        .title(title.clone())
                        .width(appearance::dialog_width(px(540.), window))
                        .max_h(appearance::dialog_max_height(window))
                        .gap(crate::tags::DIALOG_FOOTER_GAP - appearance::button_ring_room(cx))
                        .content(move |content, _, _| content.min_h_0().child(shown.clone()))
                        .footer(crate::tags::dialog_footer(
                            "Use first match",
                            Some("Cancel"),
                        ))
                        .on_ok(move |_, window, cx| {
                            chooser_ok.update(cx, |chooser, cx| chooser.activate_first(window, cx));
                            false
                        })
                });
            }
            Prepared::Remotes(remotes) => {
                let owner = cx.entity().downgrade();
                let path = self.path.clone();
                let manager = cx.new(|cx| RemoteManager::new(owner, path, remotes, window, cx));
                #[cfg(test)]
                {
                    self.branch_actions.list = Some(manager.read(cx).list.clone());
                }
                window.open_alert_dialog(cx, move |dialog, window, _| {
                    dialog
                        .title("Remotes")
                        .width(appearance::dialog_width(px(580.), window))
                        .max_h(appearance::dialog_max_height(window))
                        .child(manager.clone())
                        .footer(crate::tags::dialog_footer("Done", None))
                });
            }
        }
    }

    fn show_local_branch(
        &mut self,
        plan: Arc<BranchPlan>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let owner = cx.entity().downgrade();
        let path = self.path.clone();
        let current = plan.current_branch.as_deref() == Some(plan.name.as_str());
        let can_delete = plan.checked_out_in.is_empty() && plan.unmerged_commits == Some(0);
        let own_path = self.path.as_deref();
        let other_worktree = plan
            .checked_out_in
            .iter()
            .any(|tree| Some(tree.as_path()) != own_path);
        // Why each action that changes the branch is unavailable, and the
        // way to it: a branch another worktree has checked out is renamed
        // and tracked from there.
        let elsewhere = |change: &str| {
            other_worktree.then(|| format!("{ELSEWHERE} {change} from that worktree."))
        };
        let rename_refused = elsewhere("Rename it");
        let upstream_refused = elsewhere("Change its upstream");
        // With none set there is nothing to remove from any worktree.
        let clear_refused = plan
            .upstream
            .is_none()
            .then(|| "No upstream is set. Change upstream… sets one.".to_owned())
            .or_else(|| elsewhere("Remove its upstream"));
        let delete_refused = if !plan.checked_out_in.is_empty() {
            Some(format!(
                "Checked out in {}. Switch that worktree to another branch before deleting.",
                path_list(&plan.checked_out_in)
            ))
        } else if let Some(count) = plan.unmerged_commits.filter(|count| *count > 0) {
            Some(format!(
                "{} outside {}. Merge or preserve that work before deleting this branch.",
                crate::counted(count, "commit is", "commits are"),
                plan.merge_target
                    .as_ref()
                    .map_or("HEAD", |target| short_reference(&target.name))
            ))
        } else if can_delete {
            None
        } else {
            Some("No retained target is available to verify safe deletion. Create or select a branch that contains these commits first.".to_owned())
        };
        window.open_alert_dialog(cx, move |dialog, window, cx| {
            let p = palette(cx);
            let rename = Arc::clone(&plan);
            let upstream = Arc::clone(&plan);
            let clear_upstream = Arc::clone(&plan);
            let delete = Arc::clone(&plan);
            let source = format!("refs/heads/{}", plan.name);
            let mut actions = div().flex().flex_wrap().gap_2()
                .child(dialog_action("rename-branch", "Rename…", &owner, &path, rename_refused.clone(), move |this, window, cx| this.open_branch_form(BranchFormMode::Rename(Arc::clone(&rename)), window, cx)))
                .child(dialog_action("choose-upstream", "Change upstream…", &owner, &path, upstream_refused.clone(), move |this, window, cx| this.choose_branch(ChoicePurpose::Upstream(Arc::clone(&upstream)), window, cx)))
                .child(dialog_action("remove-upstream", "Remove upstream…", &owner, &path, clear_refused.clone(), move |this, window, cx| this.prepare_upstream(Arc::clone(&clear_upstream), None, window, cx)))
                .child(dialog_action("delete-branch", "Delete branch…", &owner, &path, delete_refused.clone(), move |this, window, cx| this.prepare_branch_delete(Arc::clone(&delete), window, cx)));
            if !current && plan.current_branch.is_some() {
                let merge_source = source.clone();
                actions = actions.child(dialog_action("merge-selected-branch", "Merge into current…", &owner, &path, None, move |this, window, cx| this.prepare_integration(merge_source.clone(), false, window, cx)))
                    .child(dialog_action("rebase-selected-branch", "Rebase current onto this…", &owner, &path, None, move |this, window, cx| this.prepare_integration(source.clone(), true, window, cx)));
            }
            let deletion_note = if let Some(reason) = &delete_refused {
                format!("{reason}{}", if other_worktree { " Rename and upstream changes belong in that worktree." } else { "" })
            } else if let Some(target) = &plan.merge_target { format!("All commits are retained by {}.", short_reference(&target.name)) } else { "No retained target is available to verify safe deletion. Create or select a branch that contains these commits first.".into() };
            let metadata = format!("{}{} · {}", if current { "Current branch · " } else { "" }, short_oid(&plan.oid), plan.upstream.as_deref().map_or("No upstream".into(), |upstream| format!("Tracks {}", short_reference(upstream))));
            let content = div().flex().flex_col().gap_3()
                .child(div().text_size(crate::appearance::ui_text(12.)).text_color(rgb(p.muted)).child(metadata))
                .child(div().text_size(crate::appearance::ui_text(12.)).child(deletion_note))
                .child(actions);
            dialog.title(plan.name.clone()).width(appearance::dialog_width(px(560.), window)).max_h(appearance::dialog_max_height(window)).child(content)
                .footer(crate::tags::dialog_footer("Done", None))
        });
    }

    fn show_remote_branch(
        &mut self,
        reference: String,
        oid: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let owner = cx.entity().downgrade();
        let path = self.path.clone();
        // Merging or rebasing needs a branch to change: detached, each says
        // so and how to get one.
        let detached = self
            .work_status
            .as_ref()
            .and_then(|status| status.branch.as_ref())
            .is_none()
            .then(|| DETACHED.to_owned());
        window.open_alert_dialog(cx, move |dialog, window, cx| {
            let p = palette(cx);
            let tracking_ref = reference.clone();
            let merge_ref = reference.clone();
            let rebase_ref = reference.clone();
            let actions = div()
                .flex()
                .flex_wrap()
                .gap_2()
                .child(dialog_action(
                    "track-remote-branch",
                    "Create local tracking branch…",
                    &owner,
                    &path,
                    None,
                    move |this, window, cx| {
                        this.open_branch_form(
                            BranchFormMode::Track(tracking_ref.clone()),
                            window,
                            cx,
                        )
                    },
                ))
                .child(dialog_action(
                    "merge-remote-branch",
                    "Merge into current…",
                    &owner,
                    &path,
                    detached.clone(),
                    move |this, window, cx| {
                        this.prepare_integration(merge_ref.clone(), false, window, cx)
                    },
                ))
                .child(dialog_action(
                    "rebase-remote-branch",
                    "Rebase current onto this…",
                    &owner,
                    &path,
                    detached.clone(),
                    move |this, window, cx| {
                        this.prepare_integration(rebase_ref.clone(), true, window, cx)
                    },
                ));
            let content = div()
                .flex()
                .flex_col()
                .gap_3()
                .child(
                    div()
                        .text_size(crate::appearance::ui_text(12.))
                        .text_color(rgb(p.muted))
                        .child(format!(
                            "Locally available remote branch · {}",
                            short_oid(&oid)
                        )),
                )
                .child(actions);
            dialog
                .title(short_reference(&reference).to_owned())
                .width(appearance::dialog_width(px(540.), window))
                .max_h(appearance::dialog_max_height(window))
                .child(content)
                .footer(crate::tags::dialog_footer("Done", None))
        });
    }

    fn open_branch_form(
        &mut self,
        mode: BranchFormMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let owner = cx.entity().downgrade();
        let path = self.path.clone();
        let initial = match &mode {
            BranchFormMode::Rename(plan) => plan.name.clone(),
            BranchFormMode::Track(reference) => {
                let branch = reference.strip_prefix("refs/remotes/").unwrap_or(reference);
                self.remotes
                    .iter()
                    .filter_map(|remote| {
                        branch
                            .strip_prefix(&format!("{}/", remote.name))
                            .map(|name| (remote.name.len(), name))
                    })
                    .max_by_key(|(length, _)| *length)
                    .map(|(_, name)| name.to_owned())
                    .unwrap_or_else(|| {
                        branch
                            .split_once('/')
                            .map_or(branch, |(_, name)| name)
                            .to_owned()
                    })
            }
        };
        let form = cx.new(|cx| BranchForm::new(owner, path, mode, initial, window, cx));
        show_branch_form(form, window, cx);
    }

    fn prepare_branch_delete(
        &mut self,
        expected: Arc<BranchPlan>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.read_branch_action(move |repo| {
            let plan = repo.branch_plan(&expected.name)?;
            anyhow::ensure!(plan == *expected, "This branch changed; open its actions again.");
            anyhow::ensure!(plan.checked_out_in.is_empty() && plan.unmerged_commits == Some(0), "Switch away from this branch and preserve its unmerged commits before deleting it.");
            let target = plan.merge_target.as_ref().map_or("HEAD", |target| short_reference(&target.name));
            Ok(Prepared::Confirmation { title: format!("Delete local branch {}", plan.name),
                explanation: format!("Delete the local branch label {} at {}. Its commits are retained by {target}. Working files and remote branches remain available.", plan.name, short_oid(&plan.oid)),
                action: "Delete branch", command: WriteCommand::Branch(BranchCommand::Delete { plan }.into()) })
        }, window, cx);
    }

    fn prepare_upstream(
        &mut self,
        expected: Arc<BranchPlan>,
        upstream: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.read_branch_action(move |repo| {
            let plan = repo.upstream_plan(&expected.name, upstream.as_deref())?;
            anyhow::ensure!(plan.branch == *expected, "This branch changed; review its tracking relationship again.");
            let (title, explanation, action) = if let Some(reference) = &plan.upstream_ref {
                (format!("Set upstream for {}", plan.branch.name), format!("Track {} from local branch {}. Ahead/behind counts and future pull defaults will use this relationship. This updates local configuration and does not fetch or push.", short_reference(reference), plan.branch.name), "Set upstream")
            } else {
                (format!("Remove upstream from {}", plan.branch.name), format!("Remove {} as the upstream for {}. Future pulls need an explicit destination. The branch and its commits stay in place.", plan.branch.upstream.as_deref().map_or("the current target", short_reference), plan.branch.name), "Remove upstream")
            };
            Ok(Prepared::Confirmation { title, explanation, action, command: WriteCommand::Branch(BranchCommand::SetUpstream { plan }.into()) })
        }, window, cx);
    }

    pub(super) fn open_remote_form(
        &mut self,
        expected: Option<Arc<RemoteConfig>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let owner = cx.entity().downgrade();
        let path = self.path.clone();
        let form = cx.new(|cx| RemoteForm::new(owner, path, expected, window, cx));
        show_remote_form(form, window, cx);
    }

    fn prepare_remote_remove(
        &mut self,
        expected: Arc<RemoteConfig>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.read_branch_action(move |repo| {
            let current = repo.remote_configs()?.into_iter().find(|remote| remote.name == expected.name)
                .ok_or_else(|| anyhow::anyhow!("This remote no longer exists; refresh the remote list."))?;
            anyhow::ensure!(current == *expected, "The remote or its tracking references changed; review it again.");
            let mut explanation = format!("Remove local configuration for {}. The remote server is unchanged.\n\n{} will be disconnected; {} will be removed. Local branches and working files stay in place.", current.name, crate::counted(current.upstream_branches.len(), "upstream relationship", "upstream relationships"), crate::counted(current.tracking_refs.len(), "unshared remote-tracking reference", "unshared remote-tracking references"));
            append_names(&mut explanation, "Upstream branches", current.upstream_branches.iter().map(String::as_str));
            append_names(&mut explanation, "Remote-tracking references", current.tracking_refs.iter().map(|reference| short_reference(&reference.name)));
            Ok(Prepared::Confirmation { title: format!("Remove remote {}", current.name), explanation, action: "Remove remote", command: WriteCommand::Branch(BranchCommand::RemoveRemote { expected: current }.into()) })
        }, window, cx);
    }
}

/// The branch picker's `action`, refused while `wait` holds why, a write
/// running or its reload yet to land as the menu was built: each action but
/// Activity refuses until then, saying why in the room its command leaves
/// (`native_accessibility::waiting_item`).
fn add_menu_action(
    mut menu: PopupMenu,
    label: &'static str,
    action: MenuAction,
    owner: &WeakEntity<GitTurtle>,
    path: &Option<PathBuf>,
    wait: Option<native_accessibility::MenuWait>,
) -> PopupMenu {
    let owner = owner.clone();
    let path = path.clone();
    let wait = wait.filter(|_| !matches!(action, MenuAction::Activity));
    menu = picker_item(
        menu,
        native_accessibility::waiting_item(label, false, wait).on_click(move |_, window, cx| {
            let _ = owner.update(cx, |this, cx| {
                if this.path == path && this.page == AppPage::Repository {
                    this.branch_menu_action(action, window, cx);
                }
            });
        }),
    );
    menu
}

/// Why a branch dialog's action that changes the branch is unavailable for
/// a branch another worktree has checked out, before the way to it.
const ELSEWHERE: &str = "Checked out in another worktree.";

/// Why a remote branch's Merge into current… and Rebase current onto this…
/// are unavailable while HEAD is detached, and the way to them.
const DETACHED: &str = "HEAD is detached. Switch to a branch first.";

/// What a branch another worktree has checked out names as the way to it
/// in the branch picker's menu, after its reason: a branch is checked out
/// in one worktree at a time.
const OCCUPIED_REMEDY: &str = "Open that worktree from Worktrees…";

/// Adds `item` to the branch picker's `menu`, noting in a test build what
/// it says and whether it is refused with a reason ([`PICKER_ITEMS`]).
fn picker_item(menu: PopupMenu, item: PopupMenuItem) -> PopupMenu {
    #[cfg(test)]
    if let Some(noted) = PickerItem::of(&item) {
        PICKER_ITEMS.with(|items| items.borrow_mut().push(noted));
    }
    menu.item(item)
}

#[cfg(test)]
impl PickerItem {
    /// What a test reads of `item`, a command, as the branch picker's and
    /// the Git profile picker's menus note theirs; `None` for a label or a
    /// separator.
    pub(crate) fn of(item: &PopupMenuItem) -> Option<Self> {
        let (label, name, disabled, reason_stated, checked) = match item {
            PopupMenuItem::Item {
                label,
                disabled,
                checked,
                checkable,
                accessibility_label,
                reason_stated,
                ..
            } => (
                label.to_string(),
                accessibility_label,
                *disabled,
                *reason_stated,
                checkable.then_some(*checked),
            ),
            PopupMenuItem::ElementItem {
                name,
                disabled,
                checked,
                checkable,
                accessibility_label,
                reason_stated,
                ..
            } => (
                name.as_ref().map(ToString::to_string).unwrap_or_default(),
                accessibility_label,
                *disabled,
                *reason_stated,
                checkable.then_some(*checked),
            ),
            _ => return None,
        };
        Some(Self {
            label,
            name: name.as_ref().map(ToString::to_string),
            disabled,
            reason_stated,
            checked,
        })
    }
}

/// A branch dialog's action `label`, unavailable while `refused` holds why,
/// which it then says, with the way to make it available where there is
/// one, in its tooltip and its accessible description, as New theme… does
/// at its bound: "Checked out in another worktree. Rename it from that
/// worktree." Refused with nothing to say why, Rename… and Delete branch…
/// once dimmed beside a note that named only the deletion's reason. The
/// tooltip is the kit's text tooltip, drawn as `‹id›/tooltip/‹reason›` so a
/// test reads the one shown under the pointer (`workspace::waiting_tooltip`).
fn dialog_action(
    id: &'static str,
    label: &'static str,
    owner: &WeakEntity<GitTurtle>,
    path: &Option<PathBuf>,
    refused: Option<String>,
    action: impl Fn(&mut GitTurtle, &mut Window, &mut Context<GitTurtle>) + 'static,
) -> impl IntoElement {
    use native_accessibility::DrawnButton as _;
    let owner = owner.clone();
    let path = path.clone();
    button(id, label, "", false)
        .debug_selector(move || id.into())
        .secondary()
        .disabled(refused.is_some())
        .when_some(refused, |action, reason| {
            let action = Described(action).aria_description(reason.clone()).0;
            workspace::waiting_tooltip(action, id, reason)
        })
        .on_click(move |_, window, cx| {
            let _ = owner.update(cx, |this, cx| {
                if this.path == path
                    && this.page == AppPage::Repository
                    && this.operation_busy.is_none()
                {
                    window.close_dialog(cx);
                    action(this, window, cx);
                }
            });
        })
        .node_recorded()
}

/// A kit Button lent GPUI's stateful element methods for its accessible
/// description, which the kit's Button does not offer: its interactivity is
/// its base Button's, whose node GPUI writes with the description.
struct Described(Button);

impl InteractiveElement for Described {
    fn interactivity(&mut self) -> &mut gpui::Interactivity {
        self.0.interactivity()
    }
}

impl StatefulInteractiveElement for Described {}

fn short_reference(reference: &str) -> &str {
    reference
        .strip_prefix("refs/heads/")
        .or_else(|| reference.strip_prefix("refs/remotes/"))
        .unwrap_or(reference)
}

fn path_list(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

fn append_names<'a>(text: &mut String, label: &str, names: impl Iterator<Item = &'a str>) {
    let names: Vec<_> = names.collect();
    if names.is_empty() {
        return;
    }
    text.push_str(&format!(
        "\n\n{label}:\n{}",
        names
            .iter()
            .take(12)
            .copied()
            .collect::<Vec<_>>()
            .join("\n")
    ));
    if names.len() > 12 {
        text.push_str(&format!("\n…and {} more", names.len() - 12));
    }
}

fn field_label(label: &'static str, cx: &App) -> AnyElement {
    div()
        .text_size(crate::appearance::ui_text(12.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(rgb(palette(cx).text))
        .child(label)
        .into_any_element()
}

fn show_branch_form(form: Entity<BranchForm>, window: &mut Window, cx: &mut App) {
    form.update(cx, |form, _| form.opened += 1);
    let title = match &form.read(cx).mode {
        BranchFormMode::Rename(_) => "Rename branch",
        BranchFormMode::Track(_) => "Create tracking branch",
    };
    window.open_alert_dialog(cx, move |dialog, window, _| {
        let submit = form.clone();
        dialog
            .title(title)
            .width(appearance::dialog_width(px(520.), window))
            .max_h(appearance::dialog_max_height(window))
            .child(form.clone())
            .footer(crate::tags::dialog_footer("Review action", Some("Cancel")))
            .on_ok(move |_, window, cx| submit.update(cx, |form, cx| form.submit(window, cx)))
    });
}

fn show_remote_form(form: Entity<RemoteForm>, window: &mut Window, cx: &mut App) {
    form.update(cx, |form, _| form.opened += 1);
    let title = form
        .read(cx)
        .expected
        .as_ref()
        .map_or("Add remote".into(), |remote| {
            format!("Edit {}", remote.name)
        });
    window.open_alert_dialog(cx, move |dialog, window, cx| {
        let submit = form.clone();
        let shown = form.clone();
        // In the dialog's content, as the profile form is: the form takes
        // what the cap leaves it and scrolls the rest, with the ring's room
        // below its last control, which the footer's gap gives back.
        dialog
            .title(title.clone())
            .width(appearance::dialog_width(px(580.), window))
            .max_h(appearance::dialog_max_height(window))
            .gap(crate::tags::DIALOG_FOOTER_GAP - appearance::button_ring_room(cx))
            .content(move |content, _, _| content.min_h_0().child(shown.clone()))
            .footer(crate::tags::dialog_footer(
                "Review settings",
                Some("Cancel"),
            ))
            .on_ok(move |_, window, cx| submit.update(cx, |form, cx| form.submit(window, cx)))
    });
}

struct BranchChooser {
    owner: WeakEntity<GitTurtle>,
    path: Option<PathBuf>,
    purpose: ChoicePurpose,
    choices: Arc<Vec<BranchChoice>>,
    query: Entity<InputState>,
    /// The list's scroll, which reveals the row Tab moves onto.
    list: FocusReveal,
    /// The content's scroll, which shows the list when a row takes focus.
    content: FocusReveal,
    _subscription: Subscription,
}

impl BranchChooser {
    fn new(
        owner: WeakEntity<GitTurtle>,
        path: Option<PathBuf>,
        purpose: ChoicePurpose,
        choices: Arc<Vec<BranchChoice>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let query = cx.new(|cx| InputState::new(window, cx).placeholder("Find a branch"));
        let subscription =
            cx.subscribe_in(&query, window, |this, _, event, window, cx| match event {
                InputEvent::Change => {
                    this.list.scroll().set_offset(Point::default());
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => this.activate_first(window, cx),
                _ => {}
            });
        Self {
            owner,
            path,
            purpose,
            choices,
            query,
            list: FocusReveal::new(Trigger::Keyboard),
            content: FocusReveal::new(Trigger::Keyboard),
            _subscription: subscription,
        }
    }

    fn matching(&self, cx: &App) -> Vec<usize> {
        let query = self.query.read(cx).value().trim().to_lowercase();
        self.choices
            .iter()
            .enumerate()
            .filter(|(_, choice)| query.is_empty() || choice.search.contains(&query))
            .take(CHOICE_LIMIT + 1)
            .map(|(index, _)| index)
            .collect()
    }

    fn activate_first(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self.matching(cx).first().copied() {
            self.activate(index, window, cx);
        }
    }

    fn activate(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(choice) = self.choices.get(index) else {
            return;
        };
        let reference = choice.reference.clone();
        let name = choice.name.clone();
        let purpose = self.purpose.clone();
        let _ = self.owner.update(cx, |this, cx| {
            if this.path != self.path
                || this.page != AppPage::Repository
                || this.operation_busy.is_some()
            {
                return;
            }
            window.close_dialog(cx);
            match purpose {
                ChoicePurpose::Track => {
                    this.open_branch_form(BranchFormMode::Track(reference), window, cx)
                }
                ChoicePurpose::Manage => this.open_contextual_branch(name, false, window, cx),
                ChoicePurpose::Integrate { rebase } => {
                    this.prepare_integration(reference, rebase, window, cx)
                }
                ChoicePurpose::Upstream(plan) => {
                    this.prepare_upstream(plan, Some(reference), window, cx)
                }
            }
        });
    }
}

impl Render for BranchChooser {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // A row focus has moved onto scrolls into view in this frame.
        self.list.reveal(window, cx);
        self.content.reveal(window, cx);
        let p = palette(cx);
        let matching = self.matching(cx);
        let detail = match &self.purpose {
            ChoicePurpose::Track => {
                "Choose a locally available remote branch. A new local branch will track it."
            }
            ChoicePurpose::Manage => {
                "Choose a branch to rename, delete safely, or configure its upstream."
            }
            ChoicePurpose::Integrate { rebase: false } => {
                "The selected branch will be merged into the current branch. Review both tips before proceeding."
            }
            ChoicePurpose::Integrate { rebase: true } => {
                "The current branch's commits will be replayed onto the selected branch. Review the history change before proceeding."
            }
            ChoicePurpose::Upstream(_) => {
                "Choose the local or remote branch used for tracking and ahead/behind counts."
            }
        };
        // The list scrolls, so it clips its rows to its bounds on both axes:
        // it keeps the focus ring's room around them, scrolled to either end,
        // and gives it back through its margin, so no row moves. The chooser
        // keeps the room below its last control inside the dialog's clip of
        // its body, and the dialog gives it back. A row Tab moves onto
        // scrolls into view with its ring.
        let room = appearance::button_ring_room(cx);
        let rows: Vec<_> = matching
            .iter()
            .take(CHOICE_LIMIT)
            .map(|&index| (index, self.list.focus(("branch-choice", index), cx)))
            .collect();
        let reveal = self
            .list
            .items(rows.iter().map(|(_, focus)| focus.clone()).collect(), room);
        // Past its height the list cues the rest with its scrollbar. It
        // shrinks to what the dialog's cap leaves it, though no shorter than
        // two whole rows, or the one there is, and keeps its whole
        // explanation when nothing matches: in a 461 × 490 half tile, laid
        // out at full height in the dialog's scrolling body, it ran below
        // the dialog's fold, where a row Tab moved onto took focus unseen.
        let shown = matching.len().min(CHOICE_LIMIT).clamp(1, 2) as f32;
        let floor = appearance::ui_size(34.) * shown
            + rems(0.25).to_pixels(window.rem_size()) * (shown - 1.);
        let cue = tags::ScrollCue::revealing(&self.list, room);
        // Where the detail, the filter and the list's floor exceed what the
        // cap leaves, as at 18 pt in a 461 × 400 window, the chooser's
        // content scrolls, cued by its scrollbar beside it in the dialog's
        // side padding as Worktrees' is, and a row Tab moves onto brings the
        // whole list into view, so the row the list reveals stays whole
        // (the dialog once clipped the list's foot there, and the row with
        // it). It keeps the focus ring's room at its sides, given back
        // through its margin, and below its last control.
        let query = self.query.read(cx).focus_handle(cx);
        let show_list = self
            .content
            .group(rows.iter().map(|(_, focus)| focus.clone()).collect(), room);
        let content = div().on_children_prepainted(self.content.observe()).id("branch-chooser-content").debug_selector(|| "branch-chooser-content".into()).flex().flex_col().gap_3().min_h_0().overflow_y_scroll().track_scroll(self.content.scroll()).px(room).mx(-room).pb(room)
            .child(div().debug_selector(|| "branch-chooser-detail".into()).text_size(crate::appearance::ui_text(12.)).text_color(rgb(p.muted)).child(detail))
            .child(div().on_children_prepainted(self.content.control(query, room)).py(room).my(-room).child(Input::new(&self.query).cleanable(true).prefix(Icon::default().path("icons/search.svg").size(px(14.)))))
            .child(div().on_children_prepainted(show_list).flex().flex_col().min_h_0().map(|entries| if matching.is_empty() { entries.flex_shrink_0() } else { entries.min_h(floor) }).child(cue.list(div().on_children_prepainted(cue.observe(reveal)).id("branch-chooser-list").debug_selector(|| "branch-chooser-list".into()).max_h(px(330.) + room * 2.).min_h_0().p(room).m(-room).overflow_y_scroll().track_scroll(self.list.scroll()).flex().flex_col().gap_1()
                .children(rows.into_iter().map(|(index, focus)| {
                    let choice = &self.choices[index];
                    Button::new(("branch-choice", index)).track_focus(&focus).debug_selector(move || format!("branch-choice-{index}")).ghost().w_full().h(crate::appearance::ui_size(34.))
                        .text_size(crate::appearance::ui_text(12.)).accessibility_label(choice.name.clone())
                        .child(div().w_full().min_w_0().flex().items_center().justify_start().gap_2()
                            .child(Icon::default().path(if choice.remote { "icons/remote.svg" } else { "icons/branch.svg" }).size(px(14.)))
                            .child(div().flex_1().min_w_0().truncate().child(choice.name.clone())))
                        .tooltip(format!("{} · {}", choice.reference, short_oid(&choice.oid)))
                        .on_click(cx.listener(move |this, _, window, cx| this.activate(index, window, cx)))
                }))
                .when(matching.is_empty(), |element| element.child(div().p_3().text_size(crate::appearance::ui_text(12.)).text_color(rgb(p.muted)).child(if self.choices.is_empty() { "No branches are available for this action. Fetch explicitly to update remote branches." } else { "No branches match this search." }))), "branch-chooser-scrollbar").min_h_0()))
            .when(matching.len() > CHOICE_LIMIT, |element| element.child(div().text_size(crate::appearance::ui_text(11.)).text_color(rgb(p.muted)).child("Showing 40 matches. Narrow the search to find another branch.")));
        div()
            .relative()
            .min_h_0()
            .flex()
            .flex_col()
            .child(crate::native_accessibility::clips_children(content))
            .child(
                div()
                    .debug_selector(|| "branch-chooser-content-scrollbar".into())
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .right(-gpui_kit::component::scroll::Scrollbar::width() - room)
                    .w(gpui_kit::component::scroll::Scrollbar::width())
                    .child(
                        gpui_kit::component::scroll::Scrollbar::vertical(self.content.scroll())
                            .id("branch-chooser-content-scrollbar")
                            .mode(gpui_kit::component::scroll::ScrollbarMode::Always)
                            .viewport_from_layout(),
                    ),
            )
    }
}

#[derive(Clone)]
enum BranchFormMode {
    Rename(Arc<BranchPlan>),
    Track(String),
}

struct BranchForm {
    owner: WeakEntity<GitTurtle>,
    path: Option<PathBuf>,
    mode: BranchFormMode,
    name: Entity<InputState>,
    checkout: bool,
    error: Option<String>,
    /// How often the form has been shown, which gives the line saying why
    /// its review failed a fresh node each time it opens again
    /// (`native_accessibility::refusal_line`).
    opened: usize,
    _subscription: Subscription,
}

impl BranchForm {
    fn new(
        owner: WeakEntity<GitTurtle>,
        path: Option<PathBuf>,
        mode: BranchFormMode,
        initial: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let name = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(initial)
                .placeholder("feature/my-change")
        });
        let subscription =
            cx.subscribe_in(&name, window, |this, _, event, window, cx| match event {
                InputEvent::Change => {
                    this.error = None;
                    cx.notify();
                }
                InputEvent::PressEnter { .. } if this.submit(window, cx) => {
                    window.close_dialog(cx);
                }
                _ => {}
            });
        Self {
            owner,
            path,
            mode,
            name,
            checkout: true,
            error: None,
            opened: 0,
            _subscription: subscription,
        }
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let name = self.name.read(cx).value().trim().to_owned();
        if name.is_empty() {
            self.error = Some("Enter a branch name.".into());
            cx.notify();
            return false;
        }
        let mode = self.mode.clone();
        let checkout = self.checkout;
        let form = cx.entity();
        let recover: PreparationFailure = Box::new(move |message, window, cx| {
            form.update(cx, |form, cx| {
                form.error = Some(message);
                cx.notify();
            });
            show_branch_form(form, window, cx);
        });
        let accepted = self.owner.update(cx, |this, cx| {
            if this.path != self.path { return false; }
            this.read_branch_action_recovering(move |repo| match mode {
                BranchFormMode::Rename(expected) => {
                    let plan = repo.rename_branch_plan(expected.as_ref(), &name)?;
                    let explanation = format!("Rename the local branch {} at {} to {name}. Its commit history and tracking configuration move to the new name.{}", plan.name, short_oid(&plan.oid), if plan.current_branch.as_deref() == Some(plan.name.as_str()) { " This is the current branch." } else { "" });
                    Ok(Prepared::Confirmation { title: format!("Rename {} to {name}", plan.name), explanation, action: "Rename branch", command: WriteCommand::Branch(BranchCommand::Rename { plan, new_name: name }.into()) })
                }
                BranchFormMode::Track(reference) => {
                    let plan = repo.tracking_branch_plan(&name, &reference, checkout)?;
                    let explanation = format!("Create local branch {name} at {} and track {}.{} The locally available remote tip is used; this action does not fetch.", short_oid(&plan.remote_oid), short_reference(&reference), if checkout { " Git will also switch this worktree to the new branch and preserve unrelated work." } else { " The current branch stays selected." });
                    Ok(Prepared::Confirmation { title: format!("Create tracking branch {name}"), explanation, action: if checkout { "Create and switch" } else { "Create branch" }, command: WriteCommand::Branch(BranchCommand::CreateTracking { plan }.into()) })
                }
            }, Some(recover), window, cx)
        }).unwrap_or(false);
        if !accepted {
            self.error = Some("The repository changed or another operation is running. Close this dialog and open the action again.".into());
            cx.notify();
        }
        accepted
    }
}

impl Render for BranchForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let target = match &self.mode {
            BranchFormMode::Rename(plan) => format!("{} · {}", plan.name, short_oid(&plan.oid)),
            BranchFormMode::Track(reference) => format!("Track {}", short_reference(reference)),
        };
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .text_size(crate::appearance::ui_text(12.))
                    .text_color(rgb(p.muted))
                    .child(target),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(field_label("Local branch name", cx))
                    .child(Input::new(&self.name)),
            )
            .when(matches!(&self.mode, BranchFormMode::Track(_)), |element| {
                element.child(
                    Checkbox::new("checkout-new-tracking-branch")
                        .debug_selector(|| "checkout-new-tracking-branch".into())
                        .label("Switch to the new branch")
                        .checked(self.checkout)
                        .on_click(cx.listener(|this, checked, _, cx| {
                            this.checkout = *checked;
                            cx.notify();
                        })),
                )
            })
            // Announced at once, again each time the form opens with it.
            .children(self.error.as_ref().map(|error| {
                native_accessibility::refusal_line(
                    "branch-form-error",
                    self.opened,
                    error.clone(),
                    |line| {
                        line.text_size(crate::appearance::ui_text(12.))
                            .text_color(rgb(p.warning))
                    },
                )
            }))
    }
}

struct RemoteManager {
    owner: WeakEntity<GitTurtle>,
    path: Option<PathBuf>,
    remotes: Arc<Vec<RemoteChoice>>,
    query: Entity<InputState>,
    /// The list's scroll, which reveals the Edit… or Remove… Tab moves onto.
    list: FocusReveal,
    _subscription: Subscription,
}

impl RemoteManager {
    fn new(
        owner: WeakEntity<GitTurtle>,
        path: Option<PathBuf>,
        remotes: Arc<Vec<RemoteChoice>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let query = cx.new(|cx| InputState::new(window, cx).placeholder("Find a remote"));
        let subscription = cx.subscribe(&query, |this, _, event, cx| {
            if matches!(event, InputEvent::Change) {
                this.list.scroll().set_offset(Point::default());
                cx.notify();
            }
        });
        Self {
            owner,
            path,
            remotes,
            query,
            list: FocusReveal::new(Trigger::Keyboard),
            _subscription: subscription,
        }
    }
}

impl Render for RemoteManager {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // A control focus has moved onto scrolls into view in this frame.
        self.list.reveal(window, cx);
        let p = palette(cx);
        let query = self.query.read(cx).value().trim().to_lowercase();
        let matches: Vec<_> = self
            .remotes
            .iter()
            .enumerate()
            .filter(|(_, remote)| query.is_empty() || remote.search.contains(&query))
            .take(CHOICE_LIMIT + 1)
            .collect();
        // An Edit… or Remove… Tab moves onto scrolls into view with its ring.
        // Past its height the list cues the rest with its scrollbar. Each
        // remote keeps its controls' ring room inside its own padding, so the
        // list keeps none around the remotes.
        let room = appearance::button_ring_room(cx);
        let cue = tags::ScrollCue::revealing(&self.list, Pixels::ZERO);
        div().flex().flex_col().gap_3()
            .child(div().flex().items_center().gap_2()
                .child(div().flex_1().child(Input::new(&self.query).cleanable(true)))
                .child(dialog_action("add-remote", "Add remote…", &self.owner, &self.path, None, |this, window, cx| this.open_remote_form(None, window, cx))))
            .child(div().text_size(crate::appearance::ui_text(12.)).text_color(rgb(p.muted)).child("Configure local destinations. Network activity starts only when you explicitly fetch, pull, or push."))
            .child(cue.list(div().on_children_prepainted(cue.observe(self.list.observe())).id("remote-manager-list").debug_selector(|| "remote-manager-list".into()).max_h(px(360.)).overflow_y_scroll().track_scroll(self.list.scroll()).flex().flex_col().gap_2()
                .children(matches.iter().take(CHOICE_LIMIT).map(|(index, choice)| {
                    let edit_focus = self.list.focus(("edit-remote", *index), cx);
                    let remove_focus = self.list.focus(("remove-remote", *index), cx);
                    let reveal = self.list.controls(vec![None, Some(edit_focus.clone()), Some(remove_focus.clone())], room);
                    let edit = Arc::clone(&choice.config);
                    let remove = Arc::clone(&choice.config);
                    let owner = self.owner.clone(); let path = self.path.clone();
                    let remove_owner = self.owner.clone(); let remove_path = self.path.clone();
                    let remote = &choice.config;
                    div().on_children_prepainted(reveal).id(("managed-remote", *index)).debug_selector({ let index = *index; move || format!("managed-remote-{index}") }).p_3().rounded(px(8.)).border_1().border_color(rgb(p.border)).flex().items_center().gap_3()
                        .child(div().flex_1().min_w_0().flex().flex_col().gap_1()
                            .child(div().text_size(crate::appearance::ui_text(13.)).font_weight(FontWeight::MEDIUM).child(remote.name.clone()))
                            .child(div().truncate().text_size(crate::appearance::ui_text(11.)).text_color(rgb(p.muted)).child(remote.urls.first().map_or("No fetch URL configured".into(), |url| workspace::display_remote_url(url))))
                            .child(div().text_size(crate::appearance::ui_text(11.)).text_color(rgb(p.muted)).child(format!("{} · {}", crate::counted(remote.urls.len(), "fetch URL", "fetch URLs"), crate::counted(remote.upstream_branches.len(), "upstream branch", "upstream branches")))))
                        .child(button(("edit-remote", *index), "Edit…", "", false).track_focus(&edit_focus).debug_selector({ let index = *index; move || format!("edit-remote-{index}") }).on_click(move |_, window, cx| {
                            let _ = owner.update(cx, |this, cx| { if this.path == path && this.page == AppPage::Repository && this.operation_busy.is_none() { window.close_dialog(cx); this.open_remote_form(Some(Arc::clone(&edit)), window, cx); } });
                        }))
                        .child(button(("remove-remote", *index), "Remove…", "", false).track_focus(&remove_focus).debug_selector({ let index = *index; move || format!("remove-remote-{index}") }).on_click(move |_, window, cx| {
                            let _ = remove_owner.update(cx, |this, cx| { if this.path == remove_path && this.page == AppPage::Repository && this.operation_busy.is_none() { window.close_dialog(cx); this.prepare_remote_remove(Arc::clone(&remove), window, cx); } });
                        }))
                }))
                .when(matches.is_empty(), |element| element.child(div().p_3().text_size(crate::appearance::ui_text(12.)).text_color(rgb(p.muted)).child(if self.remotes.is_empty() { "No remotes configured. Add a destination to fetch or publish your work." } else { "No remotes match this search." }))), "remote-manager-scrollbar"))
            .when(matches.len() > CHOICE_LIMIT, |element| element.child(div().text_size(crate::appearance::ui_text(11.)).text_color(rgb(p.muted)).child("Showing 40 matches. Narrow the search to find another remote.")))
    }
}

struct RemoteForm {
    owner: WeakEntity<GitTurtle>,
    path: Option<PathBuf>,
    expected: Option<Arc<RemoteConfig>>,
    name: Entity<InputState>,
    fetch: Entity<TextareaState>,
    push: Entity<TextareaState>,
    rules: Entity<TextareaState>,
    inherit_push: bool,
    store_refs: bool,
    advanced: bool,
    error: Option<String>,
    /// How often the form has been shown, which gives the line saying why
    /// its review failed a fresh node each time it opens again
    /// (`native_accessibility::refusal_line`).
    opened: usize,
    /// The form's scroll, which reveals the control Tab moves onto: its
    /// last controls lie below its fold in a half tile.
    body: FocusReveal,
}

impl RemoteForm {
    fn new(
        owner: WeakEntity<GitTurtle>,
        path: Option<PathBuf>,
        expected: Option<Arc<RemoteConfig>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let name = cx.new(|cx| {
            InputState::new(window, cx).default_value(
                expected
                    .as_ref()
                    .map_or("origin".into(), |remote| remote.name.clone()),
            )
        });
        let fetch = cx.new(|cx| {
            TextareaState::new(window, cx).placeholder(if expected.is_some() {
                "Leave blank to keep current fetch URLs"
            } else {
                "https://host/owner/repository.git"
            })
        });
        let push = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder("One push URL per line; blank keeps current URLs")
        });
        let rules = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder("Blank uses the standard branch mapping")
                .default_value(
                    expected
                        .as_ref()
                        .map_or(String::new(), |remote| remote.fetch_refspecs.join("\n")),
                )
        });
        let inherit_push = expected
            .as_ref()
            .is_none_or(|remote| remote.push_urls.is_empty());
        let store_refs = expected
            .as_ref()
            .is_none_or(|remote| !remote.fetch_refspecs.is_empty());
        Self {
            owner,
            path,
            expected,
            name,
            fetch,
            push,
            rules,
            inherit_push,
            store_refs,
            advanced: false,
            error: None,
            opened: 0,
            body: FocusReveal::new(Trigger::Keyboard),
        }
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let name = self.name.read(cx).value().trim().to_owned();
        let fetch = input_lines(&self.fetch.read(cx).value());
        let push = input_lines(&self.push.read(cx).value());
        let rules = input_lines(&self.rules.read(cx).value());
        if name.is_empty() || (self.expected.is_none() && fetch.is_empty()) {
            self.error = Some("Enter a remote name and at least one fetch URL.".into());
            cx.notify();
            return false;
        }
        if !self.inherit_push
            && push.is_empty()
            && self
                .expected
                .as_ref()
                .is_none_or(|remote| remote.push_urls.is_empty())
        {
            self.error = Some("Enter a push URL or choose to use the fetch URLs.".into());
            cx.notify();
            return false;
        }
        let expected = self.expected.clone();
        let inherit_push = self.inherit_push;
        let store_refs = self.store_refs;
        let form = cx.entity();
        let recover: PreparationFailure = Box::new(move |message, window, cx| {
            form.update(cx, |form, cx| {
                form.error = Some(message);
                cx.notify();
            });
            show_remote_form(form, window, cx);
        });
        let accepted = self.owner.update(cx, |this, cx| {
            if this.path != self.path { return false; }
            this.read_branch_action_recovering(move |repo| {
                if let Some(expected) = &expected {
                    let current = repo.remote_configs()?.into_iter().find(|remote| remote.name == expected.name).ok_or_else(|| anyhow::anyhow!("This remote no longer exists; reopen remote settings."))?;
                    anyhow::ensure!(current == **expected, "The remote changed; reopen its settings before editing it.");
                } else {
                    anyhow::ensure!(!repo.remote_configs()?.iter().any(|remote| remote.name == name), "A remote named '{name}' already exists.");
                }
                let mut replacement = expected.as_ref().map_or_else(|| RemoteConfig::new(&name, fetch.first().cloned().unwrap_or_default()), |remote| remote.as_ref().clone());
                if !fetch.is_empty() { replacement.urls = fetch; }
                if inherit_push { replacement.push_urls.clear(); } else if !push.is_empty() { replacement.push_urls = push; }
                if !store_refs { replacement.fetch_refspecs.clear(); }
                else if !rules.is_empty() { replacement.fetch_refspecs = rules; }
                else if replacement.fetch_refspecs.is_empty() { replacement.fetch_refspecs = RemoteConfig::new(&name, "").fetch_refspecs; }
                let mut explanation = format!("{} the local configuration for {name}. Future fetch and push actions use these destinations. Saving these settings does not contact a server.", if expected.is_some() { "Update" } else { "Add" });
                let fetch_display: Vec<_> = replacement.urls.iter().map(|url| workspace::display_remote_url(url)).collect();
                append_names(&mut explanation, "Fetch URLs", fetch_display.iter().map(String::as_str));
                if replacement.push_urls.is_empty() { explanation.push_str("\n\nPush uses the fetch URLs."); }
                else {
                    let push_display: Vec<_> = replacement.push_urls.iter().map(|url| workspace::display_remote_url(url)).collect();
                    append_names(&mut explanation, "Push URLs", push_display.iter().map(String::as_str));
                }
                append_names(&mut explanation, "Fetch rules", replacement.fetch_refspecs.iter().map(String::as_str));
                if replacement.fetch_refspecs.is_empty() { explanation.push_str("\n\nFetched branches will not be stored as remote-tracking references."); }
                let command = if let Some(expected) = expected { BranchCommand::EditRemote { expected: expected.as_ref().clone(), replacement } } else { BranchCommand::AddRemote { remote: replacement } };
                Ok(Prepared::Confirmation { title: format!("Save remote {name}"), explanation, action: "Save remote", command: WriteCommand::Branch(command.into()) })
            }, Some(recover), window, cx)
        }).unwrap_or(false);
        if !accepted {
            self.error = Some("The repository changed or another operation is running. Reopen remote settings when it finishes.".into());
            cx.notify();
        }
        accepted
    }
}

impl Render for RemoteForm {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        // The form lies in the dialog's content, as the profile form does,
        // and takes what the dialog's cap leaves it, scrolling the rest,
        // cued (`tags::ScrollCue`): the field or control Tab moves onto
        // scrolls into view with its ring's room (`focus_reveal`), where in
        // the dialog's own scrolling body Tab left its last controls below
        // the fold at 18 pt in a half tile. It keeps that room around its
        // controls, given back through its margin, and the pull of Fetch
        // rules…, a ghost Button at the dialog's text edge, with it on its
        // left, so nothing moves and its clip cuts no frame or ring; a
        // wrapper keeps the room below them inside the dialog's clip.
        let room = appearance::button_ring_room(cx);
        let lead = crate::ghost_pull(window) + room;
        self.body.reveal(window, cx);
        let [name, fetch, push, rules] = [
            self.name.read(cx).focus_handle(cx),
            self.fetch.read(cx).focus_handle(cx),
            self.push.read(cx).focus_handle(cx),
            self.rules.read(cx).focus_handle(cx),
        ];
        let [inherit, advanced, store] = [
            "remote-push-inherits-fetch",
            "remote-advanced-settings",
            "remote-store-tracking-refs",
        ]
        .map(|id| self.body.focus(id, cx));
        // The form's children in order, those that take focus with their
        // handle.
        let mut controls = vec![Some(name)];
        if self.expected.is_some() {
            controls.push(None);
        }
        controls.extend([Some(fetch), Some(inherit.clone())]);
        if !self.inherit_push {
            controls.push(Some(push));
        }
        controls.push(Some(advanced.clone()));
        let mut rules_controls = vec![Some(store.clone())];
        if self.store_refs {
            rules_controls.push(Some(rules));
        }
        let cue = crate::tags::ScrollCue::revealing(&self.body, room);
        div().flex().flex_col().min_h_0().pb(room).child(cue.list(div().on_children_prepainted(cue.observe(self.body.observe())).id("remote-form").debug_selector(|| "remote-form".into()).min_h_0().overflow_y_scroll().track_scroll(self.body.scroll()).p(room).m(-room).pl(lead).ml(-lead).flex().flex_col()
            .child(div().flex().flex_col().gap_3().on_children_prepainted(self.body.controls(controls, room))
            .child(div().flex().flex_col().gap_1().child(field_label("Remote name", cx)).child(Input::new(&self.name).disabled(self.expected.is_some())))
            .children(self.expected.as_ref().map(|remote| {
                div().text_size(crate::appearance::ui_text(11.)).text_color(rgb(p.muted)).child(format!("Current fetch: {}", remote.urls.iter().map(|url| workspace::display_remote_url(url)).collect::<Vec<_>>().join(" · ")))
            }))
            .child(div().flex().flex_col().gap_1().child(field_label(if self.expected.is_some() { "New fetch URLs" } else { "Fetch URLs" }, cx))
                .child(shortcuts::tab_leaves(Textarea::new(&self.fetch).h(crate::appearance::ui_size(70.)).aria_label("Fetch URLs, one per line")))
                .child(div().text_size(crate::appearance::ui_text(11.)).text_color(rgb(p.muted)).child(if self.expected.is_some() { "One URL per line. Leave blank to preserve the existing URLs." } else { "One URL or local repository path per line." })))
            .child(Checkbox::new("remote-push-inherits-fetch").track_focus(&inherit).label("Use fetch URLs for push").checked(self.inherit_push)
                .on_click(cx.listener(|this, checked, _, cx| { this.inherit_push = *checked; cx.notify(); })))
            .when(!self.inherit_push, |element| element.child(div().flex().flex_col().gap_1().child(field_label("Push URLs", cx))
                .children(self.expected.as_ref().filter(|remote| !remote.push_urls.is_empty()).map(|remote| div().text_size(crate::appearance::ui_text(11.)).text_color(rgb(p.muted)).child(format!("Current: {}", remote.push_urls.iter().map(|url| workspace::display_remote_url(url)).collect::<Vec<_>>().join(" · ")))))
                .child(shortcuts::tab_leaves(Textarea::new(&self.push).h(crate::appearance::ui_size(70.)).aria_label("Push URLs, one per line")))))
            // A ghost Button at the dialog's text edge, its label on that
            // edge and its frame its pull before it (`crate::edge_row`).
            .child(crate::edge_row(window).child(crate::edge_button("remote-advanced-settings", if self.advanced { "Hide fetch rules" } else { "Fetch rules…" }).track_focus(&advanced).debug_selector(|| "remote-advanced-settings".into())
                .on_click(cx.listener(|this, _, _, cx| { this.advanced = !this.advanced; cx.notify(); }))))
            .when(self.advanced, |element| element.child(div().flex().flex_col().gap_2().on_children_prepainted(self.body.controls(rules_controls, room))
                .child(Checkbox::new("remote-store-tracking-refs").track_focus(&store).label("Store fetched branches as remote-tracking references").checked(self.store_refs)
                    .on_click(cx.listener(|this, checked, _, cx| { this.store_refs = *checked; cx.notify(); })))
                .when(self.store_refs, |element| element.child(shortcuts::tab_leaves(Textarea::new(&self.rules).h(px(90.)).aria_label("Git fetch refspecs, one per line"))))
                .child(div().text_size(crate::appearance::ui_text(11.)).text_color(rgb(p.muted)).child("Advanced Git refspecs, one per line. The standard mapping tracks every remote branch under this remote's name."))))
            // Announced at once, again each time the form opens with it.
            .children(self.error.as_ref().map(|error| native_accessibility::refusal_line("remote-form-error", self.opened, error.clone(), |line| line.text_size(crate::appearance::ui_text(12.)).text_color(rgb(p.warning)))))), "remote-form-scrollbar").min_h_0())
    }
}

fn input_lines(value: &str) -> Vec<String> {
    value
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(str::to_owned)
        .collect()
}

/// Opens Add remote with its push URLs and fetch refspecs shown and returns
/// its three line lists, for the prose fields' Tab regression in
/// `views::tests`.
#[cfg(test)]
pub(crate) fn open_remote_lists(
    app: &mut GitTurtle,
    window: &mut Window,
    cx: &mut Context<GitTurtle>,
) -> [(&'static str, Entity<TextareaState>); 3] {
    let owner = cx.entity().downgrade();
    let path = app.path.clone();
    let form = cx.new(|cx| {
        let mut form = RemoteForm::new(owner, path, None, window, cx);
        form.inherit_push = false;
        form.advanced = true;
        form.store_refs = true;
        form
    });
    show_remote_form(form.clone(), window, cx);
    let form = form.read(cx);
    [
        ("the fetch URLs", form.fetch.clone()),
        ("the push URLs", form.push.clone()),
        ("the fetch refspecs", form.rules.clone()),
    ]
}

#[cfg(test)]
mod tests;
