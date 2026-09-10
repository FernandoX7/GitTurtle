//! Native accessibility semantics, keyboard navigation, and local OS choices.
//! No setting is changed here: macOS display preferences are read on startup
//! and on workspace display-change notifications, then applied to the toolkit's
//! motion policy. Foreground activation remains a fallback read.
use crate::*;

#[cfg(any(target_os = "macos", test))]
mod display_changes;

pub(super) fn observe_display_preferences(cx: &mut Context<GitTurtle>) -> Option<Task<()>> {
    #[cfg(target_os = "macos")]
    {
        Some(display_changes::subscribe(cx))
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = cx;
        None
    }
}

gpui_kit::actions!(
    gitturtle,
    [
        NextNavigation,
        PreviousNavigation,
        FirstNavigation,
        LastNavigation,
        ExpandNavigation,
        CollapseNavigation,
        ActivateNavigation,
        ManageNavigation
    ]
);

pub(super) fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("down", NextNavigation, Some("GitTurtleNavigation")),
        KeyBinding::new("up", PreviousNavigation, Some("GitTurtleNavigation")),
        KeyBinding::new("home", FirstNavigation, Some("GitTurtleNavigation")),
        KeyBinding::new("end", LastNavigation, Some("GitTurtleNavigation")),
        KeyBinding::new("right", ExpandNavigation, Some("GitTurtleNavigation")),
        KeyBinding::new("left", CollapseNavigation, Some("GitTurtleNavigation")),
        KeyBinding::new("enter", ActivateNavigation, Some("GitTurtleNavigation")),
        KeyBinding::new("space", ActivateNavigation, Some("GitTurtleNavigation")),
        KeyBinding::new("shift-f10", ManageNavigation, Some("GitTurtleNavigation")),
    ]);
}

