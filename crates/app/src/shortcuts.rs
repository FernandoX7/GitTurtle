//! The app-owned bindings are also the source for menu, palette and help labels.
//! Toolkit text-editing bindings keep their own focused input contexts.
use crate::*;
use gpui_kit::component::kbd::Kbd;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum ShortcutId {
    Palette,
    QuickOpen,
    Open,
    NewTab,
    Projects,
    History,
    Changes,
    Compare,
    Activity,
    Settings,
    Help,
    #[cfg(target_os = "linux")]
    MainMenu,
    Quit,
    Minimize,
    CloseWindow,
    CloseTab,
    NextTab,
    PreviousTab,
    Tab1,
    Tab2,
    Tab3,
    Tab4,
    Tab5,
    Tab6,
    Tab7,
    Tab8,
    #[cfg(target_os = "macos")]
    Hide,
    #[cfg(target_os = "macos")]
    HideOthers,
    Refresh,
    Search,
    Sidebar,
    Back,
    Copy,
    SelectWorking,
    SelectText,
    ExtendNext,
    ExtendPrevious,
    NextChange,
    PreviousChange,
    NextRow,
    PreviousRow,
    FirstRow,
    LastRow,
    Activate,
    Escape,
}

pub(super) struct Shortcut {
    pub id: ShortcutId,
    pub label: &'static str,
    pub group: &'static str,
    key: &'static str,
    context: Option<&'static str>,
    action: fn() -> Box<dyn Action>,
}

macro_rules! shortcut {
    ($id:ident, $label:literal, $group:literal, $key:literal, $action:expr, $context:expr) => {
        Shortcut {
            id: ShortcutId::$id,
            label: $label,
            group: $group,
            key: $key,
            action: || Box::new($action),
            context: $context,
        }
    };
}

