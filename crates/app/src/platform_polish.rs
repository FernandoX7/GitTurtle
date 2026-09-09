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
        MinimizeWindow,
        CloseWindow,
        ZoomWindow,
        HideApplication,
        HideOtherApplications,
        ShowAllApplications
    ]
);

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
    pub(super) fn effective_theme(&self, cx: &App) -> appearance::ThemeChoice {
        self.settings.resolved_theme(cx.window_appearance())
    }
    pub(super) fn apply_appearance(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.effective_theme(cx).apply(Some(window), cx);
        appearance::apply_text_sizes(
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
        let modifier = primary_label();
        let rows = [
            ("Command palette", "⇧P"),
            ("Quick Open File", "P"),
            ("Open repository", "O"),
            ("Projects", "⇧O"),
            ("History", "1"),
            ("Working Changes", "2"),
            ("Refresh local state", "R"),
            ("Settings", ","),
            ("Back to retained context", "["),
            ("Toggle navigation", "B"),
            ("Find in focused list or editor", "F"),
        ];
        window.open_alert_dialog(cx, move |dialog, _, cx| dialog.title("Keyboard shortcuts").description("Move through controls with Tab and Shift-Tab. Activate buttons with Space or Return.")
            .child(div().flex().flex_col().gap_2().children(rows.iter().enumerate().map(|(i, (label, key))| div().id(("shortcut-help", i)).role(Role::Label).aria_label(format!("{label}: {modifier}{key}")).flex().justify_between().gap_6().child(*label).child(div().font_family(mono()).child(format!("{modifier}{key}")))))
                .child(div().id("shortcut-navigation-help").role(Role::Label).aria_label("Lists: arrow keys, Home and End, Return to open. Image controls: Tab to zoom, pan and comparison amount; Space to adjust. Escape closes the current view or returns to retained context.").pt_3().text_color(rgb(palette(cx).muted)).child("Lists: ↑ / ↓, Home / End, Return to open. Image controls: Tab to zoom, pan and comparison amount; Space to adjust. Escape closes the current transient view or returns to retained context.")))
            .button_props(gpui_kit::component::dialog::DialogButtonProps::default().ok_text("Done")));
    }
}
