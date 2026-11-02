//! Native conventions and explicitly initiated local application handoff.
use crate::*;
use gpui_kit::component::{
    WindowExt,
    input::{Copy, Cut, Paste, Redo, SelectAll, Undo},
};
use std::path::Path;

gpui_kit::actions!(
    gitturtle_platform,
    [
        ShowHistory,
        RevealRepository,
        OpenEditor,
        OpenTerminal,
        ShortcutHelp,
        MainMenu,
        BrowseStashes,
        MinimizeWindow,
        CloseWindow,
        ZoomWindow,
        HideApplication,
        HideOtherApplications,
        ShowAllApplications
    ]
);

#[cfg(target_os = "linux")]
pub(super) struct PrimaryMenu {
    owner: WeakEntity<GitTurtle>,
    menu: Option<Entity<gpui_kit::component::menu::PopupMenu>>,
    return_focus: Option<FocusHandle>,
    _dismiss: Option<Subscription>,
    _focus_out: Option<Subscription>,
    /// Builds the open menu's rows again in place as what they read
    /// changes (`toggle`).
    _follows: Option<Subscription>,
    /// Whether the trigger shows "Menu" beside its icon, as the tab strip
    /// decides each frame (`GitTurtle::tab_strip_labels`) before this view
    /// renders inside it.
    labelled: bool,
    /// The trigger's bounds as the last frame painted them, below which the
    /// menu opens.
    trigger: std::rc::Rc<std::cell::Cell<Option<Bounds<Pixels>>>>,
}

/// One of the Main Menu's rows as it is built ([`PrimaryMenu::rows`]): its
/// command, the palette's reason it is refused, and the write it waits
/// for, which it states; `None` for a separator.
#[cfg(target_os = "linux")]
type MainMenuRow = Option<(
    command_palette::CommandSpec,
    Option<&'static str>,
    Option<native_accessibility::MenuWait>,
)>;

/// What a Main Menu row draws ([`PrimaryMenu::drawn`]): its command, the
/// reason it is refused and the write it waits for; `None` for a
/// separator. The open menu is built again when these change.
#[cfg(target_os = "linux")]
type DrawnRow = Option<(
    command_palette::CommandId,
    Option<&'static str>,
    Option<native_accessibility::MenuWait>,
)>;

#[cfg(target_os = "linux")]
impl PrimaryMenu {
    /// The Main Menu's commands in order, `None` for a separator. Browse
    /// stashes… follows the View menu's repository inspectors, as the
    /// palette lists it among the repository's tools.
    const ORDER: [Option<command_palette::CommandId>; 19] = {
        use command_palette::CommandId;
        [
            Some(CommandId::Open),
            Some(CommandId::Projects),
            None,
            Some(CommandId::History),
            Some(CommandId::Changes),
            Some(CommandId::QuickOpen),
            Some(CommandId::Compare),
            Some(CommandId::Stashes),
            None,
            Some(CommandId::Editor),
            Some(CommandId::Terminal),
            Some(CommandId::Reveal),
            None,
            Some(CommandId::Palette),
            Some(CommandId::Activity),
            None,
            Some(CommandId::Settings),
            Some(CommandId::Help),
            Some(CommandId::About),
        ]
    };

    pub(super) fn new(owner: WeakEntity<GitTurtle>) -> Self {
        Self {
            owner,
            menu: None,
            return_focus: None,
            _dismiss: None,
            _focus_out: None,
            _follows: None,
            labelled: true,
            trigger: Default::default(),
        }
    }

    /// Shows the trigger's label, or only its icon where the tab strip
    /// gives its room to the tabs' names. The strip sets it while it
    /// renders, before this view renders inside it in the same frame, so it
    /// needs no notification.
    pub(super) fn set_labelled(&mut self, labelled: bool) {
        self.labelled = labelled;
    }

    /// The Main Menu's rows as `app` reads them now: each command's reason
    /// is the palette's, and Browse stashes… waits for a write as the
    /// Working Changes Actions menu's item does, and says so.
    fn rows(app: &GitTurtle, window: &Window, cx: &App) -> Vec<MainMenuRow> {
        use command_palette::{COMMANDS, CommandId};
        #[cfg(test)]
        main_menu_tests::ROWS_READ.with(|read| read.set(read.get() + 1));
        Self::ORDER
            .iter()
            .map(|id| {
                id.map(|id| {
                    let spec = *COMMANDS
                        .iter()
                        .find(|spec| spec.id == id)
                        .expect("menu command is registered");
                    let reason = app.menu_command_reason(id, window, cx);
                    let wait = if id == CommandId::Stashes {
                        app.stashes_menu_wait(reason)
                    } else {
                        None
                    };
                    (spec, reason, wait)
                })
            })
            .collect()
    }

    /// What `rows` draw: each row's command, reason and wait.
    fn drawn(rows: &[MainMenuRow]) -> Vec<DrawnRow> {
        rows.iter()
            .map(|row| row.map(|(spec, reason, wait)| (spec.id, reason, wait)))
            .collect()
    }

