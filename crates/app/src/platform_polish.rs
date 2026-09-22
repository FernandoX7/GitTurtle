//! Native conventions and explicitly initiated local application handoff.
use crate::*;
use gpui_kit::component::{
    WindowExt,
    input::{Copy, Cut, Paste, Redo, SelectAll, Undo},
};

gpui_kit::actions!(
    gitturtle_platform,
    [
        ShowHistory,
        RevealRepository,
        OpenEditor,
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
        }
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
                (spec, owner.read(cx).menu_command_reason(id))
            })
        });
        let target = self.owner.clone();
        let menu = PopupMenu::build(window, cx, move |mut menu, window, _| {
            menu = menu
                .action_context(focus.clone())
                .min_w(appearance::ui_size(305.))
                .max_w((window.viewport_size().width - px(24.)).min(appearance::ui_size(390.)))
                .max_h((window.viewport_size().height - appearance::ui_size(70.)).max(px(120.)))
                .scrollable(true);
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
            menu
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
        use gpui_kit::{base::Popup, component::IconName, prelude::FluentBuilder};
        // Popup owns anchoring only. Keeping focus and opening in this entity
        // avoids a popover shell taking focus before we capture the editor.
        let mut popup = Popup::new(
            "gitturtle-primary-menu",
            button("main-menu", "Menu", "", self.menu.is_some())
                .icon(Icon::new(IconName::Menu).size(appearance::ui_size(16.)))
                .accessibility_label("Main Menu")
                .tooltip(format!(
                    "Main Menu · {}",
                    shortcuts::label(shortcuts::ShortcutId::MainMenu)
                ))
                .on_click(cx.listener(|this, _, window, cx| this.toggle(window, cx))),
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
        self.settings
            .resolved_theme(cx.window_appearance(), &self.custom_themes)
    }
    /// The one appearance application path: a Settings switch, a system
    /// appearance change and every theme-editor live-preview edit end here.
    ///
    /// Invalidation is this view's `cx.notify()` below, not
    /// `Window::refresh`. Both draw the window once, but a refresh also bars
    /// GPUI's cached-view reuse for that frame, and the twenty theme
    /// miniatures in Settings ([`settings::ThemePreviewBody`]) show their own
    /// built-in palette: a palette change recolors the window around them
    /// without altering a pixel they draw. Reusing them is most of the
    /// difference between the frame budget and a full rebuild of the picker.
    pub(super) fn apply_appearance(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
        cx.notify();
    }
    pub(super) fn open_external_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.repository.as_ref().map(|repo| repo.path().to_owned()) else {
            return;
        };
        let submitted_path = path.clone();
        let editor = self.settings.external_editor.trim().to_owned();
        if editor.is_empty() {
            self.show_settings(window, cx);
            self.operation_notice = Some("Choose your editor in Settings → Projects. Use its application name on macOS or executable path on Linux.".into());
            return;
        }
        let response = self.preferences_writer.submit_read(move || {
            let mut command = if cfg!(target_os = "macos") { let mut cmd = std::process::Command::new("/usr/bin/open"); cmd.args(["-a", &editor, "--"]); cmd } else { std::process::Command::new(&editor) };
            let mut child = command.arg(path).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().map_err(|error| anyhow::anyhow!("Could not open editor: {error}"))?;
            if cfg!(target_os = "macos") {
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
                loop {
                    if let Some(status) = child.try_wait()? {
                        anyhow::ensure!(status.success(), "macOS could not open the configured editor. Check its application name or choose the installed .app path in Settings.");
                        break;
                    }
                    if std::time::Instant::now() >= deadline {
                        let _ = child.kill(); let _ = child.wait();
                        anyhow::bail!("The macOS editor launcher did not report a result. Check whether the editor opened before trying again.");
                    }
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
            } else {
                // The external editor may intentionally keep running after handoff.
                std::thread::spawn(move || { let _ = child.wait(); });
            }
            Ok(())
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = response.await;
            let _ = this.update_in(cx, |this, _, cx| {
                if this.repository.as_ref().map(|repo| repo.path())
                    != Some(submitted_path.as_path())
                {
                    return;
                }
                this.operation_notice = Some(match result {
                    Ok(Ok(())) => "Sent this repository to the configured editor.".into(),
                    Ok(Err(error)) => error.to_string(),
                    Err(_) => "The editor launcher stopped before reporting a result.".into(),
                });
                cx.notify();
            });
        })
        .detach();
    }
    pub(super) fn shortcut_help(&self, window: &mut Window, cx: &mut Context<Self>) {
        shortcuts::open_help(window, cx);
    }
    pub(super) fn about(&self, window: &mut Window, cx: &mut Context<Self>) {
        // Diagnostics report built-in keys only: a custom theme reports its base,
        // so a user-chosen theme name never enters a copied bug report.
        let theme = match self.effective_theme(cx).selection {
            appearance::custom::ThemeSelection::BuiltIn(choice) => choice,
            appearance::custom::ThemeSelection::Custom(id) => self
                .custom_themes
                .iter()
                .find(|theme| theme.id == id)
                .map_or_else(appearance::ThemeChoice::default, |theme| theme.base),
        };
        let report = build_info::diagnostics(
            window.scale_factor(),
            theme,
            self.settings.density,
            self.settings.interface_text_size,
            self.settings.code_text_size,
        );
        let details = cx.new(|_| AboutDetails {
            report,
            copied: false,
        });
        window.open_alert_dialog(cx, move |dialog, _, _| {
            dialog
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
    copied: bool,
}

impl Render for AboutDetails {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let revision = &build_info::REVISION[..build_info::REVISION.len().min(12)];
        div()
            .id("about-gitturtle-details")
            .max_h((window.viewport_size().height - appearance::ui_size(230.)).max(px(100.)))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_2()
            .text_size(appearance::ui_text(12.))
            .child(
                div().id("about-gitturtle-version").role(Role::Label)
                    .aria_label(build_info::summary())
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
                button("copy-bug-diagnostics", if self.copied { "Diagnostics copied" } else { "Copy bug diagnostics" }, "copy", false)
                    .on_click(cx.listener(|this, _, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(this.report.clone()));
                        this.copied = true;
                        cx.notify();
                    })),
            )
    }
}