pub(super) fn polite(builder: &mut A11ySubtreeBuilder<'_>) {
    builder
        .parent_node()
        .set_live(gpui::accesskit::Live::Polite);
    builder.parent_node().set_live_atomic();
}
pub(super) fn assertive(builder: &mut A11ySubtreeBuilder<'_>) {
    builder
        .parent_node()
        .set_live(gpui::accesskit::Live::Assertive);
    builder.parent_node().set_live_atomic();
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct DisplayPreferences {
    pub reduce_motion: bool,
    pub increase_contrast: bool,
    pub reduce_transparency: bool,
}

#[cfg(target_os = "macos")]
pub(super) fn display_preferences() -> Option<DisplayPreferences> {
    let workspace = objc2_app_kit::NSWorkspace::sharedWorkspace();
    Some(DisplayPreferences {
        reduce_motion: workspace.accessibilityDisplayShouldReduceMotion(),
        increase_contrast: workspace.accessibilityDisplayShouldIncreaseContrast(),
        reduce_transparency: workspace.accessibilityDisplayShouldReduceTransparency(),
    })
}
#[cfg(not(target_os = "macos"))]
pub(super) fn display_preferences() -> Option<DisplayPreferences> {
    None
}

pub(super) fn sync_preferences(cx: &mut App) {
    if let Some(preferences) = display_preferences() {
        cx.set_reduce_motion(preferences.reduce_motion);
    }
}

pub(super) fn next_cursor(
    rows: &[NavRow],
    cursor: Option<usize>,
    forward: bool,
    boundary: bool,
) -> Option<usize> {
    if rows.is_empty() {
        return None;
    }
    let navigable = |index: usize| !matches!(rows[index], NavRow::Section(..));
    let current = cursor.filter(|index| *index < rows.len() && navigable(*index));
    if current.is_none() && !boundary {
        return (0..rows.len()).find(|index| navigable(*index));
    }
    if boundary {
        if forward {
            (0..rows.len()).rev().find(|index| navigable(*index))
        } else {
            (0..rows.len()).find(|index| navigable(*index))
        }
    } else {
        let current = current.expect("validated cursor");
        if forward {
            ((current + 1)..rows.len())
                .find(|index| navigable(*index))
                .or(Some(current))
        } else {
            (0..current)
                .rev()
                .find(|index| navigable(*index))
                .or(Some(current))
        }
    }
}
impl GitTurtle {
    pub(super) fn move_navigation(
        &mut self,
        forward: bool,
        boundary: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.page != AppPage::Repository
            || window.has_active_dialog(cx)
            || window.has_active_sheet(cx)
        {
            return;
        }
        self.nav_cursor = next_cursor(&self.nav_rows, self.nav_cursor, forward, boundary);
        if let Some(index) = self.nav_cursor {
            self.nav_scroll
                .scroll_to_item(index, ScrollStrategy::Center);
        }
        window.focus(&self.nav_focus, cx);
        cx.notify();
    }
    pub(super) fn expand_navigation(
        &mut self,
        expand: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if window.has_active_dialog(cx) || window.has_active_sheet(cx) {
            return;
        }
        let Some(index) = self.nav_cursor else {
            return;
        };
        let Some(row) = self.nav_rows.get(index).cloned() else {
            return;
        };
        if let NavRow::Folder { key, expanded, .. } = &row
            && *expanded != expand
        {
            if expand {
                self.expanded_folders.insert(key.clone());
            } else {
                self.expanded_folders.remove(key);
            }
            self.rebuild_navigation(cx);
            cx.notify();
            return;
        }
        let depth = match row {
            NavRow::Folder { depth, .. } | NavRow::Branch(_, depth) => depth,
            _ => return,
        };
        if expand {
            if matches!(self.nav_rows.get(index+1),Some(NavRow::Folder {depth:next,..})|Some(NavRow::Branch(_,next)) if *next>depth){self.move_navigation(true,false,window,cx);}
        }else if depth>0 && let Some(parent)=(0..index).rev().find(|index|matches!(self.nav_rows[*index],NavRow::Folder {depth:parent,..} if parent<depth)) {
            self.nav_cursor=Some(parent);self.nav_scroll.scroll_to_item(parent,ScrollStrategy::Center);cx.notify();
        }
    }
    pub(super) fn activate_navigation(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.page != AppPage::Repository
            || window.has_active_dialog(cx)
            || window.has_active_sheet(cx)
        {
            return;
        }
        let Some(row) = self.nav_rows.get(index).cloned() else {
            return;
        };
        self.nav_cursor = Some(index);
        self.limit = 500;
        match row {
            NavRow::Folder { key, .. } => {
                if !self.expanded_folders.remove(&key) {
                    self.expanded_folders.insert(key);
                }
                self.rebuild_navigation(cx);
            }
            NavRow::All => {
                if let Some(path) = self.path.clone() {
                    self.open(path, None, window, cx);
                }
            }
            NavRow::Branch(index, _) => {
                if let Some(path) = self.path.clone() {
                    let branch = &self.branches[index];
                    self.open(
                        path,
                        Some((
                            branch.name.clone(),
                            worker::Scope::Branch {
                                name: branch.name.clone(),
                                remote: branch.remote,
                            },
                        )),
                        window,
                        cx,
                    );
                }
            }
            NavRow::Worktree(index) => {
                let tree = &self.worktrees[index];
                let path = tree.path.clone();
                let scope = Some((
                    tree.branch.clone().unwrap_or("Detached worktree".into()),
                    worker::Scope::Worktree { path: path.clone() },
                ));
                self.open(path, scope, window, cx);
            }
            NavRow::Section(..) => return,
        }
        window.focus(&self.nav_focus, cx);
        cx.notify();
    }
    pub(super) fn manage_navigation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if window.has_active_dialog(cx)
            || window.has_active_sheet(cx)
            || self.operation_busy.is_some()
        {
            return;
        }
        let Some(NavRow::Branch(index, _)) =
            self.nav_cursor.and_then(|index| self.nav_rows.get(index))
        else {
            return;
        };
        let branch = &self.branches[*index];
        self.open_contextual_branch(branch.name.clone(), branch.remote, window, cx);
    }
}

