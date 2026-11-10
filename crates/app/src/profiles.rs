//! Native named-profile picker. Definitions are app preferences; applying one is
//! a separate, captured Git operation with a visible configuration-scope review.
use crate::*;
use gitturtle_core::{ProfilePlan, WriteCommand};
use gpui_kit::{
    component::{
        WindowExt,
        checkbox::Checkbox,
        menu::{PopupMenu, PopupMenuItem},
    },
    prelude::FluentBuilder,
};
#[cfg(test)]
mod manager_tests;
#[cfg(test)]
mod menu_tests;
mod store;
use crate::focus_reveal::{FocusReveal, Trigger};
use crate::native_accessibility::{MenuReasons, MenuWait};
use store::{Definition, Mutation, Signing, Store};

/// Presses a profile's Apply…, whose review opens once its preparation
/// answers, for the regression of the reads a preparation's end runs in
/// `page_navigation::tests`.
#[cfg(test)]
pub(crate) fn apply_profile(app: &mut GitTurtle, window: &mut Window, cx: &mut Context<GitTurtle>) {
    let definition = Definition {
        id: "reviewer".into(),
        label: "Reviewer".into(),
        name: "Reviewer".into(),
        email: "reviewer@example.com".into(),
        signing: None,
    };
    app.prepare_profile(definition, window, cx);
}

#[derive(Default)]
pub(super) struct State {
    store: Option<Store>,
    error: Option<String>,
    saving: bool,
    failed: Option<Mutation>,
    draft: Option<Entity<ProfileForm>>,
    pub prepared: Option<Definition>,
    /// The body of the profiles dialog opened last, for tests.
    #[cfg(test)]
    manager: Option<FocusReveal>,
}

/// The height, at the default interface size, of the header's controls,
/// the profile picker's in either header: one height for the row's controls
/// (it was a 36 px tile in the full header beside the 28 px Settings).
const ROW_CONTROL: f32 = 28.;

/// The profile's icon, the person artwork, which only the profile draws.
const PROFILE_ICON: &str = "icons/user.svg";

/// What the profile menu's items wait for while a profile is saved, which
/// refuses choosing, creating and managing profiles until it ends: the
/// label the profile form shows meanwhile, heard in an item's name as
/// "available when saving profile finishes".
const SAVING_PROFILE: &str = "Saving profile…";
/// What New profile… waits for until the saved profiles it joins are first
/// read.
const LOADING_PROFILES: &str = "Loading profiles…";
/// Why New profile… is unavailable where the saved profiles could not be
/// read: a profile joins them, as the manager's New profile… does, and
/// Retry profile storage, below it, reads them again.
const STORAGE_UNAVAILABLE: &str = "Storage unavailable";
/// Why a saved profile cannot be applied where the repository could not be
/// read: applying one writes that repository's configuration.
const REPOSITORY_UNAVAILABLE: &str = "Repository unavailable";

/// The profile menu's groups of items, each refused for a reason of its
/// own ([`GitTurtle::profile_menu_reason`]), in the menu's order.
#[derive(Clone, Copy)]
enum MenuGroup {
    /// The saved profiles, listed once they are read, each applied to the
    /// repository.
    Saved,
    /// New profile…, which joins the saved profiles.
    Create,
    /// Manage profiles…, which needs neither.
    Manage,
}

impl MenuGroup {
    /// Every group, which the menu's key reads
    /// ([`GitTurtle::profile_menu_refusal`]).
    const ALL: [Self; 3] = [Self::Saved, Self::Create, Self::Manage];
}

/// Why a group of the profile menu's items is refused for the window's own
/// state rather than a write's ([`GitTurtle::profile_menu_reason`]).
#[derive(Clone, Copy)]
enum Refusal {
    /// A reason of its own, which no wait's end changes, drawn beside its
    /// command (`native_accessibility::detailed_item`): the saved profiles
    /// or the repository could not be read.
    Own(&'static str),
    /// What it waits for, by the label of what runs, in the room its
    /// command leaves (`native_accessibility::waiting_item`), after a
    /// write, which comes first: a profile save, the saved profiles' first
    /// read or the repository's.
    Waits(&'static str),
}

impl Refusal {
    /// The text it states, which the menu's key compares
    /// ([`GitTurtle::profile_menu_refusal`]): a reason of an item's own is
    /// never the label of a wait.
    fn text(self) -> &'static str {
        match self {
            Self::Own(reason) | Self::Waits(reason) => reason,
        }
    }

    /// `label`'s item, refused for `reason`, the first that holds of a
    /// reason of its own, the write's `wait` and what it waits for.
    fn item(label: SharedString, wait: Option<MenuWait>, reason: Option<Self>) -> PopupMenuItem {
        match reason {
            Some(Self::Own(reason)) => crate::native_accessibility::detailed_item(label, reason),
            reason => crate::native_accessibility::waiting_item(
                label,
                false,
                wait.or(reason.map(|reason| MenuWait::named(reason.text()))),
            ),
        }
    }
}

use crate::native_accessibility::text as label;

/// What the header picker's label is made of
/// ([`GitTurtle::profile_label`]): borrowed from the window's state where it
/// is compared, owned where a cache keeps it ([`Self::owned`]).
#[derive(Clone, PartialEq)]
pub(super) enum ProfileLabel<'a> {
    /// The profile assigned to the worktree, by its label, and whether the
    /// effective identity differs from it.
    Assigned { label: Cow<'a, str>, differs: bool },
    /// The effective author's name, with no profile assigned.
    Effective(Cow<'a, str>),
    /// Neither: the picker says "Profiles".
    None,
}

impl ProfileLabel<'_> {
    /// The label as the picker draws it.
    fn text(self) -> String {
        #[cfg(test)]
        PROFILE_LABELS_BUILT.with(|built| built.set(built.get() + 1));
        match self {
            Self::Assigned { label, differs } => {
                format!("{label}{}", if differs { " · differs" } else { "" })
            }
            Self::Effective(name) => name.into_owned(),
            Self::None => "Profiles".into(),
        }
    }

    /// This label as a key a cache keeps, owning what it borrowed.
    pub(super) fn owned(&self) -> ProfileLabel<'static> {
        match self {
            Self::Assigned { label, differs } => ProfileLabel::Assigned {
                label: Cow::Owned(String::from(&**label)),
                differs: *differs,
            },
            Self::Effective(name) => ProfileLabel::Effective(Cow::Owned(String::from(&**name))),
            Self::None => ProfileLabel::None,
        }
    }
}