    pub(super) fn toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.menu.is_some() {
            self.close(window, cx);
            return;
        }
        if window.has_active_dialog(cx) || window.has_active_sheet(cx) {
            return;
        }
        use gpui_kit::component::menu::{CursorRests, PopupMenu, PopupMenuItem};
        let Some(owner) = self.owner.upgrade() else {
            return;
        };
        let path = owner.read(cx).path.clone();
        let focus = window
            .focused(cx)
            .unwrap_or_else(|| owner.read(cx).app_focus.clone());
        self.return_focus = Some(focus.clone());
        let rows = Self::rows(owner.read(cx), window, cx);
        let target = self.owner.clone();
        // The menu's fill opens the kit's popup offset below its trigger
        // (the popup's top padding), the gap every menu keeps between its
        // drawn edge and what it opened from (`appearance::popup_gap`), and
        // from F10, Enter or Space with its first command under the
        // keyboard's cursor (`native_accessibility::open_menu`).
        let top = self
            .trigger
            .get()
            .map(|trigger| trigger.bottom() + gpui_kit::component::popup_offset(window, cx));
        let keyboard = window.last_input_was_keyboard();
        // The menu with `rows`, as it opens and as it is built again. Each
        // item acts on the repository the menu opened for, checked again
        // as it is chosen (`run_menu_command`).
        let build = std::rc::Rc::new(
            move |mut menu: PopupMenu,
                  rows: &[MainMenuRow],
                  window: &mut Window,
                  cx: &mut Context<PopupMenu>| {
                menu = menu
                    .action_context(focus.clone())
                    .min_w(appearance::ui_size(305.))
                    .max_w((window.viewport_size().width - px(24.)).min(appearance::ui_size(390.)));
                for row in rows {
                    let Some((spec, reason, wait)) = row else {
                        menu = menu.separator();
                        continue;
                    };
                    let command = spec.id;
                    let target = target.clone();
                    let path = path.clone();
                    let return_focus = focus.clone();
                    let mut item = match wait {
                        Some(wait) => {
                            native_accessibility::waiting_item(spec.label, false, Some(*wait))
                        }
                        None => PopupMenuItem::new(spec.label).disabled(reason.is_some()),
                    };
                    if let Some(action) = spec.shortcut.and_then(shortcuts::action) {
                        item = item.action(action);
                    }
                    menu = menu.item(item.on_click(move |_, window, cx| {
                        // Restore the originating editor before opening the
                        // workflow; modal return focus must never capture
                        // this transient menu.
                        return_focus.focus(window, cx);
                        let _ = target.update(cx, |this, cx| {
                            this.run_menu_command(command, &path, window, cx)
                        });
                    }));
                }
                native_accessibility::open_menu(menu, keyboard, top, None, window, cx)
            },
        );
        let menu = PopupMenu::build(window, cx, {
            let (build, rows) = (build.clone(), rows.clone());
            move |menu, window, cx| build(menu, &rows, window, cx)
        });
        // A write that starts or ends while the menu is open, or the reload
        // after it landing, builds its rows again in place, as the Actions
        // menu follows writes (`AppDropdown::follows_writes`), the kit
        // keeping the keyboard's cursor on its item or resting it while that
        // item waits: built only as it opened, Browse stashes… went on
        // saying "Fetching…" once the fetch had ended, and an item the write
        // refused stayed refused until the menu was opened again.
        // Every notify of the app reaches it, as a fetch's progress notices
        // do, so it compares what the rows read (`MenuState`), a few fields,
        // and reads the rows only as that changes: each row's reason reads
        // the palette's availability, which clones the selection's scope,
        // so reading them allocated for each of the menu's fifteen commands
        // at every notify.
        let mut read = owner.read(cx).menu_state();
        let mut drawn = Self::drawn(&rows);
        self._follows = Some(
            cx.observe_in(&owner, window, move |this, owner, window, cx| {
                let Some(menu) = this.menu.clone() else {
                    return;
                };
                let now = owner.read(cx).menu_state();
                if now == read {
                    return;
                }
                read = now;
                let rows = Self::rows(owner.read(cx), window, cx);
                let now = Self::drawn(&rows);
                if now == drawn {
                    return;
                }
                drawn = now;
                let build = build.clone();
                menu.update(cx, |menu, cx| {
                    menu.rebuild(window, cx, move |menu, window, cx| {
                        build(menu, &rows, window, cx)
                    })
                });
            }),
        );
        // A rest that begins, the keyboard's cursor on Browse stashes… as a
        // write starts, says once why it waits, as every menu that follows
        // writes says it (`native_accessibility::announces_resting`).
        window
            .subscribe(&menu, cx, |_, rest: &CursorRests, window, cx| {
                if rest.label != rest.name {
                    crate::copy_feedback::announce(window, cx, rest.label.clone());
                }
            })
            .detach();
        self._dismiss = Some(
            cx.subscribe_in(&menu, window, |this, _, _: &DismissEvent, _, cx| {
                this.menu = None;
                this._follows = None;
                cx.notify();
            }),
        );
        let menu_focus = menu.focus_handle(cx);
        self._focus_out = Some(cx.on_focus_out(&menu_focus, window, |this, _, _, cx| {
            // A newly focused control or dialog owns focus now; remove the
            // transient menu without restoring over that destination.
            this.menu = None;
            this._follows = None;
            cx.notify();
        }));
        menu_focus.focus(window, cx);
        self.menu = Some(menu);
        cx.notify();
    }

    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self._follows = None;
        if let Some(menu) = self.menu.take()
            && menu.focus_handle(cx).contains_focused(window, cx)
            && let Some(focus) = &self.return_focus
        {
            focus.focus(window, cx);
        }
        cx.notify();
    }
}

#[cfg(target_os = "linux")]
impl Render for PrimaryMenu {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use gpui_kit::{
            base::{ElementExt as _, Popup},
            component::IconName,
            prelude::FluentBuilder,
        };
        // Popup owns anchoring only. Keeping focus and opening in this entity
        // avoids a popover shell taking focus before we capture the editor.
        let open = self.menu.is_some();
        // Where the tab strip needs the room for its tabs' names, the
        // trigger is its icon alone, its name and tooltip unchanged.
        const NAME: &str = "Main Menu";
        let trigger = if self.labelled {
            button("main-menu", "Menu", "", open).accessibility_label(NAME)
        } else {
            crate::views::compact_control_name(button("main-menu", "", "", open), "main-menu", NAME)
        };
        // Its drawn edge, the ring outside its fill, stands the popup margin
        // inside the window, as every menu's does
        // (`gpui_kit::component::POPUP_WINDOW_MARGIN`): moved in from the
        // window's side at 13 pt, its fill stood 8 px in and its edge 7 px.
        let mut popup = Popup::new(
            "gitturtle-primary-menu",
            trigger
                .debug_selector(|| "main-menu".into())
                .icon(Icon::new(IconName::Menu).size(appearance::ui_size(16.)))
                .tooltip(format!(
                    "Main Menu · {}",
                    shortcuts::label(shortcuts::ShortcutId::MainMenu)
                ))
                .on_click(cx.listener(|this, _, window, cx| this.toggle(window, cx)))
                .on_prepaint({
                    let trigger = self.trigger.clone();
                    move |bounds, _, _| trigger.set(Some(bounds))
                }),
        )
        .margin(gpui_kit::component::popup_fill_margin())
        .when_some(self.menu.clone(), |popup, menu| {
            popup.content(
                div()
                    .pt(gpui_kit::component::popup_offset(window, cx))
                    .child(menu),
            )
        });
        // App shortcuts still work from an open menu. Close it before the
        // existing handler captures focus for its next workflow/dialog.
        for spec in shortcuts::SHORTCUTS {
            if spec.id == shortcuts::ShortcutId::MainMenu {
                continue;
            }
            let Some(action) = shortcuts::action(spec.id) else {
                continue;
            };
            let view = cx.entity();
            popup = popup.on_boxed_action(action.as_ref(), move |_, window, cx| {
                view.update(cx, |this, cx| this.close(window, cx));
                cx.propagate();
            });
        }
        popup
    }
}

/// What the native menus read from their window, compared with what they
/// were last built from in each frame it draws
/// ([`GitTurtle::update_native_menus`]), so they are built again only as it
/// changes: whether a repository is open, the write or request that runs
/// as the busy spell, by the label it runs under, whether Projects is
/// refused (`GitTurtle::projects_refusal`), which a wait for a dropped
/// review's read never refuses, since Projects ends it, whether the
/// repository's page is shown, and whether a write runs or the reload after
/// it has yet to land (`GitTurtle::write_unsettled`). Each is a read of a
/// field or two; Browse Stashes… is offered or refused for the palette's
/// reason, which reads exactly these, and waits for the write they name
/// ([`StashesItem`]). The Linux Main Menu, open, reads its rows again only
/// as these change (`PrimaryMenu::toggle`): the palette's reason for each of
/// its commands reads nothing else (`CommandId::reason`), and the label the
/// write runs under decides its busy reason. A command added to that menu
/// whose reason reads more, as File History reads the selection, needs
/// that read here too, or the open menu would not follow it.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub(super) struct MenuState {
    repository: bool,
    busy: Option<&'static str>,
    projects_refused: bool,
    workspace: bool,
    unsettled: bool,
}