#[cfg(test)]
mod control_tests;
#[cfg(test)]
mod dialog_tests;
#[cfg(test)]
mod input_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    #[gpui::test]
    fn rendered_buttons_report_disabled_and_tab_selection(cx: &mut TestAppContext) {
        use gpui::Element as _;
        use gpui_kit::base::Button as BaseButton;
        type Nodes = Arc<std::sync::Mutex<Vec<gpui::accesskit::Node>>>;
        struct Probe(Nodes);
        impl Render for Probe {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                let nodes = self.0.clone();
                canvas(
                    move |_, window, cx| {
                        let mut capture = |id: &'static str, disabled, role, selected| {
                            let button = BaseButton::new(id)
                                .disabled(disabled)
                                .role(role)
                                .selected(selected)
                                .accessibility_label("Send review")
                                .on_click(|_, _, _| {});
                            let element = RenderOnce::render(button, window, cx).into_element();
                            let mut node = gpui::accesskit::Node::new(element.a11y_role().unwrap());
                            element.write_a11y_info(&mut node);
                            node
                        };
                        *nodes.lock().unwrap() = vec![
                            capture("enabled", false, Role::Button, false),
                            capture("disabled", true, Role::Button, false),
                            capture("selected-tab", false, Role::Tab, true),
                            capture("unselected-tab", false, Role::Tab, false),
                            capture("selected-button", false, Role::Button, true),
                        ];
                    },
                    |_, _, _, _| {},
                )
            }
        }
        let nodes: Nodes = Arc::new(std::sync::Mutex::new(Vec::new()));
        let observed = nodes.clone();
        let (_, cx) = cx.add_window_view(move |_, _| Probe(nodes));
        cx.update(|window, cx| {
            window.draw(cx).clear(cx);
        });
        let nodes = observed.lock().unwrap();
        let [
            enabled,
            disabled,
            selected_tab,
            unselected_tab,
            selected_button,
        ] = nodes.as_slice()
        else {
            panic!("five rendered nodes expected");
        };
        assert_eq!(enabled.role(), Role::Button);
        assert_eq!(disabled.role(), Role::Button);
        assert_eq!(enabled.label(), Some("Send review"));
        assert_eq!(disabled.label(), Some("Send review"));
        assert!(!enabled.is_disabled());
        assert!(disabled.is_disabled());
        assert!(enabled.supports_action(gpui::accesskit::Action::Click));
        assert!(!disabled.supports_action(gpui::accesskit::Action::Click));
        assert_eq!(selected_tab.role(), Role::Tab);
        assert_eq!(unselected_tab.role(), Role::Tab);
        assert_eq!(selected_tab.is_selected(), Some(true));
        assert_eq!(unselected_tab.is_selected(), Some(false));
        assert_eq!(selected_tab.toggled(), None);
        assert_eq!(unselected_tab.toggled(), None);
        assert_eq!(selected_button.is_selected(), None);
        assert_eq!(selected_button.toggled(), None);
    }

    #[gpui::test]
    fn focus_traps_preserve_named_dialog_and_group_semantics(cx: &mut TestAppContext) {
        use gpui::Element as _;
        use gpui_kit::base::FocusTrapElement as _;
        cx.update(|cx| {
            let focus = cx.focus_handle();
            for role in [Role::Dialog, Role::AlertDialog, Role::Group] {
                let element = div()
                    .id("captured-dialog")
                    .role(role)
                    .aria_label("Review captured destination")
                    .aria_description("Only this reviewed repository is affected")
                    .focus_trap("dialog-trap", &focus);
                let mut node = gpui::accesskit::Node::new(element.a11y_role().unwrap());
                element.write_a11y_info(&mut node);
                assert_eq!(node.role(), role);
                assert_eq!(node.label(), Some("Review captured destination"));
                assert_eq!(
                    node.description(),
                    Some("Only this reviewed repository is affected")
                );
                assert_eq!(node.is_modal(), role != Role::Group);
            }
        });
    }

    #[test]
    fn navigator_skips_sections_stops_at_bounds_and_recovers_removed_cursor() {
        let rows = vec![
            NavRow::All,
            NavRow::Section("Branches", 2),
            NavRow::Branch(0, 0),
            NavRow::Branch(1, 0),
        ];
        assert_eq!(next_cursor(&rows, Some(0), true, false), Some(2));
        assert_eq!(next_cursor(&rows, Some(2), false, false), Some(0));
        assert_eq!(next_cursor(&rows, Some(3), true, false), Some(3));
        assert_eq!(next_cursor(&rows, Some(2), false, true), Some(0));
        assert_eq!(next_cursor(&rows, Some(0), true, true), Some(3));
        assert_eq!(next_cursor(&rows, Some(99), false, false), Some(0));
        assert_eq!(next_cursor(&rows, None, true, false), Some(0));
        assert_eq!(next_cursor(&[], None, true, false), None);
    }
}