const APP: Option<&str> = Some("GitTurtle");
const LIST: Option<&str> = Some("GitTurtleList");
/// Selectable read-only text, such as the commit message. Copy there is the
/// toolkit root's binding of the same key.
const TEXT: Option<&str> = Some("GitTurtleText");
pub(super) const SHORTCUTS: &[Shortcut] = &[
    shortcut!(
        Palette,
        "Command Palette",
        "General",
        "primary-shift-p",
        ShowCommandPalette,
        APP
    ),
    shortcut!(
        Open,
        "Open Repository",
        "General",
        "primary-o",
        OpenRepository,
        APP
    ),
    shortcut!(
        Projects,
        "Go to Projects",
        "General",
        "primary-shift-o",
        ShowProjects,
        APP
    ),
    shortcut!(
        Settings,
        "Settings",
        "General",
        "primary-,",
        ShowSettings,
        APP
    ),
    shortcut!(
        Help,
        "Keyboard Shortcuts",
        "General",
        "primary-?",
        ShortcutHelp,
        APP
    ),
    #[cfg(target_os = "linux")]
    shortcut!(MainMenu, "Main Menu", "General", "f10", MainMenu, APP),
    shortcut!(
        Activity,
        "GitTurtle Activity",
        "General",
        "primary-shift-a",
        ShowActivity,
        APP
    ),
    shortcut!(Quit, "Quit GitTurtle", "General", "primary-q", Quit, None),
    shortcut!(
        History,
        "Go to History",
        "Repository",
        "primary-1",
        ShowHistory,
        APP
    ),
    shortcut!(
        Changes,
        "Go to Working Changes",
        "Repository",
        "primary-2",
        ShowChanges,
        APP
    ),
    shortcut!(
        QuickOpen,
        "Quick Open File",
        "Repository",
        "primary-p",
        QuickOpenFile,
        APP
    ),
    shortcut!(
        Compare,
        "Compare Revisions",
        "Repository",
        "primary-shift-c",
        CompareRevisions,
        APP
    ),
    shortcut!(
        Refresh,
        "Refresh Local State",
        "Repository",
        "primary-r",
        Refresh,
        APP
    ),
    shortcut!(
        Sidebar,
        "Toggle Sidebar",
        "Repository",
        "primary-b",
        ToggleSidebar,
        APP
    ),
    shortcut!(
        Back,
        "Back to Retained Context",
        "Repository",
        "primary-[",
        BackHistory,
        APP
    ),
    shortcut!(
        Search,
        "Search Focused List",
        "Repository",
        "primary-f",
        Search,
        LIST
    ),
    shortcut!(
        NewTab,
        "Open Repository Tab",
        "Tabs and Windows",
        "primary-t",
        OpenRepository,
        APP
    ),
    shortcut!(
        CloseTab,
        "Close Repository Tab",
        "Tabs and Windows",
        "primary-w",
        CloseRepositoryTab,
        APP
    ),
    shortcut!(
        NextTab,
        "Next Repository Tab",
        "Tabs and Windows",
        "ctrl-tab",
        NextRepositoryTab,
        APP
    ),
    shortcut!(
        PreviousTab,
        "Previous Repository Tab",
        "Tabs and Windows",
        "ctrl-shift-tab",
        PreviousRepositoryTab,
        APP
    ),
    shortcut!(
        Tab1,
        "Switch to Repository Tab 1",
        "Tabs and Windows",
        "primary-alt-1",
        SelectRepositoryTab1,
        APP
    ),
    shortcut!(
        Tab2,
        "Switch to Repository Tab 2",
        "Tabs and Windows",
        "primary-alt-2",
        SelectRepositoryTab2,
        APP
    ),
    shortcut!(
        Tab3,
        "Switch to Repository Tab 3",
        "Tabs and Windows",
        "primary-alt-3",
        SelectRepositoryTab3,
        APP
    ),
    shortcut!(
        Tab4,
        "Switch to Repository Tab 4",
        "Tabs and Windows",
        "primary-alt-4",
        SelectRepositoryTab4,
        APP
    ),
    shortcut!(
        Tab5,
        "Switch to Repository Tab 5",
        "Tabs and Windows",
        "primary-alt-5",
        SelectRepositoryTab5,
        APP
    ),
    shortcut!(
        Tab6,
        "Switch to Repository Tab 6",
        "Tabs and Windows",
        "primary-alt-6",
        SelectRepositoryTab6,
        APP
    ),
    shortcut!(
        Tab7,
        "Switch to Repository Tab 7",
        "Tabs and Windows",
        "primary-alt-7",
        SelectRepositoryTab7,
        APP
    ),
    shortcut!(
        Tab8,
        "Switch to Repository Tab 8",
        "Tabs and Windows",
        "primary-alt-8",
        SelectRepositoryTab8,
        APP
    ),
    shortcut!(
        Minimize,
        "Minimize Window",
        "Tabs and Windows",
        "primary-m",
        MinimizeWindow,
        APP
    ),
    shortcut!(
        CloseWindow,
        "Close Window",
        "Tabs and Windows",
        "primary-shift-w",
        CloseWindow,
        APP
    ),
    #[cfg(target_os = "macos")]
    shortcut!(
        Hide,
        "Hide GitTurtle",
        "Tabs and Windows",
        "primary-h",
        HideApplication,
        None
    ),
    #[cfg(target_os = "macos")]
    shortcut!(
        HideOthers,
        "Hide Other Applications",
        "Tabs and Windows",
        "primary-alt-h",
        HideOtherApplications,
        None
    ),
    shortcut!(
        NextChange,
        "Next Text Change",
        "Text Review",
        "alt-down",
        NextTextChange,
        APP
    ),
    shortcut!(
        PreviousChange,
        "Previous Text Change",
        "Text Review",
        "alt-up",
        PreviousTextChange,
        APP
    ),
    shortcut!(
        Copy,
        "Copy Selected List Item",
        "Lists and Selection",
        "primary-c",
        gpui_kit::component::input::Copy,
        LIST
    ),
    shortcut!(
        SelectWorking,
        "Select All Working Changes",
        "Lists and Selection",
        "primary-a",
        SelectAllWorking,
        LIST
    ),
    shortcut!(
        SelectText,
        "Select All Message Text",
        "Lists and Selection",
        "primary-a",
        gpui_kit::component::input::SelectAll,
        TEXT
    ),
    shortcut!(
        ExtendNext,
        "Extend Working Selection Down",
        "Lists and Selection",
        "shift-down",
        ExtendNextWorking,
        LIST
    ),
    shortcut!(
        ExtendPrevious,
        "Extend Working Selection Up",
        "Lists and Selection",
        "shift-up",
        ExtendPreviousWorking,
        LIST
    ),
    shortcut!(
        NextRow,
        "Next List Row",
        "Lists and Selection",
        "down",
        NextRow,
        LIST
    ),
    shortcut!(
        PreviousRow,
        "Previous List Row",
        "Lists and Selection",
        "up",
        PreviousRow,
        LIST
    ),
    shortcut!(
        FirstRow,
        "First List Row",
        "Lists and Selection",
        "home",
        FirstRow,
        LIST
    ),
    shortcut!(
        LastRow,
        "Last List Row",
        "Lists and Selection",
        "end",
        LastRow,
        LIST
    ),
    shortcut!(
        Activate,
        "Open Selected List Item",
        "Lists and Selection",
        "enter",
        NextPane,
        LIST
    ),
    shortcut!(
        Escape,
        "Dismiss or Return to Retained Context",
        "Lists and Selection",
        "escape",
        ClearSearch,
        APP
    ),
];