/// Browse Stashes… in the menu bar's View menu as the menus are built, as
/// the Linux Main Menu's Browse stashes… stands: offered, refused for a
/// reason of its own (no repository, or another page shown), or waiting
/// for a write, or the reload after it, by the label it runs under
/// ("Fetching…", "Refreshing…"), which its title then states after the
/// command, "Browse Stashes… · Fetching…", as a control waiting in place
/// says what it waits for (decided 2026-11-02: a native menu item has no
/// room of its own at its trailing edge for the write, which the Linux row
/// draws there, and its title is what VoiceOver reads).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum StashesItem {
    Offered,
    Refused,
    Waits(&'static str),
}

impl StashesItem {
    /// The View menu's title for Browse Stashes… in this state.
    pub(super) fn title(self) -> std::borrow::Cow<'static, str> {
        const TITLE: &str = "Browse Stashes…";
        match self {
            Self::Waits(wait) => format!("{TITLE} · {wait}").into(),
            Self::Offered | Self::Refused => TITLE.into(),
        }
    }
}

/// The native menus, built for `state` ([`MenuState`]), Browse Stashes…
/// as `stashes` stands.
pub(super) fn menus(state: MenuState, stashes: StashesItem, cx: &mut App) {
    let MenuState {
        repository,
        busy,
        projects_refused,
        ..
    } = state;
    let busy = busy.is_some();
    let action = |label: &str, action: Box<dyn Action>, enabled: bool| MenuItem::Action {
        name: label.to_owned().into(),
        action,
        os_action: None,
        checked: false,
        disabled: !enabled,
    };
    cx.set_menus(vec![
        Menu::new("GitTurtle").items([
            MenuItem::action("Settings…", ShowSettings),
            MenuItem::separator(),
            MenuItem::os_submenu("Services", SystemMenuType::Services),
            MenuItem::separator(),
            MenuItem::action("Hide GitTurtle", HideApplication),
            MenuItem::action("Hide Others", HideOtherApplications),
            MenuItem::action("Show All", ShowAllApplications),
            MenuItem::separator(),
            MenuItem::action("Quit GitTurtle", Quit),
        ]),
        Menu::new("File").items([
            action("Open Repository…", Box::new(OpenRepository), !busy),
            action("Projects…", Box::new(ShowProjects), !projects_refused),
            action("Close Window", Box::new(CloseWindow), !busy),
            MenuItem::separator(),
            action(
                if cfg!(target_os = "macos") {
                    "Reveal Repository in Finder"
                } else {
                    "Reveal Repository in File Manager"
                },
                Box::new(RevealRepository),
                repository,
            ),
            action(
                "Open Repository in Editor",
                Box::new(OpenEditor),
                repository,
            ),
            action(
                "Open Repository in Terminal",
                Box::new(OpenTerminal),
                repository,
            ),
        ]),
        Menu::new("Edit").items([
            MenuItem::action("Undo", Undo),
            MenuItem::action("Redo", Redo),
            MenuItem::separator(),
            // Standard selectors reach Cocoa file-panel fields first; GPUI's
            // native delegate retains these same actions as its fallback.
            MenuItem::os_action("Cut", Cut, OsAction::Cut),
            MenuItem::os_action("Copy", Copy, OsAction::Copy),
            MenuItem::os_action("Paste", Paste, OsAction::Paste),
            MenuItem::os_action("Select All", SelectAll, OsAction::SelectAll),
        ]),
        Menu::new("View").items([
            MenuItem::action("Command Palette…", ShowCommandPalette),
            action("History", Box::new(ShowHistory), repository),
            action("Working Changes", Box::new(ShowChanges), repository),
            action(
                "Quick Open File…",
                Box::new(QuickOpenFile),
                repository && !busy,
            ),
            action(
                "Compare Revisions…",
                Box::new(CompareRevisions),
                repository && !busy,
            ),
            // Among the repository's inspectors, after Compare Revisions…,
            // as the Linux Main Menu and the palette list it.
            action(
                &stashes.title(),
                Box::new(BrowseStashes),
                stashes == StashesItem::Offered,
            ),
            MenuItem::action("GitTurtle Activity…", ShowActivity),
            action(
                "Refresh Local State",
                Box::new(Refresh),
                repository && !busy,
            ),
            MenuItem::separator(),
            action("Back", Box::new(BackHistory), repository),
            action("Toggle Sidebar", Box::new(ToggleSidebar), repository),
            MenuItem::action("Toggle Project List", ToggleProjectPane),
            MenuItem::action("Search Commits", Search),
        ]),
        Menu::new("Window").items([
            MenuItem::action("Minimize", MinimizeWindow),
            MenuItem::action("Zoom", ZoomWindow),
        ]),
        Menu::new("Help").items([MenuItem::action("Keyboard Shortcuts", ShortcutHelp)]),
    ]);
}

impl GitTurtle {
    /// Builds the native menus again where what they read
    /// ([`MenuState`]) has changed since this window last built them,
    /// Browse Stashes… then offered or refused for the palette's reason and
    /// waiting as the Linux Main Menu's item waits
    /// ([`Self::stashes_menu_wait`]). Called as each frame is built: what it
    /// compares is a few fields, and the palette's reason, which clones the
    /// selection's scope, is read only as they change.
    pub(super) fn update_native_menus(&mut self, window: &Window, cx: &mut App) {
        let state = self.menu_state();
        if self.menu_state == Some(state) {
            return;
        }
        let reason = self.menu_command_reason(command_palette::CommandId::Stashes, window, cx);
        let stashes = match (reason, self.stashes_menu_wait(reason)) {
            (None, _) => StashesItem::Offered,
            // The wait is the running write's label, or the refresh after
            // it once it has ended (`menu_wait`).
            (Some(_), Some(_)) => StashesItem::Waits(
                self.operation_busy
                    .unwrap_or(crate::workspace::REFRESHING_ROWS),
            ),
            (Some(_), None) => StashesItem::Refused,
        };
        menus(state, stashes, cx);
        self.menu_state = Some(state);
    }

    /// What the menus read now ([`MenuState`]): a few fields, which every
    /// reason the menu bar's items and the Linux Main Menu's rows are
    /// refused for, and every wait they state, are read from.
    pub(super) fn menu_state(&self) -> MenuState {
        MenuState {
            repository: self.repository.is_some(),
            busy: self.operation_busy,
            projects_refused: self.projects_refusal().0,
            workspace: self.page == AppPage::Repository,
            unsettled: self.write_unsettled(),
        }
    }

