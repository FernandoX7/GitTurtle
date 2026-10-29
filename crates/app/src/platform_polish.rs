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
    /// Whether the trigger shows "Menu" beside its icon, as the tab strip
    /// decides each frame (`GitTurtle::tab_strip_labels`) before this view
    /// renders inside it.
    labelled: bool,
    /// The trigger's bounds as the last frame painted them, below which the
    /// menu opens.
    trigger: std::rc::Rc<std::cell::Cell<Option<Bounds<Pixels>>>>,
}

#[cfg(target_os = "linux")]
impl PrimaryMenu {
    pub(super) fn new(owner: WeakEntity<GitTurtle>) -> Self {
        Self {
            owner,
            menu: None,
            return_focus: None,
            _dismiss: None,
            _focus_out: None,
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

    pub(super) fn toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.menu.is_some() {
            self.close(window, cx);
            return;
        }
        if window.has_active_dialog(cx) || window.has_active_sheet(cx) {
            return;
        }
        use command_palette::{COMMANDS, CommandId};
        use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
        let Some(owner) = self.owner.upgrade() else {
            return;
        };
        let path = owner.read(cx).path.clone();
        let focus = window
            .focused(cx)
            .unwrap_or_else(|| owner.read(cx).app_focus.clone());
        self.return_focus = Some(focus.clone());
        let rows = [
            Some(CommandId::Open),
            Some(CommandId::Projects),
            None,
            Some(CommandId::History),
            Some(CommandId::Changes),
            Some(CommandId::QuickOpen),
            Some(CommandId::Compare),
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
        .map(|id| {
            id.map(|id| {
                let spec = *COMMANDS
                    .iter()
                    .find(|spec| spec.id == id)
                    .expect("menu command is registered");
                (spec, owner.read(cx).menu_command_reason(id, window))
            })
        });
        let target = self.owner.clone();
        // The menu opens 4 px below its trigger (the popup's top padding),
        // and from F10, Enter or Space with its first command under the
        // keyboard's cursor (`native_accessibility::open_menu`).
        let top = self
            .trigger
            .get()
            .map(|trigger| trigger.bottom() + appearance::ui_size(4.));
        let keyboard = window.last_input_was_keyboard();
        let menu = PopupMenu::build(window, cx, move |mut menu, window, cx| {
            menu = menu
                .action_context(focus.clone())
                .min_w(appearance::ui_size(305.))
                .max_w((window.viewport_size().width - px(24.)).min(appearance::ui_size(390.)));
            for row in &rows {
                let Some((spec, reason)) = row else {
                    menu = menu.separator();
                    continue;
                };
                let command = spec.id;
                let target = target.clone();
                let path = path.clone();
                let return_focus = focus.clone();
                let mut item = PopupMenuItem::new(spec.label).disabled(reason.is_some());
                if let Some(action) = spec.shortcut.and_then(shortcuts::action) {
                    item = item.action(action);
                }
                menu = menu.item(item.on_click(move |_, window, cx| {
                    // Restore the originating editor before opening the workflow;
                    // modal return focus must never capture this transient menu.
                    return_focus.focus(window, cx);
                    let _ = target.update(cx, |this, cx| {
                        this.run_menu_command(command, &path, window, cx)
                    });
                }));
            }
            native_accessibility::open_menu(menu, keyboard, top, None, window, cx)
        });
        self._dismiss = Some(
            cx.subscribe_in(&menu, window, |this, _, _: &DismissEvent, _, cx| {
                this.menu = None;
                cx.notify();
            }),
        );
        let menu_focus = menu.focus_handle(cx);
        self._focus_out = Some(cx.on_focus_out(&menu_focus, window, |this, _, _, cx| {
            // A newly focused control or dialog owns focus now; remove the
            // transient menu without restoring over that destination.
            this.menu = None;
            cx.notify();
        }));
        menu_focus.focus(window, cx);
        self.menu = Some(menu);
        cx.notify();
    }

    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
        .when_some(self.menu.clone(), |popup, menu| {
            popup.content(div().pt(appearance::ui_size(4.)).child(menu))
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

pub(super) fn menus(repository: bool, busy: bool, cx: &mut App) {
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
            action("Projects…", Box::new(ShowProjects), !busy),
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
                .button_props(
                    gpui_kit::component::dialog::DialogButtonProps::default().ok_text("Done"),
                )
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