/// Whether an assigned profile matches the effective identity, by both.
type ProfileMatches =
    std::cell::RefCell<crate::views::Measured<(Definition, gitturtle_core::GitProfile), bool>>;
/// The picker's label as drawn, with its accessible name, by what it is
/// made of.
type ProfileTitles =
    std::cell::RefCell<crate::views::Measured<ProfileLabel<'static>, (SharedString, SharedString)>>;
/// The picker's tooltip as drawn, by the effective identity's name, email
/// and whether its configuration is the worktree's own, if any.
type ProfileTooltips =
    std::cell::RefCell<crate::views::Measured<Option<(String, String, bool)>, SharedString>>;

thread_local! {
    /// Whether each assigned profile drawn lately matches the effective
    /// identity, by both ([`GitTurtle::profile_label`]).
    static PROFILE_MATCHES: ProfileMatches =
        const { std::cell::RefCell::new(crate::views::Measured::new()) };
    /// The picker's label as drawn lately, by what it is made of
    /// ([`GitTurtle::shown_profile_title`]).
    static PROFILE_TITLES: ProfileTitles =
        const { std::cell::RefCell::new(crate::views::Measured::new()) };
    /// The picker's tooltip as drawn lately, by what it is made of
    /// ([`GitTurtle::shown_profile_tooltip`]).
    static PROFILE_TOOLTIPS: ProfileTooltips =
        const { std::cell::RefCell::new(crate::views::Measured::new()) };
}

/// Empties [`PROFILE_MATCHES`], [`PROFILE_TITLES`] and [`PROFILE_TOOLTIPS`],
/// for a test that counts what the picker formats: a GPUI test's seeds
/// share the thread and its caches, so from the second seed on an identity
/// the first drew is kept.
#[cfg(test)]
pub(crate) fn forget_picker_text() {
    PROFILE_MATCHES.with_borrow_mut(|kept| *kept = crate::views::Measured::new());
    PROFILE_TITLES.with_borrow_mut(|kept| *kept = crate::views::Measured::new());
    PROFILE_TOOLTIPS.with_borrow_mut(|kept| *kept = crate::views::Measured::new());
}