fn resolve_key(key: &str, macos: bool) -> String {
    key.replace("primary", if macos { "cmd" } else { "ctrl" })
}

pub(super) fn bind_keys(cx: &mut App) {
    // The first registry entry for an action is its advertised shortcut.
    // Register in reverse so aliases (Open Tab) do not replace Open's label.
    cx.bind_keys(SHORTCUTS.iter().rev().map(|spec| {
        KeyBinding::load(
            &resolve_key(spec.key, cfg!(target_os = "macos")),
            (spec.action)(),
            spec.context.map(|context| {
                gpui::KeyBindingContextPredicate::parse(context)
                    .expect("static shortcut context must be valid")
                    .into()
            }),
            false,
            None,
            &gpui::DummyKeyboardMapper,
        )
        .expect("static shortcut must be valid")
    }));
}

/// `field`, a multi-line prose field, in a wrapper from which Tab and
/// Shift-Tab move focus to the next and the previous stop, as from every other
/// stop (DESIGN.md's keyboard contract), changing none of its text. The kit's
/// multi-line field binds them to indenting its lines, so focus never left
/// it; the wrapper takes the indent actions in their capture phase, ahead of
/// the field. The commit description, the rebase message, the amended
/// description, the tag annotation and the remote form's URL and refspec
/// lists take it. The GitHub panel's composers grow with their text, a mode
/// the kit never indents, and the conflict editors keep Tab for indenting
/// code.
pub(crate) fn tab_leaves(field: impl IntoElement) -> Div {
    use gpui_kit::component::input::{IndentInline, OutdentInline};
    div()
        .capture_action(|_: &IndentInline, window, cx| {
            window.focus_next(cx);
            cx.stop_propagation();
        })
        .capture_action(|_: &OutdentInline, window, cx| {
            window.focus_prev(cx);
            cx.stop_propagation();
        })
        .child(field)
}

#[cfg(target_os = "linux")]
pub(super) fn action(id: ShortcutId) -> Option<Box<dyn Action>> {
    SHORTCUTS
        .iter()
        .find(|spec| spec.id == id)
        .map(|spec| (spec.action)())
}

/// The Enter key as prose names it: Mac keyboards label it Return.
pub(super) const ENTER: &str = if cfg!(target_os = "macos") {
    "Return"
} else {
    "Enter"
};

/// A Shift-modified click as prose names it, in the platform's modifier style.
pub(super) const SHIFT_CLICK: &str = if cfg!(target_os = "macos") {
    "Shift-click"
} else {
    "Shift+click"
};

pub(super) fn key_label(key: &str) -> String {
    // Use the same platform renderer as native GPUI popup-menu accelerators.
    Kbd::format(
        &Keystroke::parse(&resolve_key(key, cfg!(target_os = "macos")))
            .expect("static shortcut must be valid"),
    )
}