    /// Why Browse stashes… waits in a menu while the palette refuses it
    /// for `reason`: as the Working Changes Actions menu's item waits, for
    /// a write and through the reload after it, stating the write's label
    /// at the row's end and "available when … finishes" in its name. Its
    /// reason is the palette's, the write's while one refuses it; refused
    /// for a reason of its own, as on another page, it states no wait. The
    /// Linux Main Menu's row and the menu bar's View ▸ Browse Stashes… both
    /// read it.
    pub(super) fn stashes_menu_wait(
        &self,
        reason: Option<&'static str>,
    ) -> Option<native_accessibility::MenuWait> {
        self.menu_wait().filter(|_| {
            reason == Some(crate::workspace::REFRESHING_ROWS_REASON)
                || (self.operation_busy.is_some() && reason == Some(self.busy_reason()))
        })
    }

    pub(super) fn effective_theme(&self, cx: &App) -> appearance::custom::ResolvedTheme {
        self.settings.effective_theme(cx, &self.custom_themes)
    }
    /// The one appearance application path: a Settings switch, a system
    /// appearance change and every theme-editor live-preview edit end here.
    ///
    /// Invalidation is this view's `cx.notify()` below and the Settings
    /// page's ([`settings::SettingsPage`], a cached view the palette recolors
    /// too), not `Window::refresh`. Both draw the window once, but a refresh
    /// also bars GPUI's cached-view reuse for that frame, and the twenty theme
    /// miniatures in Settings ([`settings::ThemePreviewBody`]) show their own
    /// built-in palette: a palette change recolors the window around them
    /// without altering a pixel they draw. Reusing them is most of the
    /// difference between the frame budget and a full rebuild of the picker.
    pub(super) fn apply_appearance(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // The desktop's Omarchy theme is followed while it is the selection
        // (and watched, too, while Settings shows its card).
        #[cfg(target_os = "linux")]
        appearance::omarchy::follow(
            self.settings.theme == appearance::custom::ThemeSelection::Omarchy,
            cx,
        );
        // An open theme editor shows its draft through the same path.
        match self.theme_editor.preview() {
            Some(draft) => draft.apply(draft.is_light(), None, cx),
            None => self.effective_theme(cx).apply(None, cx),
        }
        appearance::sync_text_sizes(
            self.settings.interface_text_size,
            self.settings.code_text_size,
            window,
            cx,
        );
        if let (Some(collection), Some(Content::Text { presentation, .. })) =
            (&self.patch_decoration, self.content.as_deref())
        {
            text::refresh_theme(collection, presentation, cx);
        }
        if let Some(split) = &self.split_view {
            split.update(cx, |view, cx| view.refresh_theme(cx));
        }
        self.file_history.refresh_theme(cx);
        self.revision_inspection.refresh_theme(cx);
        self.notify_settings_page(cx);
        cx.notify();
    }
    pub(super) fn open_external_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.repository.as_ref().map(|repo| repo.path().to_owned()) else {
            return;
        };
        let submitted_path = path.clone();
        let editor = self.settings.external_editor.trim().to_owned();
        let response = self
            .preferences_writer
            .submit_read(move || open_in_editor(&editor, &path));
        cx.spawn_in(window, async move |this, cx| {
            let result = response.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.repository.as_ref().map(|repo| repo.path())
                    != Some(submitted_path.as_path())
                {
                    return;
                }
                this.operation_notice = Some(match result {
                    Ok(Ok(Some(Opened::Configured))) => {
                        "Sent this repository to the configured editor.".into()
                    }
                    Ok(Ok(Some(Opened::Omarchy))) => {
                        "Sent this repository to your Omarchy editor.".into()
                    }
                    Ok(Ok(None)) => {
                        this.show_settings(window, cx);
                        EDITOR_NEEDED.into()
                    }
                    Ok(Err(error)) => error.to_string(),
                    Err(_) => "The editor launcher stopped before reporting a result.".into(),
                });
                cx.notify();
            });
        })
        .detach();
    }
    pub(super) fn open_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.repository.as_ref().map(|repo| repo.path().to_owned()) else {
            return;
        };
        let submitted_path = path.clone();
        let response = self
            .preferences_writer
            .submit_read(move || open_terminal_at(&path));
        cx.spawn_in(window, async move |this, cx| {
            let result = response.await;
            let _ = this.update_in(cx, |this, _, cx| {
                if this.repository.as_ref().map(|repo| repo.path())
                    != Some(submitted_path.as_path())
                {
                    return;
                }
                this.operation_notice = Some(match result {
                    Ok(Ok(())) => "Opened a terminal in this repository.".into(),
                    Ok(Err(error)) => error.to_string(),
                    Err(_) => "The terminal launcher stopped before reporting a result.".into(),
                });
                cx.notify();
            });
        })
        .detach();
    }
    pub(super) fn shortcut_help(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        shortcuts::open_help(&mut self.dialog_hints, window, cx);
    }
    pub(super) fn about(&self, window: &mut Window, cx: &mut Context<Self>) {
        // Diagnostics report fixed keys only: a custom theme reports its base,
        // so a user-chosen theme name never enters a copied bug report, and the
        // Omarchy theme reports "omarchy", not the desktop theme's name.
        use appearance::custom::ThemeSelection;
        let theme = match self.effective_theme(cx).selection {
            ThemeSelection::Custom(id) => ThemeSelection::BuiltIn(
                self.custom_themes
                    .iter()
                    .find(|theme| theme.id == id)
                    .map_or_else(appearance::ThemeChoice::default, |theme| theme.base),
            ),
            selection => selection,
        };
        let report = build_info::diagnostics(
            window.scale_factor(),
            theme,
            self.settings.density,
            self.settings.interface_text_size,
            self.settings.code_text_size,
        );
        let details = cx.new(|_| AboutDetails { report });
        window.open_alert_dialog(cx, move |dialog, window, _| {
            dialog
                .max_h(appearance::dialog_max_height(window))
                .title("About GitTurtle")
                .description("A native Git workspace for history inspection and everyday Git work.")
                .width(appearance::ui_size(550.))
                .child(details.clone())
                .footer(crate::tags::dialog_footer("Done", None))
        });
    }
}

struct AboutDetails {
    report: String,
}

impl AboutDetails {
    /// Copy bug diagnostics' mark, keyed by the report it copies.
    fn diagnostics_mark(&self) -> copy_feedback::Mark {
        copy_feedback::Mark::new("copy-bug-diagnostics", &self.report)
    }
}

impl Render for AboutDetails {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let revision = &build_info::REVISION[..build_info::REVISION.len().min(12)];
        crate::native_accessibility::clips_children(div()
            .id("about-gitturtle-details")
            .max_h((window.viewport_size().height - appearance::ui_size(230.)).max(px(100.)))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_2()
            .text_size(appearance::ui_text(12.))
            .child(
                native_accessibility::static_text(
                    div().id("about-gitturtle-version"),
                    build_info::summary(),
                )
                .child(format!("Version {} · Preview", build_info::VERSION)),
            )
            .child(
                div().text_color(rgb(palette(cx).muted))
                    .child(format!("Source {revision} · {}", build_info::TREE)),
            )
            .child(
                div().text_color(rgb(palette(cx).muted))
                    .child(format!("{} · {} build", build_info::TARGET, build_info::PROFILE)),
            )
            .child(
                div().text_color(rgb(palette(cx).muted))
                    .child("Bug diagnostics include build and display settings. Repository paths, file content and account details are omitted."),
            )
            .child(
                copy_feedback::copy_button(self.diagnostics_mark(), "Copy bug diagnostics", "", cx)
                    .on_click(cx.listener(|this, _, window, cx| {
                        let report = this.report.clone();
                        copy_feedback::copy(window, cx, Some(this.diagnostics_mark()), "bug diagnostics", report);
                    })),
            ))
    }
}