#[cfg(test)]
thread_local! {
    /// How many times the picker's label was formatted
    /// ([`ProfileLabel::text`]), for a test.
    pub(crate) static PROFILE_LABELS_BUILT: std::cell::Cell<usize> =
        const { std::cell::Cell::new(0) };
    /// How many times the picker's accessible name or its tooltip was
    /// formatted, for a test.
    pub(crate) static PROFILE_PICKER_TEXT_FORMATTED: std::cell::Cell<usize> =
        const { std::cell::Cell::new(0) };
    /// The picker's accessible name and tooltip as last drawn, for a test:
    /// a Button writes no accessibility node a test can read.
    pub(crate) static PROFILE_PICKER_DRAWN: std::cell::RefCell<Option<(SharedString, SharedString)>> =
        const { std::cell::RefCell::new(None) };
    /// The commands of the profile menu as last built, for a test.
    static PROFILE_MENU_ITEMS: std::cell::RefCell<Vec<crate::branch_actions::PickerItem>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Adds `item` to the profile menu, noting in a test build what it says and
/// whether it is refused with a reason ([`PROFILE_MENU_ITEMS`]).
fn profile_menu_item(menu: PopupMenu, item: PopupMenuItem) -> PopupMenu {
    #[cfg(test)]
    if let Some(noted) = crate::branch_actions::PickerItem::of(&item) {
        PROFILE_MENU_ITEMS.with(|items| items.borrow_mut().push(noted));
    }
    menu.item(item)
}

/// The profile menu's first line: the identity Git authors with now, after
/// "Current:", so it is not read as one of the saved profiles below it. A
/// missing name or email is left out rather than drawn as empty brackets.
fn current_identity(profile: &gitturtle_core::GitProfile) -> String {
    let name = profile.name.trim();
    let email = profile.email.trim();
    match (name.is_empty(), email.is_empty()) {
        (false, false) => format!("Current: {name} <{email}>"),
        (false, true) => format!("Current: {name}"),
        (true, false) => format!("Current: <{email}>"),
        (true, true) => "Current: no Git identity".into(),
    }
}
impl GitTurtle {
    pub(super) fn profile_save_pending(&self) -> bool {
        self.profiles.saving
    }

    /// Why `group`'s items are refused for the window's own state rather
    /// than a write's, the first reason that holds, or `None` while none
    /// does: what the menu's rows state and what its key compares
    /// ([`Self::profile_menu_refusal`]), so the open menu is built again
    /// whenever a row's reason changes. A reason of the item's own comes
    /// first, as no wait's end would make it available, then a profile
    /// save, which refuses every item that acts, then the read the item
    /// waits for: the saved profiles' first read for New profile…, the
    /// repository's for a saved profile. The saved profiles are listed only
    /// once read, so until then their group states that read's reason. The
    /// menu's key once ranked the save first and its rows a reason of the
    /// item's own first, so a read that failed during a save left the open
    /// menu stating the save, and one that succeeded left it without the
    /// saved profiles, until the save ended.
    fn profile_menu_reason(&self, group: MenuGroup) -> Option<Refusal> {
        let saving = self
            .profiles
            .saving
            .then_some(Refusal::Waits(SAVING_PROFILE));
        let store = match (&self.profiles.store, &self.profiles.error) {
            (Some(_), _) => None,
            (None, None) => Some(Refusal::Waits(LOADING_PROFILES)),
            (None, Some(_)) => Some(Refusal::Own(STORAGE_UNAVAILABLE)),
        };
        match group {
            MenuGroup::Saved => store.or(match (self.repository.is_none(), self.loading) {
                (false, _) => saving,
                (true, Some(read)) => saving.or(Some(Refusal::Waits(read))),
                (true, None) => Some(Refusal::Own(REPOSITORY_UNAVAILABLE)),
            }),
            MenuGroup::Create => match store {
                own @ Some(Refusal::Own(_)) => own,
                read => saving.or(read),
            },
            MenuGroup::Manage => saving,
        }
    }

    /// Why Retry profile storage is offered, below the line that says what
    /// failed: the saved profiles' last read or save failed; `None` while
    /// neither did. The menu's key reads it as Retry's group
    /// ([`Self::profile_menu_refusal`]), so the open menu offers Retry as a
    /// failure comes and takes it away as one ends. Read only for why items
    /// were refused, which a failure changes nowhere once the saved
    /// profiles are listed, a menu open as a later read or save failed went
    /// without Retry, and one open as a read then succeeded kept it, until
    /// the menu was opened again.
    fn profile_store_failure(&self) -> Option<&'static str> {
        self.profiles.error.is_some().then_some(STORAGE_UNAVAILABLE)
    }

    /// Each group's reason by the text it states
    /// (`AppDropdown::follows_refusal`, which builds the open menu again
    /// when any changes): every reason a row states for the window's own
    /// state, so the menu is built again whenever one changes, and why
    /// Retry profile storage is offered. A write's is followed apart
    /// (`AppDropdown::follows_writes`).
    fn profile_menu_refusal(&self) -> MenuReasons {
        let [saved, create, manage] =
            MenuGroup::ALL.map(|group| self.profile_menu_reason(group).map(Refusal::text));
        [saved, create, manage, self.profile_store_failure()]
    }
    /// The profile store failed with `error`, for tests in other modules.
    #[cfg(test)]
    pub(crate) fn fail_profile_store(&mut self, error: &str) {
        self.profiles.error = Some(error.into());
    }
    pub(super) fn load_profiles(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let response = self.preferences_writer.submit_read(Store::load);
        cx.spawn_in(window, async move |this, cx| {
            let result = response
                .await
                .unwrap_or_else(|_| Err(anyhow::anyhow!("Profile loading was interrupted")));
            let _ = this.update_in(cx, |this, _, cx| {
                match result {
                    Ok(store) => {
                        this.profiles.store = Some(store);
                        this.profiles.error = None;
                    }
                    Err(error) => this.profiles.error = Some(format!("{error:#}")),
                }
                cx.notify();
            });
        })
        .detach();
    }
    /// The header picker's label: the assigned profile, marked where it
    /// differs from the effective identity, or the effective author's name.
    pub(super) fn profile_title(&self) -> String {
        self.profile_label().text()
    }

    /// What [`Self::profile_title`] is made of, borrowed, so the header
    /// compares it each frame without formatting the label: the assigned
    /// profile's label and whether the effective identity differs from it,
    /// or else the effective author's name, if any. Whether it differs is
    /// decided again only when the assigned profile or the effective
    /// identity changes ([`PROFILE_MATCHES`]): its comparison clones the
    /// profile's identity.
    pub(super) fn profile_label(&self) -> ProfileLabel<'_> {
        let assignment = self
            .path
            .as_ref()
            .and_then(|path| self.profiles.store.as_ref()?.assigned(path));
        match (assignment, self.shown_profile()) {
            (Some(saved), effective) => ProfileLabel::Assigned {
                label: Cow::Borrowed(&saved.label),
                differs: effective.is_some_and(|effective| {
                    !PROFILE_MATCHES.with_borrow_mut(|matches| {
                        *matches.get_where(
                            |(kept, against)| kept == saved && against == effective,
                            || (saved.clone(), effective.clone()),
                            |(saved, effective)| saved.identity().matches(effective),
                        )
                    })
                }),
            },
            (None, Some(effective)) if !effective.name.is_empty() => {
                ProfileLabel::Effective(Cow::Borrowed(&effective.name))
            }
            (None, _) => ProfileLabel::None,
        }
    }

    /// The effective identity the header speaks from: none on a marked
    /// tab, whose identity is the last one read, from the configuration of
    /// a repository the tab can no longer open, so the picker names no
    /// author and its tooltip no address from before, and an assigned
    /// profile no difference from it.
    fn shown_profile(&self) -> Option<&gitturtle_core::GitProfile> {
        self.profile
            .as_ref()
            .filter(|_| !self.shown_tab_unavailable())
    }

    /// [`Self::profile_title`] as the picker draws it, and the picker's
    /// accessible name, which reads it, formatted again only when what it is
    /// made of changes ([`PROFILE_TITLES`]).
    fn shown_profile_title(&self) -> (SharedString, SharedString) {
        let label = self.profile_label();
        PROFILE_TITLES.with_borrow_mut(|titles| {
            titles
                .get_where(
                    |kept| *kept == label,
                    || label.owned(),
                    |label| {
                        let title: SharedString = label.clone().text().into();
                        #[cfg(test)]
                        PROFILE_PICKER_TEXT_FORMATTED.with(|formatted| {
                            formatted.set(formatted.get() + 1);
                        });
                        let name = format!("Git profile: {title}. Choose or manage profiles");
                        (title, name.into())
                    },
                )
                .clone()
        })
    }

    /// The picker's tooltip: the effective identity with where its
    /// configuration lives, or what the picker offers without one, formatted
    /// again only when that identity changes ([`PROFILE_TOOLTIPS`]); the
    /// picker formatted it on every frame.
    fn shown_profile_tooltip(&self) -> SharedString {
        let identity = self
            .shown_profile()
            .map(|p| (p.name.as_str(), p.email.as_str(), p.private_worktree));
        PROFILE_TOOLTIPS.with_borrow_mut(|tooltips| {
            tooltips
                .get_where(
                    |kept| {
                        kept.as_ref()
                            .map(|(name, email, private)| (name.as_str(), email.as_str(), *private))
                            == identity
                    },
                    || {
                        identity.map(|(name, email, private)| {
                            (name.to_owned(), email.to_owned(), private)
                        })
                    },
                    |identity| {
                        #[cfg(test)]
                        PROFILE_PICKER_TEXT_FORMATTED.with(|formatted| {
                            formatted.set(formatted.get() + 1);
                        });
                        identity.as_ref().map_or_else(
                            || {
                                SharedString::new_static(
                                    "Choose or manage named Git author profiles",
                                )
                            },
                            |(name, email, private)| {
                                format!(
                                    "{name} <{email}> · {}",
                                    if *private {
                                        "Private worktree configuration"
                                    } else {
                                        "Repository configuration shared by linked worktrees"
                                    }
                                )
                                .into()
                            },
                        )
                    },
                )
                .clone()
        })
    }

    /// The picker, [`ROW_CONTROL`] px tall at the default interface size, as
    /// the header's other controls are; `compact` draws only the icon, in a
    /// square of that size. In the compact header's `row` it takes its
    /// neighbours' transparent frame and scaled icon; in the full header it
    /// is the secondary picker.
    pub(super) fn render_profile_button(
        &self,
        compact: bool,
        row: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (title, name) = self.shown_profile_title();
        let tooltip = self.shown_profile_tooltip();
        #[cfg(test)]
        PROFILE_PICKER_DRAWN.with(|drawn| {
            *drawn.borrow_mut() = Some((name.clone(), tooltip.clone()));
        });
        let owner = cx.entity().downgrade();
        let size = ROW_CONTROL;
        crate::views::note_control_icon(&"profile".into(), PROFILE_ICON);
        Button::new("profile")
            .debug_selector(|| "profile-button".into())
            .map(|button| {
                if row {
                    button
                        .with_variant(crate::appearance::control_button_variant(false))
                        .px(crate::appearance::ui_size(6.))
                        .rounded(crate::appearance::ui_size(7.))
                } else {
                    button.secondary()
                }
            })
            .h(crate::appearance::ui_size(size))
            .max_w(px(230.))
            // The kit sizes a label-less button as a square icon button; the
            // caret and the full button's padding would clip the icon.
            .map(|button| {
                if compact {
                    button.w(crate::appearance::ui_size(size))
                } else {
                    button.px_3().label(title.clone())
                }
            })
            .icon(Icon::default().path(PROFILE_ICON).size(if row {
                crate::appearance::ui_size(16.)
            } else {
                px(16.)
            }))
            .dropdown_caret(!compact)
            // Its menu opens until a write looks busy, offering none of the
            // items the write refuses; only then does the picker look
            // unavailable (`workspace::BusyLook`). It dimmed the moment any
            // write started. A profile save in progress leaves it so at once.
            .map(|picker| {
                crate::workspace::refusing(
                    picker,
                    || "profile-button".into(),
                    false,
                    self.operation_shown().is_some() || self.profiles.saving,
                )
            })
            .accessibility_label(name)
            .tooltip(tooltip)
            .app_menu_within(px(440.), move |mut menu, _, cx| {
                let Some(view) = owner.upgrade() else {
                    return menu;
                };
                let this = view.read(cx);
                let path = this.path.clone();
                #[cfg(test)]
                PROFILE_MENU_ITEMS.with(|items| items.borrow_mut().clear());
                // Read as it opens, and built again as a write starts or
                // ends and its reload lands (`AppDropdown::follows_writes`)
                // and as any group's reason to refuse its items for the
                // window's own state changes (`AppDropdown::follows_refusal`,
                // `GitTurtle::profile_menu_refusal`), each row stating its
                // group's reason (`GitTurtle::profile_menu_reason`). Every
                // item it refuses says why, as the branch picker's do, so
                // the menu keys rest on it and a screen reader hears why:
                // what it waits for, a write, a profile save, the saved
                // profiles' first read or the repository's, in the room its
                // command leaves (`native_accessibility::waiting_item`), or
                // else a reason of its own beside it, where the saved
                // profiles or the repository could not be read, which no
                // wait's end changes (`native_accessibility::detailed_item`).
                // New profile… while the saved profiles were read or could
                // not be, and the saved profiles while the repository was,
                // were dimmed with nothing to say why, and a write's items
                // once were too. The header that opens the menu is drawn
                // only on the repository's page, so no item is refused for
                // another page. Retrying the profile store, which no write
                // touches, stays available.
                let wait = this.menu_wait();
                let reason = |group| this.profile_menu_reason(group);
                if let Some(profile) = &this.profile {
                    menu = menu.label(current_identity(profile)).separator();
                }
                if let Some(store) = &this.profiles.store {
                    // A heading titles the saved profiles; with none saved,
                    // one line says so instead of titling an empty group.
                    menu = menu.label(if store.definitions.is_empty() {
                        "No saved profiles"
                    } else {
                        "Saved Git profiles"
                    });
                    let assigned = path.as_ref().and_then(|path| store.assigned(path));
                    // A profile is applied to the repository: it waits for
                    // the read that opens it, and is unavailable where that
                    // read could not open it.
                    let saved = reason(MenuGroup::Saved);
                    for profile in &store.definitions {
                        let profile = profile.clone();
                        let owner = owner.clone();
                        let path = path.clone();
                        let checked = assigned.is_some_and(|current| current.id == profile.id);
                        menu = profile_menu_item(
                            menu,
                            Refusal::item(
                                format!("{} · {}", profile.label, profile.email).into(),
                                wait,
                                saved,
                            )
                            .checked(checked)
                            .on_click(move |_, window, cx| {
                                let _ = owner.update(cx, |this, cx| {
                                    if this.path == path {
                                        this.prepare_profile(profile.clone(), window, cx);
                                    }
                                });
                            }),
                        );
                    }
                } else {
                    menu = menu.label("Profiles are unavailable or still loading");
                }
                if let Some(error) = &this.profiles.error {
                    menu = menu.label(format!(
                        "Profiles: {}",
                        error.lines().next().unwrap_or(error)
                    ));
                }
                let create_owner = owner.clone();
                let manage_owner = owner.clone();
                // Creating a profile needs the saved profiles it joins, as
                // the manager's New profile… does: it waits for their first
                // read, and is unavailable where they could not be read.
                let create = reason(MenuGroup::Create);
                menu = profile_menu_item(
                    menu.separator(),
                    Refusal::item("New profile…".into(), wait, create).on_click(
                        move |_, window, cx| {
                            let _ = create_owner
                                .update(cx, |this, cx| this.edit_profile(None, window, cx));
                        },
                    ),
                );
                let manage = reason(MenuGroup::Manage);
                menu = profile_menu_item(
                    menu,
                    Refusal::item("Manage profiles…".into(), wait, manage).on_click(
                        move |_, window, cx| {
                            let _ =
                                manage_owner.update(cx, |this, cx| this.open_profiles(window, cx));
                        },
                    ),
                );
                if this.profile_store_failure().is_some() {
                    let retry_owner = owner.clone();
                    menu = profile_menu_item(
                        menu,
                        PopupMenuItem::new("Retry profile storage").on_click(
                            move |_, window, cx| {
                                let _ = retry_owner.update(cx, |this, cx| {
                                    if let Some(mutation) = this.profiles.failed.clone() {
                                        this.save_profile_mutation(mutation, None, window, cx);
                                    } else {
                                        this.load_profiles(window, cx);
                                    }
                                });
                            },
                        ),
                    );
                }
                menu
            })
            // A write's start or end builds the open menu again, and so does
            // a change in why it refuses any group of items for the window's
            // own state.
            .follows_writes(cx.entity().downgrade())
            .follows_refusal(Self::profile_menu_refusal)
            .into_any_element()
    }
    pub(super) fn open_profiles(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.operation_busy.is_some() || self.profiles.saving {
            return;
        }
        let owner = cx.entity().downgrade();
        let path = self.path.clone();
        let target = self.repository.as_ref().filter(|_| self.page != AppPage::Projects).map(|repo| format!("Target: {}\n{}", self.project_name(repo.path()), repo.path().display())).unwrap_or_else(|| "Choose a repository to apply a profile. Saved profiles are available in every project.".into());
        let store = self.profiles.store.clone();
        let error = self.profiles.error.clone();
        let can_apply = self.repository.is_some() && self.page != AppPage::Projects;
        let body = FocusReveal::new(Trigger::Keyboard);
        #[cfg(test)]
        {
            self.profiles.manager = Some(body.clone());
        }
        // Built once: the dialog's builder runs at every render and clones
        // only the pointer.
        let manager = std::rc::Rc::new(Manager {
            owner,
            path,
            target,
            store,
            error,
            can_apply,
            body,
        });
        window.open_alert_dialog(cx, move |dialog, window, cx| {
            let room = appearance::button_ring_room(cx);
            let manager = manager.clone();
            // In the dialog's content rather than its scrolling body, whose
            // scroller lays what it holds out at full height and scrolled it
            // with no cue: the profiles take what the cap leaves below the
            // title and above Done, which stays drawn, and scroll the rest,
            // cued. They keep the focus ring's room below their last
            // control, which the footer's gap gives back.
            dialog
                .title("Git profiles")
                .width(appearance::dialog_width(px(600.), window))
                .max_h(appearance::dialog_max_height(window))
                .gap(crate::tags::DIALOG_FOOTER_GAP - room)
                .content(move |content, window, cx| {
                    content.min_h_0().child(manager.render(window, cx))
                })
                .footer(crate::tags::dialog_footer("Done", None))
        });
        window.refresh();
    }
    fn prepare_profile(
        &mut self,
        definition: Definition,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.operation_busy.is_some() || self.profiles.saving || self.page == AppPage::Projects {
            return;
        }
        let Some(repo) = self.repository.clone() else {
            return;
        };
        let path = repo.path().to_owned();
        let identity = definition.identity();
        self.operation_busy = Some("Reviewing Git profile…");
        let response = self
            .operations
            .submit_read(move || repo.profile_plan(identity));
        cx.spawn_in(window, async move |this, cx| {
            let result = response.await.unwrap_or_else(|_| Err(anyhow::anyhow!("Profile preparation was interrupted")));
            let _ = this.update_in(cx, |this, window, cx| {
                if this.operation_busy == Some("Reviewing Git profile…") { this.operation_busy = None; }
                this.read_deferred_working(window, cx);
                if this.path.as_ref() != Some(&path) || this.page == AppPage::Projects { return; }
                match result {
                    Ok(plan) => {
                        let scope = if plan.private_worktree { "Only this worktree's existing private Git configuration will change." } else { "Repository configuration is shared with every linked worktree. Other worktrees inheriting these keys will also use this identity; private worktree overrides remain effective." };
                        let signing = definition.signing.as_ref().map(|s| format!("Existing signing reference: {}\nFormat: {} · Require signed commits: {} · Require signed tags: {} · Signed annotated tags: {}.\nExisting signing requirements are never disabled.", s.key.as_deref().unwrap_or("keep configured key"), s.format.as_deref().unwrap_or("keep configured format"), s.commits, s.tags, s.annotated_tags)).unwrap_or_else(|| "All existing signing settings and requirements remain inherited.".into());
                        let explanation = format!("Apply '{}' as {} <{}>\n\nWorktree: {}\nConfiguration: {}\n\n{scope}\n\n{signing}\n\nThis explicit action affects subsequent Git commits and annotated tags. Global configuration, hooks and credentials are preserved. Reopening a repository never reapplies a profile.", definition.label, definition.name, definition.email, plan.worktree.display(), plan.config_path.display());
                        this.profiles.prepared = Some(definition.clone());
                        this.confirm_git_write(format!("Apply {} profile", definition.label), explanation, "Apply profile", WriteCommand::ApplyProfile(Arc::new(plan)), window, cx);
                    }
                    Err(error) => this.operation_error = Some(crate::views::OperationFailure::of(&error)),
                }
                cx.notify();
            });
        }).detach();
        cx.notify();
    }
    pub(super) fn submitted_profile(&self, plan: &ProfilePlan) -> Option<Definition> {
        self.profiles
            .prepared
            .as_ref()
            .filter(|profile| profile.identity() == plan.identity)
            .cloned()
    }
    pub(super) fn finish_profile_write(
        &mut self,
        path: &std::path::Path,
        profile: Definition,
        succeeded: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if succeeded {
            self.save_profile_mutation(
                Mutation::Assign {
                    worktree: path.to_owned(),
                    profile,
                },
                None,
                window,
                cx,
            );
        }
    }
    fn save_profile_mutation(
        &mut self,
        mutation: Mutation,
        form: Option<WeakEntity<ProfileForm>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.profiles.saving {
            return;
        }
        self.profiles.saving = true;
        self.profiles.error = None;
        let write_mutation = mutation.clone();
        let response = self
            .preferences_writer
            .submit(move || Store::update(&write_mutation));
        cx.spawn_in(window, async move |this, cx| {
            let result = response.await.unwrap_or_else(|_| {
                Err(anyhow::anyhow!(
                    "Profile save ended without a result; inspect storage before retrying"
                ))
            });
            let _ = this.update_in(cx, |this, window, cx| {
                this.profiles.saving = false;
                match result {
                    Ok(store) => {
                        this.profiles.store = Some(store);
                        this.profiles.failed = None;
                        this.profiles.error = None;
                        if let Some(form) = &form {
                            let _ = form.update(cx, |form, cx| {
                                form.pending = false;
                                cx.notify();
                            });
                            let visible = form.upgrade().is_some_and(|form| form.read(cx).visible);
                            this.profiles.draft = None;
                            if visible {
                                window.close_dialog(cx);
                                this.open_profiles(window, cx);
                            }
                        } else {
                            this.operation_notice = Some("Profile preferences saved".into());
                        }
                    }
                    Err(error) => {
                        let error = format!("{error:#}");
                        this.profiles.error = Some(error.clone());
                        this.profiles.failed = if form.is_some() { None } else { Some(mutation) };
                        // Said once: by the form's own line where the form
                        // shows, which the window's alert says, or else by the
                        // window's error. Both were said, the form's even
                        // while it was hidden.
                        let shown = form
                            .as_ref()
                            .and_then(|form| form.upgrade())
                            .filter(|form| form.read(cx).visible);
                        if let Some(form) = shown {
                            form.update(cx, |form, cx| {
                                form.pending = false;
                                crate::tags::set_error_line(&mut form.error, error, window, cx);
                                cx.notify();
                            });
                        } else {
                            this.operation_error =
                                Some(format!("Profile persistence: {error}").into());
                            if let Some(form) = &form {
                                let _ = form.update(cx, |form, cx| {
                                    form.pending = false;
                                    form.error = Some(error);
                                    cx.notify();
                                });
                            }
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    fn edit_profile(
        &mut self,
        previous: Option<Definition>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.operation_busy.is_some() || self.profiles.saving {
            return;
        }
        let owner = cx.entity().downgrade();
        let effective = self.profile.clone().unwrap_or_default();
        let form = self
            .profiles
            .draft
            .as_ref()
            .filter(|form| form.read(cx).previous == previous)
            .cloned()
            .unwrap_or_else(|| {
                cx.new(|cx| ProfileForm::new(owner, previous.clone(), effective, window, cx))
            });
        form.update(cx, |form, _| form.visible = true);
        self.profiles.draft = Some(form.clone());
        let focus = form.read(cx).label.read(cx).focus_handle(cx);
        let focus_form = form.downgrade();
        window.open_alert_dialog(cx, move |dialog, window, cx| {
            let submit = form.clone();
            let cancel = form.clone();
            let shown = form.clone();
            // In the dialog's content, as the profiles dialog's body is: the
            // form takes what the cap leaves it and scrolls the rest, cued,
            // with the ring's room below its last control, which the
            // footer's gap gives back.
            dialog
                .title(if previous.is_some() {
                    "Edit Git profile"
                } else {
                    "Create Git profile"
                })
                .width(appearance::dialog_width(px(550.), window))
                .max_h(appearance::dialog_max_height(window))
                .gap(crate::tags::DIALOG_FOOTER_GAP - appearance::button_ring_room(cx))
                .content(move |content, _, _| content.min_h_0().child(shown.clone()))
                .footer(crate::tags::dialog_footer("Save profile", Some("Cancel")))
                .on_ok(move |_, window, cx| {
                    submit.update(cx, |form, cx| form.submit(window, cx));
                    false
                })
                .on_cancel(move |_, _, cx| {
                    cancel.update(cx, |form, _| form.visible = false);
                    true
                })
        });
        window.refresh();
        window.on_next_frame(move |window, cx| {
            if focus_form
                .upgrade()
                .is_some_and(|form| form.read(cx).visible)
            {
                focus.focus(window, cx);
            }
        });
    }
    fn delete_profile(
        &mut self,
        definition: Definition,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let owner = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |dialog, window, _| {
            let dialog = dialog.max_h(appearance::dialog_max_height(window));
            let owner = owner.clone(); let definition = definition.clone(); let explanation = format!("Delete the saved '{}' profile and its app assignments? Repository Git configuration remains as it is. Repositories, keys and drafts are preserved.", definition.label);
            dialog.kind(DialogKind::Alert).title("Delete saved profile").child(label("delete-profile-consequences", explanation))
                .footer(crate::tags::dialog_footer("Delete profile", Some("Cancel")))
                .on_ok(move |_, window, cx| { let _ = owner.update(cx, |this, cx| { if this.operation_busy.is_none() { this.save_profile_mutation(Mutation::Delete(definition.clone()), None, window, cx); } }); true })
        });
        window.refresh();
    }
}

/// What the profiles dialog shows, captured as it opens, and the scroll of
/// its body.
struct Manager {
    owner: WeakEntity<GitTurtle>,
    path: Option<PathBuf>,
    target: String,
    store: Option<Store>,
    error: Option<String>,
    can_apply: bool,
    /// The body's scroll, which reveals the control Tab moves onto.
    body: FocusReveal,
}

impl Manager {
    /// The dialog's body. It has no height of its own past its 430 px: it
    /// shrinks to what the dialog's cap leaves it and scrolls the rest, cued
    /// by its scrollbar, or by its fades where its track is too short for a
    /// thumb (`tags::ScrollCue`), and a control the keyboard moves focus
    /// onto scrolls into view with its ring (`focus_reveal`). It keeps the
    /// ring's room around its controls, given back through its margin, so
    /// nothing moves, and a wrapper keeps the room below them inside the
    /// dialog's clip, as the branch chooser does. Laid out at full height in
    /// the dialog's scrolling body, at 461 × 490 it ran below the fold with
    /// nothing to say more lay past it.
    fn render(&self, window: &mut Window, cx: &mut App) -> Div {
        let p = palette(cx);
        let room = appearance::button_ring_room(cx);
        let body = &self.body;
        body.reveal(window, cx);
        let create = body.focus("new-git-profile", cx);
        let mut head = vec![None, None];
        if self.error.is_some() {
            head.push(None);
        }
        head.push(Some(create.clone()));
        let (owner, path, can_apply) = (&self.owner, &self.path, self.can_apply);
        let create_owner = owner.clone();
        let cue = crate::tags::ScrollCue::revealing(body, room);
        // New profile…, a ghost Button at the dialog's text edge, stands its
        // "+" on that edge, its frame its pull before it (DESIGN.md, "One
        // left edge"); the body keeps the pull with the ring's room on its
        // left, given back through its margin, so its clip cuts neither.
        let lead = crate::ghost_pull(window) + room;
        div().flex().flex_col().min_h_0().pb(room).child(cue.list(div().on_children_prepainted(cue.observe(body.observe())).id("profile-manager-content").debug_selector(|| "profile-manager-content".into()).max_h(px(430.) + room * 2.).min_h_0().overflow_y_scroll().track_scroll(body.scroll()).p(room).m(-room).pl(lead).ml(-lead).flex().flex_col().gap_3()
            .child(div().flex().flex_col().gap_3().on_children_prepainted(body.controls(head, room))
                .child(label("profiles-context", self.target.clone()).text_size(crate::appearance::ui_text(12.)))
                .child(label("profiles-purpose", "Profiles choose Git author identity. Provider sign-in, credential helpers and private keys stay separate. Editing or deleting a definition does not change Git configuration.").text_size(crate::appearance::ui_text(12.)).text_color(rgb(p.muted)))
                .children(self.error.as_ref().map(|error| label("profiles-error", error.clone()).text_color(rgb(p.warning))))
                .child(edge_row(window).child(button("new-git-profile", "New profile…", "plus", false).track_focus(&create).debug_selector(|| "new-git-profile".into()).flex_shrink(1.).disabled(self.store.is_none()).on_click(move |_, window, cx| { let _ = create_owner.update(cx, |this, cx| { window.close_dialog(cx); this.edit_profile(None, window, cx); }); }))))
            .children(self.store.iter().flat_map(|store| &store.definitions).enumerate().map(|(index, profile)| {
                let apply_owner = owner.clone(); let edit_owner = owner.clone(); let delete_owner = owner.clone();
                let apply_profile = profile.clone(); let edit_profile = profile.clone(); let delete_profile = profile.clone(); let path = path.clone();
                let focus = ["apply-profile", "edit-profile", "delete-profile"].map(|id| body.focus((id, index), cx));
                div().p_3().rounded(px(8.)).border_1().border_color(rgb(p.border)).flex().flex_col().gap_2()
                    .child(div().text_size(crate::appearance::ui_text(13.)).font_weight(FontWeight::SEMIBOLD).child(profile.label.clone()))
                    .child(div().text_size(crate::appearance::ui_text(12.)).child(format!("{} <{}>", profile.name, profile.email)))
                    .child(div().flex().gap_2().on_children_prepainted(body.controls(focus.iter().cloned().map(Some).collect(), room))
                        .child(button(("apply-profile", index), "Apply…", "", false).track_focus(&focus[0]).debug_selector(move || format!("apply-profile-{index}")).disabled(!can_apply).on_click(move |_, window, cx| { let _ = apply_owner.update(cx, |this, cx| { if this.path == path { window.close_dialog(cx); this.prepare_profile(apply_profile.clone(), window, cx); } }); }))
                        .child(button(("edit-profile", index), "Edit…", "", false).track_focus(&focus[1]).debug_selector(move || format!("edit-profile-{index}")).on_click(move |_, window, cx| { let _ = edit_owner.update(cx, |this, cx| { window.close_dialog(cx); this.edit_profile(Some(edit_profile.clone()), window, cx); }); }))
                        .child(button(("delete-profile", index), "Delete…", "", false).track_focus(&focus[2]).debug_selector(move || format!("delete-profile-{index}")).on_click(move |_, window, cx| { let _ = delete_owner.update(cx, |this, cx| { window.close_dialog(cx); this.delete_profile(delete_profile.clone(), window, cx); }); })))
            })), "profile-manager-scrollbar").min_h_0())
    }
}

struct ProfileForm {
    owner: WeakEntity<GitTurtle>,
    previous: Option<Definition>,
    id: String,
    label: Entity<InputState>,
    name: Entity<InputState>,
    email: Entity<InputState>,
    signing: Signing,
    current_signing: Signing,
    capture_signing: bool,
    pending: bool,
    visible: bool,
    error: Option<String>,
    /// The form's scroll, which reveals the field or control Tab moves onto.
    body: FocusReveal,
}
impl ProfileForm {
    fn new(
        owner: WeakEntity<GitTurtle>,
        previous: Option<Definition>,
        effective: gitturtle_core::GitProfile,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let id = previous.as_ref().map(|p| p.id.clone()).unwrap_or_else(|| {
            format!(
                "profile-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
            )
        });
        let text = |value: String,
                    placeholder: &'static str,
                    window: &mut Window,
                    cx: &mut Context<Self>| {
            cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(value)
                    .placeholder(placeholder)
            })
        };
        Self {
            owner,
            id,
            label: text(
                previous
                    .as_ref()
                    .map(|p| p.label.clone())
                    .unwrap_or_default(),
                "Personal or Work",
                window,
                cx,
            ),
            name: text(
                previous
                    .as_ref()
                    .map(|p| p.name.clone())
                    .unwrap_or_else(|| effective.name.clone()),
                "Git author name",
                window,
                cx,
            ),
            email: text(
                previous
                    .as_ref()
                    .map(|p| p.email.clone())
                    .unwrap_or_else(|| effective.email.clone()),
                "author@example.com",
                window,
                cx,
            ),
            signing: previous
                .as_ref()
                .and_then(|p| p.signing.clone())
                .unwrap_or_else(|| Signing::capture(&effective)),
            current_signing: Signing::capture(&effective),
            capture_signing: previous.as_ref().is_some_and(|p| p.signing.is_some()),
            previous,
            pending: false,
            visible: true,
            error: None,
            body: FocusReveal::new(Trigger::Keyboard),
        }
    }
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending {
            return;
        }
        let definition = Definition {
            id: self.id.clone(),
            label: self.label.read(cx).value().trim().into(),
            name: self.name.read(cx).value().trim().into(),
            email: self.email.read(cx).value().trim().into(),
            signing: self.capture_signing.then(|| self.signing.clone()),
        };
        if let Err(error) = definition.validate() {
            crate::tags::set_error_line(&mut self.error, format!("{error:#}"), window, cx);
            cx.notify();
            return;
        }
        let form = cx.entity().downgrade();
        let mutation = Mutation::Save {
            previous: self.previous.clone(),
            definition,
        };
        let accepted = self
            .owner
            .update(cx, |this, cx| {
                if this.operation_busy.is_some() || this.profiles.saving {
                    return false;
                }
                this.save_profile_mutation(mutation, Some(form), window, cx);
                true
            })
            .unwrap_or(false);
        self.pending = accepted;
        if !accepted {
            crate::tags::set_error_line(
                &mut self.error,
                "Wait for the current operation before editing profiles.",
                window,
                cx,
            );
        }
        cx.notify();
    }
}
impl Render for ProfileForm {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        // The form has no height of its own past its 420 px: it shrinks to
        // what the dialog's cap leaves it and scrolls the rest, cued
        // (`tags::ScrollCue`), and the field or control Tab moves onto
        // scrolls into view with its ring's room. It keeps that room around
        // its controls, given back through its margin, so nothing moves, and
        // a wrapper keeps the room below them inside the dialog's clip. Laid
        // out at full height in the dialog's scrolling body, at 461 × 490 it
        // ran below the fold with nothing to say more lay past it.
        let room = appearance::button_ring_room(cx);
        self.body.reveal(window, cx);
        let [label_field, name_field, email_field] =
            [&self.label, &self.name, &self.email].map(|field| field.read(cx).focus_handle(cx));
        let signing = self.body.focus("profile-capture-signing", cx);
        let signer = self.body.focus("profile-use-current-signer", cx);
        // The form's children in order, those that take focus with their
        // handle.
        let controls = vec![
            None,
            Some(label_field),
            None,
            Some(name_field),
            None,
            Some(email_field),
            Some(signing.clone()),
            Some(signer.clone()),
        ];
        let cue = crate::tags::ScrollCue::revealing(&self.body, room);
        // Use current repository signing settings, a ghost Button at the
        // dialog's text edge, stands its label on that edge, its frame its
        // pull before it, and wraps rather than run past the form's column
        // (`edge_button`); the form keeps the pull with the ring's room on
        // its left, given back through its margin, so its clip cuts neither.
        let lead = crate::ghost_pull(window) + room;
        div().flex().flex_col().min_h_0().pb(room).child(cue.list(div().on_children_prepainted(cue.observe(self.body.observe())).id("profile-form").debug_selector(|| "profile-form".into()).max_h(px(420.) + room * 2.).min_h_0().overflow_y_scroll().track_scroll(self.body.scroll()).p(room).m(-room).pl(lead).ml(-lead).flex().flex_col()
            .child(div().flex().flex_col().gap_3().on_children_prepainted(self.body.controls(controls, room))
            .child(label("profile-label", "Profile name").text_size(crate::appearance::ui_text(12.))).child(Input::new(&self.label).aria_label("Profile name").disabled(self.pending))
            .child(label("profile-author-label", "Git author name").text_size(crate::appearance::ui_text(12.))).child(Input::new(&self.name).aria_label("Profile Git author name").disabled(self.pending))
            .child(label("profile-email-label", "Git author email").text_size(crate::appearance::ui_text(12.))).child(Input::new(&self.email).aria_label("Profile Git author email").disabled(self.pending))
            .child(Checkbox::new("profile-capture-signing").track_focus(&signing).label("Include the existing signing configuration").checked(self.capture_signing).disabled(self.pending).on_click(cx.listener(|this, checked: &bool, _, cx| { this.capture_signing = *checked; cx.notify(); })))
            .child(edge_row(window).child(edge_button("profile-use-current-signer", "Use current repository signing settings").track_focus(&signer).debug_selector(|| "profile-use-current-signer".into()).disabled(self.pending).on_click(cx.listener(|this, _, _, cx| { this.signing = this.current_signing.clone(); this.capture_signing = true; cx.notify(); }))))
            .when(self.capture_signing, |element| element.child(label("profile-signing-reference", format!("Key reference: {}\nFormat: {} · Commit signing: {} · Tag signing: {} · Annotated signing: {}", self.signing.key.as_deref().unwrap_or("Keep repository key"), self.signing.format.as_deref().unwrap_or("Keep repository format"), self.signing.commits, self.signing.tags, self.signing.annotated_tags)).text_size(crate::appearance::ui_text(11.)).text_color(rgb(p.muted))))
            .child(label("profile-save-explanation", "Save creates or edits this reusable profile. Apply it separately to a reviewed repository. Existing signing requirements remain enabled; unavailable keys produce Git errors without an unsigned fallback.").debug_selector(|| "profile-save-explanation".into()).text_size(crate::appearance::ui_text(12.)).text_color(rgb(p.muted)))
            .when(self.pending, |element| element.child(label("profile-save-pending", "Saving profile…").text_color(rgb(p.accent))))
            .children(self.error.as_ref().map(|error| crate::tags::error_line("profile-form-error", error, None, window, cx).text_color(rgb(p.warning))))), "profile-form-scrollbar").min_h_0())
    }
}