pub(super) fn label(id: ShortcutId) -> String {
    SHORTCUTS
        .iter()
        .find(|spec| spec.id == id)
        .map(|spec| key_label(spec.key))
        .unwrap_or_default()
}

pub(super) fn tab_label(index: usize) -> String {
    key_label(&format!("primary-alt-{}", index + 1))
}

#[derive(Clone)]
struct HelpEntry {
    group: &'static str,
    label: &'static str,
    key: String,
}

fn help_entries() -> Vec<HelpEntry> {
    let mut entries: Vec<_> = SHORTCUTS
        .iter()
        .map(|spec| HelpEntry {
            group: spec.group,
            label: spec.label,
            key: label(spec.id),
        })
        .collect();
    // These belong to GPUI's focused inputs, not the application key context.
    // In particular, the toolkit uses Ctrl+Y for Redo on Linux.
    for (label, key) in [
        ("Undo Text Edit", "primary-z"),
        (
            "Redo Text Edit",
            if cfg!(target_os = "macos") {
                "cmd-shift-z"
            } else {
                "ctrl-y"
            },
        ),
        ("Cut Text", "primary-x"),
        ("Copy Text", "primary-c"),
        ("Paste Text", "primary-v"),
        ("Select All Text", "primary-a"),
        ("Find in Editor", "primary-f"),
    ] {
        entries.push(HelpEntry {
            group: "Text Editing",
            label,
            key: key_label(key),
        });
    }
    for (label, key) in [
        ("Next Control", "tab"),
        ("Previous Control", "shift-tab"),
        ("Activate Focused Button", "space"),
        ("Branch / Worktree Actions in Navigator", "shift-f10"),
        ("File Actions in a File List", "shift-f10"),
        // Linux reports the Menu key; macOS keyboards have none.
        #[cfg(not(target_os = "macos"))]
        ("File Actions in a File List", "menu"),
    ] {
        entries.push(HelpEntry {
            group: "Keyboard Navigation",
            label,
            key: key_label(key),
        });
    }
    entries
}

#[derive(Clone)]
enum HelpRow {
    Group(&'static str),
    Entry(HelpEntry),
}

fn help_matches(query: &str) -> Vec<HelpRow> {
    if query.len() > 4096 {
        return Vec::new();
    }
    let query = query.to_lowercase();
    let terms: Vec<_> = query.split_whitespace().collect();
    let mut rows = Vec::new();
    let mut group = None;
    for entry in help_entries() {
        let key_words = entry
            .key
            .replace('⌃', "ctrl control ")
            .replace('⌘', "cmd command ")
            .replace('⌥', "alt option ")
            .replace('⇧', "shift ");
        let haystack =
            format!("{} {} {} {key_words}", entry.group, entry.label, entry.key).to_lowercase();
        if !terms.iter().all(|term| haystack.contains(term)) {
            continue;
        }
        if group != Some(entry.group) {
            group = Some(entry.group);
            rows.push(HelpRow::Group(entry.group));
        }
        rows.push(HelpRow::Entry(entry));
    }
    rows
}

/// A dialog control with keys of its own, which the status bar names while
/// that control holds focus, in place of the page's hints beneath the dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DialogKeys {
    /// The command palette's search, which moves through its commands.
    Palette,
    /// Quick Open's search, which moves through its files.
    QuickOpen,
    /// Keyboard Shortcuts' search, which browses its list.
    Shortcuts,
    /// The interactive rebase's planned sequence.
    RebaseSequence,
}

impl DialogKeys {
    /// The hints in the status bar's form, `Key Action · Key Action`. Escape
    /// is named by the dialog's own button it stands for ([`Self::dismiss`]):
    /// the palette's hints said "Esc Close" beside its Cancel button, while
    /// Quick Open's said "Esc Cancel".
    pub(super) fn hints(self) -> String {
        let escape = format!("Esc {}", self.dismiss());
        match self {
            Self::Palette | Self::QuickOpen => format!("↑↓ Select · {ENTER} Open · {escape}"),
            Self::Shortcuts => format!("↑↓ Browse · {escape}"),
            Self::RebaseSequence => format!(
                "↑↓ Select · {} / {} Reorder · {escape}",
                key_label("alt-up"),
                key_label("alt-down")
            ),
        }
    }