/// Where `open_in_editor` sent a folder.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Opened {
    /// The editor named in Settings.
    Configured,
    /// The editor Omarchy launches (`omarchy launch editor`).
    Omarchy,
}

/// Shown, with Settings, when no editor can open a folder.
pub(super) const EDITOR_NEEDED: &str = if cfg!(target_os = "macos") {
    "Choose your editor in Settings → Projects, using its application name."
} else {
    "Choose your editor in Settings → Projects, using its executable name or path."
};

/// Hands `path` to the person's editor: the one named in Settings, else on
/// Omarchy the editor `omarchy launch editor` opens, which runs a terminal
/// editor such as Neovim in the default terminal. `None` when neither is
/// available. It searches the `PATH` and spawns, so it runs on a worker.
pub(super) fn open_in_editor(configured: &str, path: &Path) -> anyhow::Result<Option<Opened>> {
    let Some((command, opened)) = editor_command(configured, path, &LaunchEnvironment::current())
    else {
        return Ok(None);
    };
    launch(
        command,
        "Could not open editor",
        "macOS could not open the configured editor. Check its application name or choose the installed .app path in Settings.",
    )?;
    Ok(Some(opened))
}

/// Opens a terminal in `path`: Terminal on macOS; on Linux the desktop's
/// default terminal through `xdg-terminal-exec` (the default-terminal
/// specification Omarchy and current GNOME follow), else `$TERMINAL`, else
/// Debian's `x-terminal-emulator`. Runs on a worker.
pub(super) fn open_terminal_at(path: &Path) -> anyhow::Result<()> {
    launch(
        terminal_command(path, &LaunchEnvironment::current())?,
        "Could not open a terminal",
        "macOS could not open Terminal.",
    )
}

/// What a launcher reads from the process environment.
struct LaunchEnvironment {
    path: Option<std::ffi::OsString>,
    terminal: Option<std::ffi::OsString>,
    /// Where Omarchy 4 and 3 install their commands, for a `PATH` without them.
    omarchy: Vec<PathBuf>,
}

impl LaunchEnvironment {
    fn current() -> Self {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|home| home.is_absolute());
        Self {
            path: std::env::var_os("PATH"),
            terminal: std::env::var_os("TERMINAL").filter(|value| !value.is_empty()),
            omarchy: std::iter::once(PathBuf::from("/usr/share/omarchy/bin"))
                .chain(home.map(|home| home.join(".local/share/omarchy/bin")))
                .collect(),
        }
    }

    /// The first executable named `program` in the `PATH`.
    fn find(&self, program: &str) -> Option<PathBuf> {
        std::env::split_paths(self.path.as_ref()?)
            .filter(|directory| directory.is_absolute())
            .map(|directory| directory.join(program))
            .find(|candidate| is_executable(candidate))
    }
}

fn editor_command(
    configured: &str,
    path: &Path,
    environment: &LaunchEnvironment,
) -> Option<(std::process::Command, Opened)> {
    let (mut command, opened) = if !configured.is_empty() {
        let command = if cfg!(target_os = "macos") {
            let mut command = std::process::Command::new("/usr/bin/open");
            command.args(["-a", configured, "--"]);
            command
        } else {
            std::process::Command::new(configured)
        };
        (command, Opened::Configured)
    } else {
        (omarchy_editor(environment)?, Opened::Omarchy)
    };
    // A terminal editor such as Neovim works in its working directory.
    command.arg(path).current_dir(path);
    Some((command, opened))
}

fn terminal_command(
    path: &Path,
    environment: &LaunchEnvironment,
) -> anyhow::Result<std::process::Command> {
    let mut command = if cfg!(target_os = "macos") {
        let mut command = std::process::Command::new("/usr/bin/open");
        command.args(["-a", "Terminal", "--"]).arg(path);
        command
    } else if let Some(program) = environment.find("xdg-terminal-exec") {
        let mut command = std::process::Command::new(program);
        let mut directory = std::ffi::OsString::from("--dir=");
        directory.push(path);
        command.arg(directory);
        command
    } else if let Some(terminal) = &environment.terminal {
        std::process::Command::new(terminal)
    } else if let Some(program) = environment.find("x-terminal-emulator") {
        std::process::Command::new(program)
    } else {
        anyhow::bail!(
            "No default terminal was found. Install xdg-terminal-exec, or set the TERMINAL environment variable."
        );
    };
    command.current_dir(path);
    Ok(command)
}

/// Starts `command` in its own process group with no standard streams. macOS's
/// `open` reports within 10 s whether the application opened; on Linux the
/// launched program may keep running, so it is only reaped.
fn launch(
    mut command: std::process::Command,
    spawn_failure: &str,
    macos_failure: &str,
) -> anyhow::Result<()> {
    use std::os::unix::process::CommandExt as _;
    let mut child = command
        .process_group(0)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|error| anyhow::anyhow!("{spawn_failure}: {error}"))?;
    if cfg!(target_os = "macos") {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            if let Some(status) = child.try_wait()? {
                anyhow::ensure!(status.success(), "{macos_failure}");
                return Ok(());
            }
            if std::time::Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                anyhow::bail!(
                    "The macOS launcher did not report a result. Check whether it opened before trying again."
                );
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// Omarchy's `omarchy-launch-editor`, which `omarchy launch editor` runs, on
/// the `PATH` or where Omarchy 4 or 3 installs it. Its directory leads the
/// child's `PATH`, since the script calls Omarchy's other commands.
fn omarchy_editor(environment: &LaunchEnvironment) -> Option<std::process::Command> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    const LAUNCHER: &str = "omarchy-launch-editor";
    let launcher = environment.find(LAUNCHER).or_else(|| {
        environment
            .omarchy
            .iter()
            .map(|directory| directory.join(LAUNCHER))
            .find(|candidate| is_executable(candidate))
    })?;
    let mut command = std::process::Command::new(&launcher);
    let inherited = environment.path.clone().unwrap_or_default();
    let path = std::env::join_paths(
        launcher
            .parent()
            .map(Path::to_owned)
            .into_iter()
            .chain(std::env::split_paths(&inherited)),
    );
    if let Ok(path) = path {
        command.env("PATH", path);
    }
    Some(command)
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(all(test, target_os = "linux"))]
mod launcher_tests {
    use super::*;
    use core::prelude::v1::test;
    use std::os::unix::fs::PermissionsExt as _;