    /// The verb of the dialog's button that Escape stands for, as the button
    /// and hints say it and as its inline help does: Cancel in the command
    /// palette and Quick Open, which leave without acting, and Close in
    /// Keyboard Shortcuts, a read-only sheet, and the rebase dialog.
    fn verb(self) -> (&'static str, &'static str) {
        match self {
            Self::Palette | Self::QuickOpen => ("Cancel", "cancels"),
            Self::Shortcuts | Self::RebaseSequence => ("Close", "closes"),
        }
    }

    /// The label of the dialog's button that Escape stands for, the one verb
    /// its hints, its footer and its inline help share ([`Self::verb`]).
    pub(super) fn dismiss(self) -> &'static str {
        self.verb().0
    }

    /// The clause the dialog's inline help says Escape with, in the button's
    /// verb: "Escape cancels" or "Escape closes".
    pub(super) fn escape_help(self) -> String {
        format!("Escape {}", self.verb().1)
    }

    /// The dialog's footer button that Escape stands for, labelled
    /// [`Self::dismiss`] and named `dialog-dismiss: <label>` for tests.
    pub(super) fn dismiss_button(self, id: &'static str, primary: bool) -> Button {
        let label = self.dismiss();
        button(id, label, "", primary).debug_selector(move || format!("dialog-dismiss: {label}"))
    }
}

/// A dialog's inline help line, `text`, named for tests by the clause in it
/// that says what Escape does (`dialog-help-escape: <clause>`, empty where
/// the line has none), because GPUI's test scene does not expose glyphs.
pub(super) fn dialog_help(text: String) -> Div {
    let escape = text
        .split(" · ")
        .find(|clause| clause.starts_with("Escape"))
        .unwrap_or_default()
        .to_owned();
    div()
        .debug_selector(move || format!("dialog-help-escape: {escape}"))
        .child(text)
}

/// The dialog controls whose keys the status bar names, registered as their
/// dialogs open. A control is held weakly, so a closed dialog's entry lapses
/// with its view, and each registration drops the lapsed ones.
#[derive(Default)]
pub(super) struct DialogHints(Vec<(WeakFocusHandle, DialogKeys)>);

impl DialogHints {
    pub(super) fn register(&mut self, focus: &FocusHandle, keys: DialogKeys) {
        self.0
            .retain(|(handle, _)| handle.upgrade().is_some() && handle != focus);
        self.0.push((focus.downgrade(), keys));
    }

    /// The keys of the registered control that holds focus, if one does.
    pub(super) fn focused(&self, window: &Window) -> Option<DialogKeys> {
        self.0.iter().find_map(|(handle, keys)| {
            handle
                .upgrade()
                .is_some_and(|handle| handle.is_focused(window))
                .then_some(*keys)
        })
    }
}