    /// An executable shell script named `name` in `directory`.
    fn program(directory: &Path, name: &str, body: &str) -> PathBuf {
        std::fs::create_dir_all(directory).unwrap();
        let path = directory.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn environment(path: &[&Path], omarchy: &[&Path]) -> LaunchEnvironment {
        LaunchEnvironment {
            path: Some(std::env::join_paths(path).unwrap()),
            terminal: None,
            omarchy: omarchy.iter().map(|path| path.to_path_buf()).collect(),
        }
    }

    #[test]
    fn the_configured_editor_wins_then_omarchy_then_settings() {
        let root = tempfile::tempdir().unwrap();
        let (bin, omarchy) = (root.path().join("bin"), root.path().join("omarchy"));
        let repository = Path::new("/work/東京 repo");

        let (command, opened) =
            editor_command("code", repository, &environment(&[&bin], &[])).unwrap();
        assert_eq!(opened, Opened::Configured);
        assert_eq!(command.get_program(), "code");
        assert_eq!(command.get_args().collect::<Vec<_>>(), [repository]);

        assert!(editor_command("", repository, &environment(&[&bin], &[&omarchy])).is_none());
        // Not executable, so not a launcher.
        std::fs::create_dir_all(&omarchy).unwrap();
        std::fs::write(omarchy.join("omarchy-launch-editor"), "").unwrap();
        assert!(editor_command("", repository, &environment(&[&bin], &[&omarchy])).is_none());

        // Omarchy's own directory, outside the PATH, leads the launcher's PATH.
        let launcher = program(&omarchy, "omarchy-launch-editor", "exit 0");
        let (command, opened) =
            editor_command("", repository, &environment(&[&bin], &[&omarchy])).unwrap();
        assert_eq!(opened, Opened::Omarchy);
        assert_eq!(command.get_program(), launcher.as_os_str());
        assert_eq!(command.get_args().collect::<Vec<_>>(), [repository]);
        assert_eq!(command.get_current_dir(), Some(repository));
        let path = command
            .get_envs()
            .find(|(name, _)| *name == "PATH")
            .and_then(|(_, value)| value)
            .unwrap();
        assert_eq!(
            std::env::split_paths(path).collect::<Vec<_>>(),
            [omarchy.clone(), bin.clone()]
        );

        // One on the PATH comes first.
        let on_path = program(&bin, "omarchy-launch-editor", "exit 0");
        let (command, _) =
            editor_command("", repository, &environment(&[&bin], &[&omarchy])).unwrap();
        assert_eq!(command.get_program(), on_path.as_os_str());
    }

    #[test]
    fn the_terminal_is_the_desktop_default_then_terminal_then_debian_alternative() {
        let root = tempfile::tempdir().unwrap();
        let bin = root.path().join("bin");
        let repository = root.path().join("東京 repo");
        std::fs::create_dir(&repository).unwrap();

        let mut none = environment(&[&bin], &[]);
        let error = terminal_command(&repository, &none).unwrap_err();
        assert!(error.to_string().contains("xdg-terminal-exec"), "{error}");

        let alternative = program(&bin, "x-terminal-emulator", "exit 0");
        let command = terminal_command(&repository, &none).unwrap();
        assert_eq!(command.get_program(), alternative.as_os_str());
        assert_eq!(command.get_current_dir(), Some(repository.as_path()));

        none.terminal = Some("foot".into());
        assert_eq!(
            terminal_command(&repository, &none).unwrap().get_program(),
            "foot"
        );

        // The default terminal opens in the repository, by its argument and
        // its working directory.
        let record = root.path().join("record");
        let launcher = program(
            &bin,
            "xdg-terminal-exec",
            &format!(
                "printf '%s\\n%s\\n' \"$*\" \"$PWD\" > '{}'",
                record.display()
            ),
        );
        let command = terminal_command(&repository, &none).unwrap();
        assert_eq!(command.get_program(), launcher.as_os_str());
        launch(command, "spawn", "macOS").unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !record.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert_eq!(
            std::fs::read_to_string(&record).unwrap(),
            format!("--dir={0}\n{0}\n", repository.display())
        );
    }
}

#[cfg(all(test, target_os = "linux"))]
mod main_menu_tests {
    use crate::command_palette::{COMMANDS, CommandId};
    use crate::*;
    use core::prelude::v1::test;

    thread_local! {
        /// How often the Main Menu's rows have been read
        /// (`PrimaryMenu::rows`).
        pub(super) static ROWS_READ: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }

    fn rows_read() -> usize {
        ROWS_READ.with(std::cell::Cell::get)
    }

    /// The label `id` reads in the palette and the Main Menu.
    fn label(id: CommandId) -> &'static str {
        COMMANDS
            .iter()
            .find(|spec| spec.id == id)
            .expect("the command is registered")
            .label
    }

    /// Where the open menu draws the item named `name`.
    fn item(cx: &mut VisualTestContext, name: &str) -> Option<Bounds<Pixels>> {
        cx.debug_bounds(format!("menu-item: {name} · MenuItem").leak())
    }

    /// Opens the Main Menu from its button.
    fn open(cx: &mut VisualTestContext) {
        crate::tags::tests::draw(cx);
        let trigger = cx.debug_bounds("main-menu").expect("the Main Menu button");
        cx.simulate_click(trigger.center(), Modifiers::default());
        crate::tags::tests::draw(cx);
    }