pub(super) fn open_help(hints: &mut DialogHints, window: &mut Window, cx: &mut Context<GitTurtle>) {
    use gpui_kit::component::{WindowExt, dialog::DialogFooter};
    if window.has_active_dialog(cx) || window.has_active_sheet(cx) {
        return;
    }
    let return_focus = window.focused(cx);
    let help = cx.new(|cx| ShortcutHelpView::new(return_focus, window, cx));
    let focus = help.read(cx).query.read(cx).focus_handle(cx);
    hints.register(&focus, DialogKeys::Shortcuts);
    window.open_alert_dialog(cx, move |dialog, window, cx| {
        let done = help.clone();
        let cancel = help.clone();
        let confirm = help.clone();
        // A list of commands' keys floats as the menus and the command
        // palette do: on the raised surface with their fitted edge, read on
        // every frame so a theme switch retints it with them.
        let surface = palette(cx);
        // The dialog clips its body at the body's top, where the search
        // field stands, so the view keeps the focus ring's room above the
        // field and the title's margin gives it back: nothing moves.
        let room = appearance::button_ring_room(cx);
        dialog
            .title(
                div()
                    .id("keyboard-shortcuts-title")
                    .role(Role::Label)
                    .aria_label("Keyboard Shortcuts")
                    .mb(-room)
                    .child("Keyboard Shortcuts"),
            )
            .width(appearance::dialog_width(appearance::ui_size(650.), window))
            .bg(rgb(surface.raised()))
            .border_color(rgb(surface.raised_edge()))
            .child(help.clone())
            .footer(
                DialogFooter::new().child(
                    DialogKeys::Shortcuts
                        .dismiss_button("close-shortcut-help", true)
                        .on_click(move |_, window, cx| {
                            done.update(cx, |view, cx| view.close(window, cx))
                        }),
                ),
            )
            .on_cancel(move |_, window, cx| {
                cancel.update(cx, |view, cx| view.close(window, cx));
                false
            })
            .on_ok(move |_, window, cx| {
                confirm.update(cx, |view, cx| view.close(window, cx));
                false
            })
    });
    focus.focus(window, cx);
    window.refresh();
}

struct ShortcutHelpView {
    query: Entity<InputState>,
    rows: Vec<HelpRow>,
    selected: usize,
    scroll: UniformListScrollHandle,
    return_focus: Option<FocusHandle>,
    closed: bool,
    _subscription: Subscription,
}

impl ShortcutHelpView {
    fn new(return_focus: Option<FocusHandle>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let query =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search shortcuts or groups…"));
        let subscription = cx.subscribe(&query, |this, _, event, cx| {
            if matches!(event, InputEvent::Change) {
                this.rows = help_matches(&this.query.read(cx).value());
                this.selected = 1.min(this.rows.len().saturating_sub(1));
                this.scroll.scroll_to_item(0, ScrollStrategy::Top);
                cx.notify();
            }
        });
        Self {
            query,
            rows: help_matches(""),
            selected: 1,
            scroll: UniformListScrollHandle::new(),
            return_focus,
            closed: false,
            _subscription: subscription,
        }
    }

    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        use gpui_kit::component::WindowExt;
        if self.closed {
            return;
        }
        self.closed = true;
        window.close_dialog(cx);
        if let Some(focus) = &self.return_focus {
            focus.focus(window, cx);
        }
        window.refresh();
    }
}

impl Render for ShortcutHelpView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use gpui_kit::prelude::FluentBuilder;
        let p = palette(cx);
        let count = self
            .rows
            .iter()
            .filter(|row| matches!(row, HelpRow::Entry(_)))
            .count();
        let height = (window.viewport_size().height - appearance::ui_size(280.))
            .max(appearance::ui_size(88.))
            .min(appearance::ui_size(430.));
        let list = uniform_list(
            "shortcut-help-list",
            self.rows.len(),
            cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                let p = palette(cx);
                // Fitted once for the rows drawn, not for each row.
                let (edge, selected_edge) = (p.raised_edge(), p.selected_edge());
                range
                    .map(|index| {
                        let row = div()
                            .id(("shortcut-help-row", index))
                            .h(appearance::ui_size(42.))
                            .w_full()
                            .px_3()
                            .flex()
                            .items_center()
                            .gap_4();
                        match &this.rows[index] {
                            HelpRow::Group(group) => row
                                .role(Role::Heading)
                                .aria_label(*group)
                                .text_size(appearance::ui_text(11.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(p.muted))
                                // Ruled with the edge, as a menu's
                                // separators are.
                                .border_b_1()
                                .border_color(rgb(edge))
                                .child(*group),
                            HelpRow::Entry(entry) => row
                                .debug_selector(move || format!("shortcut-help-row-{index}"))
                                .role(Role::ListBoxOption)
                                .aria_label(format!("{}: {}", entry.label, entry.key))
                                .aria_selected(index == this.selected)
                                .relative()
                                // The highlight is the keyboard's cursor, as
                                // in the other pickers.
                                .when(index == this.selected, |row| {
                                    row.bg(rgb(p.selected)).child(appearance::cursor_marker(p))
                                })
                                .child(div().flex_1().min_w_0().truncate().child(entry.label))
                                .child(
                                    div()
                                        .debug_selector(move || {
                                            format!("shortcut-help-key-{index}")
                                        })
                                        .flex_shrink_0()
                                        .rounded(px(4.))
                                        .px_2()
                                        .py_1()
                                        .text_size(appearance::ui_text(11.))
                                        .font_family(mono())
                                        .border_1()
                                        // The raised edge nearly equals the
                                        // selected fill in some palettes.
                                        .border_color(rgb(if index == this.selected {
                                            selected_edge
                                        } else {
                                            edge
                                        }))
                                        .child(entry.key.clone()),
                                )
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.selected = index;
                                    cx.notify();
                                })),
                        }
                        .into_any_element()
                    })
                    .collect()
            }),
        )
        .track_scroll(&self.scroll)
        .size_full();
        div().id("keyboard-shortcuts-view").debug_selector(|| "keyboard-shortcuts-view".into()).key_context("GitTurtleShortcuts")
            .flex().flex_col().gap_3().pt(appearance::button_ring_room(cx)).text_size(appearance::ui_text(12.))
            .on_action(cx.listener(|this, _: &gpui_kit::component::input::Escape, window, cx| {
                this.close(window, cx); cx.stop_propagation();
            }))
            .on_action(cx.listener(|this, _: &ClearSearch, window, cx| {
                this.close(window, cx); cx.stop_propagation();
            }))
            .on_action(cx.listener(|this, _: &gpui_kit::component::dialog::Cancel, window, cx| {
                this.close(window, cx); cx.stop_propagation();
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.modifiers.modified() { return; }
                let forward = match event.keystroke.key.as_str() { "down" => true, "up" => false, _ => return };
                let next = if forward {
                    (this.selected + 1..this.rows.len()).find(|index| matches!(this.rows[*index], HelpRow::Entry(_)))
                } else {
                    (0..this.selected).rev().find(|index| matches!(this.rows[*index], HelpRow::Entry(_)))
                };
                if let Some(index) = next {
                    this.selected = index;
                    this.scroll.scroll_to_item(index, ScrollStrategy::Center);
                    cx.notify();
                }
                cx.stop_propagation();
            }))
            .child(Input::new(&self.query).aria_label("Search keyboard shortcuts").cleanable(true))
            .child(div().id("shortcut-help-count").role(Role::Status)
                .a11y_synthetic_children(native_accessibility::polite)
                .aria_label(format!("{count} matching shortcuts"))
                .text_color(rgb(p.muted)).child(format!("{count} shortcuts · {}",
                    if cfg!(target_os = "macos") { "⌘ Command  ⌥ Option  ⌃ Control  ⇧ Shift" } else { "Ctrl, Alt and Shift" })))
            .child(crate::native_accessibility::clips_children(div().id("shortcut-help-results").debug_selector(|| "shortcut-help-results".into()).role(Role::ListBox)
                .aria_label("Keyboard shortcuts by group").h(height).overflow_hidden()
                .rounded(px(7.)).border_1().border_color(rgb(p.raised_edge()))
                .when(count > 0, |view| view.child(list))
                .when(count == 0, |view| view.child(div().p_4().text_color(rgb(p.muted)).child(
                    if self.query.read(cx).value().len() > 4096 { "Search is limited to 4,096 bytes." }
                    else { "No shortcuts match. Try tabs, compare, editing or Ctrl." })))))
            .child(div().id("shortcut-help-selected").role(Role::Status)
                .a11y_synthetic_children(native_accessibility::polite)
                .aria_label(match self.rows.get(self.selected) {
                    Some(HelpRow::Entry(entry)) => format!("{}: {}", entry.label, entry.key),
                    _ => String::new(),
                }).h(px(1.)).overflow_hidden())
            .child(div().text_size(appearance::ui_text(11.)).text_color(rgb(p.muted))
                .child(dialog_help(format!("↑ / ↓ browse · {} · Shortcuts act in the focused view; text editing keeps its own keys.", DialogKeys::Shortcuts.escape_help()))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn bindings_are_unique_valid_on_both_platforms_and_keep_input_scope() {
        for macos in [true, false] {
            let mut seen = std::collections::HashSet::new();
            for spec in SHORTCUTS {
                let key = resolve_key(spec.key, macos);
                assert!(Keystroke::parse(&key).is_ok(), "{}: {key}", spec.label);
                assert!(
                    seen.insert((key, spec.context)),
                    "duplicate: {}",
                    spec.label
                );
            }
        }
        assert_eq!(resolve_key("ctrl-shift-tab", true), "ctrl-shift-tab");
        assert_eq!(resolve_key("primary-shift-p", false), "ctrl-shift-p");
        for id in [
            ShortcutId::Search,
            ShortcutId::SelectWorking,
            ShortcutId::Copy,
        ] {
            assert_eq!(
                SHORTCUTS.iter().find(|spec| spec.id == id).unwrap().context,
                LIST
            );
        }
    }

    #[test]
    fn labels_keep_platform_modifiers_and_real_tab_binding() {
        if cfg!(target_os = "macos") {
            assert_eq!(label(ShortcutId::NextTab), "⌃Tab");
            assert_eq!(label(ShortcutId::Palette), "⇧⌘P");
            assert_eq!(label(ShortcutId::Help), "⌘?");
        } else {
            assert_eq!(label(ShortcutId::NextTab), "Ctrl+Tab");
            assert_eq!(label(ShortcutId::Palette), "Ctrl+Shift+P");
            assert_eq!(label(ShortcutId::Help), "Ctrl+?");
            assert_eq!(tab_label(2), "Ctrl+Alt+3");
            for spec in SHORTCUTS {
                assert!(!label(spec.id).contains(['⌃', '⌥', '⇧', '⌘']));
            }
        }
    }

    #[test]
    fn help_binding_matches_backend_normalized_shifted_punctuation() {
        let help = SHORTCUTS
            .iter()
            .find(|spec| spec.id == ShortcutId::Help)
            .unwrap();
        // Both the XKB and Cocoa adapters emit '?' with Shift consumed, even
        // when a US keyboard physically presses Shift+/. Letters retain Shift.
        for macos in [false, true] {
            let binding =
                KeyBinding::new(&resolve_key(help.key, macos), ShortcutHelp, help.context);
            let event = Keystroke {
                modifiers: Modifiers {
                    control: !macos,
                    platform: macos,
                    ..Modifiers::default()
                },
                key: "?".into(),
                key_char: None,
            };
            // Some(false) means a complete match; Some(true) is a pending
            // prefix of a longer multi-stroke binding, and None is no match.
            assert_eq!(
                binding.match_keystrokes(std::slice::from_ref(&event)),
                Some(false)
            );
            let unnormalized = KeyBinding::new(
                &resolve_key("primary-shift-/", macos),
                ShortcutHelp,
                help.context,
            );
            assert_eq!(unnormalized.match_keystrokes(&[event]), None);
        }
    }

    #[test]
    fn help_search_preserves_groups_matches_keys_and_bounds_queries() {
        let rows = help_matches("tabs ctrl");
        assert!(matches!(
            rows.first(),
            Some(HelpRow::Group("Tabs and Windows"))
        ));
        assert!(rows.iter().any(
            |row| matches!(row, HelpRow::Entry(entry) if entry.label == "Next Repository Tab")
        ));
        assert!(help_matches("unlikely nonexistent shortcut").is_empty());
        assert!(help_matches(&"x".repeat(4097)).is_empty());
        let text_editing = help_matches("redo text");
        let key = text_editing
            .iter()
            .find_map(|row| match row {
                HelpRow::Entry(entry) => Some(entry.key.as_str()),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            key,
            if cfg!(target_os = "macos") {
                "⇧⌘Z"
            } else {
                "Ctrl+Y"
            }
        );
    }
}