    /// The Linux Main Menu offers Browse stashes…, as the palette and the
    /// Working Changes Actions menu do, right after Compare revisions…
    /// among the repository's inspectors, before the separator above the
    /// editor and terminal handoff: in a repository with a stash, at 13 and
    /// 18 pt in a 461 × 490 window, choosing it opens Stashes with focus in
    /// its list. While a write runs, and through the reload after it, it
    /// waits as the Actions menu's item does: its reason is the palette's,
    /// the row states the write's label ("Fetching…", then
    /// "Refreshing…"), its name says when it is available, and a click
    /// opens nothing. The Main Menu had no way to Stashes.
    #[gpui::test]
    async fn the_main_menu_browses_stashes_as_the_palette_does(cx: &mut TestAppContext) {
        use crate::tags::tests::{draw, git, tagged_repository, window};
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        std::fs::write(repo.path().join("notes.txt"), "edited\n").unwrap();
        git(
            repo.path(),
            &[
                "stash",
                "push",
                "--include-untracked",
                "-m",
                "kept for later",
            ],
        );
        let (app, cx) = window(cx, &repo);
        cx.simulate_resize(size(px(461.), px(490.)));
        let (stashes, quick, compare, editor) = (
            label(CommandId::Stashes),
            label(CommandId::QuickOpen),
            label(CommandId::Compare),
            label(CommandId::Editor),
        );
        let slack = px(0.5);
        for text in [13, 18] {
            let at = format!("{text} pt in 461 × 490");
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            open(cx);
            let (Some(row), Some(first), Some(before), Some(after)) = (
                item(cx, stashes),
                item(cx, quick),
                item(cx, compare),
                item(cx, editor),
            ) else {
                panic!("{at}: the Main Menu draws {quick}, {compare}, {stashes} and {editor}");
            };
            // One row's pitch after Compare revisions…, as it follows Quick
            // Open; more than one before the editor, past the separator.
            let pitch = before.top() - first.top();
            assert!(
                (row.top() - before.top() - pitch).abs() <= slack
                    && after.top() - row.top() > pitch + slack,
                "{at}: {stashes} {row:?} follows {compare} {before:?} a row's pitch \
                 ({pitch:?}) apart, before the separator above {editor} {after:?}"
            );
            cx.simulate_click(row.center(), Modifiers::default());
            draw(cx);
            let (browser, list) = app
                .read_with(cx, crate::recovery::open_stash_list)
                .unwrap_or_else(|| panic!("{at}: choosing {stashes} opens Stashes"));
            crate::recovery::StashBrowser::lands(&browser, cx);
            assert!(
                cx.debug_bounds("saved-stash-row-0").is_some(),
                "{at}: Stashes shows the saved stash"
            );
            assert!(
                cx.update(|window, cx| window
                    .focused(cx)
                    .is_some_and(|focused| focused == list || focused.contains(&list, window))),
                "{at}: focus is in Stashes"
            );
            cx.simulate_keystrokes("escape");
            draw(cx);
            assert!(
                !cx.update(|window, cx| window.has_active_dialog(cx)),
                "{at}: Escape closes Stashes"
            );

            for (state, reason, wait) in [
                (
                    "during a write",
                    crate::workspace::BUSY_REASON,
                    "available when fetching finishes",
                ),
                (
                    "through the reload after a write",
                    crate::workspace::REFRESHING_ROWS_REASON,
                    "available when refreshing finishes",
                ),
            ] {
                app.update(cx, |app, _| {
                    if reason == crate::workspace::BUSY_REASON {
                        app.operation_busy = Some("Fetching…");
                    } else {
                        app.status_task = Some(Task::ready(()));
                        app.status_after_write = Some(app.work_generation);
                    }
                });
                assert_eq!(
                    cx.update(|window, cx| app.read(cx).menu_command_reason(
                        CommandId::Stashes,
                        window,
                        cx
                    )),
                    Some(reason),
                    "{at}: {state}, its reason is the palette's"
                );
                open(cx);
                let name = format!("{stashes}, {wait}");
                assert_eq!(
                    crate::native_accessibility::waiting_names::of(stashes).as_deref(),
                    Some(name.as_str()),
                    "{at}: {state}, the row is named for its wait"
                );
                let row = cx
                    .debug_bounds(format!("menu-item: {name} · MenuItem disabled").leak())
                    .unwrap_or_else(|| {
                        panic!("{at}: {state}, the row {name:?} is drawn unavailable")
                    });
                cx.simulate_click(row.center(), Modifiers::default());
                draw(cx);
                assert!(
                    cx.debug_bounds("saved-stash-row-0").is_none()
                        && !cx.update(|window, cx| window.has_active_dialog(cx)),
                    "{at}: {state}, a click on {row:?} opens no Stashes: {:?} {}",
                    cx.debug_bounds("saved-stash-row-0"),
                    cx.update(|window, cx| window.has_active_dialog(cx))
                );
                cx.simulate_keystrokes("escape");
                draw(cx);
                app.update(cx, |app, _| {
                    app.operation_busy = None;
                    app.status_task = None;
                    app.status_after_write = None;
                });
            }
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// The Main Menu, open as a write runs, builds its rows again as the
    /// write ends and as the reload after it lands, as the Actions menu
    /// follows writes: Browse stashes…, opened waiting for "Fetching…",
    /// waits for "Refreshing…" once the fetch has ended and is offered once
    /// the reload has landed, a click then opening Stashes, and Quick Open,
    /// refused by the fetch, is offered again with it; a notify that changes
    /// nothing the rows read reads none of them. Opened from F10 with the
    /// keyboard's cursor on Quick Open, the cursor stays there, and focus in
    /// the menu, through a fetch's start, its end and its reload's landing,
    /// so Enter opens Quick Open, and the closed menu follows nothing; with
    /// the cursor on Browse stashes…, a write that starts rests it there and
    /// says once why it waits. Built only as it opened, the menu went on
    /// saying "Fetching…" after the fetch, and read every row at each notify.
    #[gpui::test]
    async fn the_main_menu_follows_a_write_while_it_is_open(cx: &mut TestAppContext) {
        use crate::tags::tests::{draw, git, tagged_repository, window};
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        std::fs::write(repo.path().join("notes.txt"), "edited\n").unwrap();
        git(
            repo.path(),
            &["stash", "push", "--include-untracked", "-m", "kept"],
        );
        let (app, cx) = window(cx, &repo);
        let (stashes, quick) = (label(CommandId::Stashes), label(CommandId::QuickOpen));
        let waiting = |cx: &mut VisualTestContext, wait: &str| {
            cx.debug_bounds(
                format!("menu-item: {stashes}, available when {wait} finishes · MenuItem disabled")
                    .leak(),
            )
        };
        let refused = |cx: &mut VisualTestContext, name: &str| {
            cx.debug_bounds(format!("menu-item: {name} · MenuItem disabled").leak())
        };
        // A write runs, its reload yet to land, or neither.
        let write = |cx: &mut VisualTestContext, busy: Option<&'static str>, reload: bool| {
            app.update(cx, |app, cx| {
                app.operation_busy = busy;
                app.status_task = reload.then(|| Task::ready(()));
                app.status_after_write = reload.then_some(app.work_generation);
                cx.notify();
            });
            draw(cx);
        };

        write(cx, Some("Fetching…"), false);
        open(cx);
        assert!(
            waiting(cx, "fetching").is_some() && refused(cx, quick).is_some(),
            "opened during the fetch, {stashes} waits for it and {quick} is refused"
        );
        // A notify that changes nothing the rows read, as a fetch's progress
        // notices, reads no row; the fetch's end reads them again.
        let before = rows_read();
        app.update(cx, |_, cx| cx.notify());
        draw(cx);
        assert_eq!(
            rows_read(),
            before,
            "a notify that changes nothing the rows read reads none"
        );
        write(cx, None, true);
        assert!(rows_read() > before, "the fetch's end reads the rows again");
        assert!(
            waiting(cx, "fetching").is_none() && waiting(cx, "refreshing").is_some(),
            "once the fetch has ended, {stashes} waits for its reload: {:?} {:?}",
            waiting(cx, "fetching"),
            waiting(cx, "refreshing")
        );
        assert!(
            item(cx, quick).is_some(),
            "{quick}, which only the fetch refused, is offered"
        );
        write(cx, None, false);
        let row = item(cx, stashes)
            .unwrap_or_else(|| panic!("once the reload has landed, {stashes} is offered"));
        cx.simulate_click(row.center(), Modifiers::default());
        draw(cx);
        assert!(
            cx.update(|window, cx| window.has_active_dialog(cx))
                && cx.debug_bounds("saved-stash-row-0").is_some(),
            "the menu built again opens Stashes"
        );
        cx.simulate_keystrokes("escape");
        draw(cx);

        // From the keyboard: Open, Projects, History, Changes, then Quick
        // Open, which the cursor keeps as the menu is built again, resting
        // there while the fetch refuses it, back on it as the fetch ends and
        // on it still as the reload lands, focus in the menu throughout;
        // Enter then opens Quick Open, and the closed menu follows nothing.
        cx.update(|_, cx| crate::shortcuts::bind_keys(cx));
        let primary = app.read_with(cx, |app, _| app.primary_menu.clone());
        let in_menu = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                primary
                    .read(cx)
                    .menu
                    .as_ref()
                    .is_some_and(|menu| menu.focus_handle(cx).contains_focused(window, cx))
            })
        };
        cx.simulate_keystrokes("f10");
        draw(cx);
        for _ in 0..4 {
            cx.simulate_keystrokes("down");
        }
        draw(cx);
        for (busy, reload) in [(Some("Fetching…"), false), (None, true), (None, false)] {
            let before = rows_read();
            write(cx, busy, reload);
            assert!(
                rows_read() > before && in_menu(cx),
                "built again ({busy:?}, reload {reload}), focus stays in the menu"
            );
        }
        cx.simulate_keystrokes("enter");
        draw(cx);
        assert!(
            cx.debug_bounds("quick-open-results").is_some(),
            "Enter on the row the cursor kept opens {quick}"
        );
        assert!(
            primary.read_with(cx, |menu, _| menu.menu.is_none() && menu._follows.is_none()),
            "the closed menu follows nothing"
        );
        cx.simulate_keystrokes("escape");
        draw(cx);

        // From the keyboard: Open, Projects, History, Changes, Quick Open,
        // Compare revisions…, then Browse stashes….
        cx.simulate_keystrokes("f10");
        draw(cx);
        for _ in 0..6 {
            cx.simulate_keystrokes("down");
        }
        draw(cx);
        write(cx, Some("Fetching…"), false);
        assert_eq!(
            crate::copy_feedback::tests::announced(cx).map(|(_, said)| said),
            Some(format!("{stashes}, available when fetching finishes")),
            "the cursor resting on {stashes} says why it waits"
        );
        write(cx, None, false);
    }
}

/// The menu bar's menus, as macOS draws them; built on Linux as well,
/// where nothing draws them, so these run on both.
#[cfg(test)]
mod menu_bar_tests {
    use crate::command_palette::CommandId;
    use crate::*;
    use core::prelude::v1::test;

    /// The View menu's items, by title, each with whether it was built
    /// refused.
    fn view_menu(cx: &mut VisualTestContext) -> Vec<(String, bool)> {
        cx.update(|_, cx| cx.get_menus())
            .unwrap_or_default()
            .into_iter()
            .find(|menu| menu.name.as_ref() == "View")
            .expect("the View menu")
            .items
            .into_iter()
            .filter_map(|item| match item {
                gpui::OwnedMenuItem::Action { name, disabled, .. } => Some((name, disabled)),
                _ => None,
            })
            .collect()
    }

    /// The menu bar's View menu offers Browse Stashes… right after Compare
    /// Revisions…, as the Linux Main Menu and the palette do, and choosing
    /// it opens Stashes with focus in its list. While a write runs, and
    /// through the reload after it, it is refused and its title states the
    /// write it waits for, "Browse Stashes… · Fetching…", then
    /// "· Refreshing…", the palette's reason refusing it; on another page
    /// it is refused with no wait; refused, choosing it opens nothing,
    /// though macOS may offer it. Once the write has landed it is offered
    /// again. The menu bar had no way to Stashes.
    #[gpui::test]
    async fn the_menu_bar_browses_stashes_as_the_main_menu_does(cx: &mut TestAppContext) {
        use crate::tags::tests::{draw, git, tagged_repository, window};
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        std::fs::write(repo.path().join("notes.txt"), "edited\n").unwrap();
        git(
            repo.path(),
            &["stash", "push", "--include-untracked", "-m", "kept"],
        );
        let (app, cx) = window(cx, &repo);
        const TITLE: &str = "Browse Stashes…";
        let stashes = |cx: &mut VisualTestContext| {
            let items = view_menu(cx);
            let at = items
                .iter()
                .position(|(name, _)| name.starts_with(TITLE))
                .unwrap_or_else(|| panic!("the View menu offers {TITLE}: {items:?}"));
            assert_eq!(
                items[at - 1].0,
                "Compare Revisions…",
                "{TITLE} follows Compare Revisions…: {items:?}"
            );
            items[at].clone()
        };
        let palette = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                app.read(cx)
                    .menu_command_reason(CommandId::Stashes, window, cx)
            })
        };
        // Chosen as macOS chooses it: dispatched where the window's focus
        // is, inside the app's view. Whether Stashes then shows.
        let opens = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                let focus = app.read(cx).app_focus.clone();
                if !focus.contains_focused(window, cx) {
                    focus.focus(window, cx);
                }
            });
            cx.dispatch_action(BrowseStashes);
            draw(cx);
            cx.update(|window, cx| window.has_active_dialog(cx))
                || cx.debug_bounds("saved-stash-row-0").is_some()
        };

        assert_eq!(stashes(cx), (TITLE.to_owned(), false), "offered at rest");
        assert!(opens(cx), "choosing it opens Stashes");
        let (browser, list) = app
            .read_with(cx, crate::recovery::open_stash_list)
            .expect("the open Stashes");
        crate::recovery::StashBrowser::lands(&browser, cx);
        assert!(
            cx.debug_bounds("saved-stash-row-0").is_some(),
            "Stashes shows the saved stash"
        );
        assert!(
            cx.update(|window, cx| window
                .focused(cx)
                .is_some_and(|focused| focused == list || focused.contains(&list, window))),
            "focus is in Stashes"
        );
        cx.simulate_keystrokes("escape");
        draw(cx);
        assert!(
            !cx.update(|window, cx| window.has_active_dialog(cx))
                && cx.debug_bounds("saved-stash-row-0").is_none(),
            "Escape closes Stashes"
        );

        for (state, reason, title) in [
            (
                "during a write",
                crate::workspace::BUSY_REASON,
                format!("{TITLE} · Fetching…"),
            ),
            (
                "through the reload after a write",
                crate::workspace::REFRESHING_ROWS_REASON,
                format!("{TITLE} · Refreshing…"),
            ),
            (
                "on another page",
                "Return to the repository workspace first",
                TITLE.to_owned(),
            ),
        ] {
            app.update(cx, |app, cx| {
                match reason {
                    crate::workspace::BUSY_REASON => app.operation_busy = Some("Fetching…"),
                    crate::workspace::REFRESHING_ROWS_REASON => {
                        app.status_task = Some(Task::ready(()));
                        app.status_after_write = Some(app.work_generation);
                    }
                    _ => app.page = AppPage::Settings,
                }
                cx.notify();
            });
            draw(cx);
            assert_eq!(palette(cx), Some(reason), "{state}: the palette's reason");
            assert_eq!(stashes(cx), (title, true), "{state}: refused, saying why");
            assert!(!opens(cx), "{state}: choosing it opens nothing");
            app.update(cx, |app, cx| {
                app.operation_busy = None;
                app.status_task = None;
                app.status_after_write = None;
                app.page = AppPage::Repository;
                cx.notify();
            });
            draw(cx);
            assert_eq!(
                stashes(cx),
                (TITLE.to_owned(), false),
                "{state}: offered again once it ends"
            );
        }
    }
}
