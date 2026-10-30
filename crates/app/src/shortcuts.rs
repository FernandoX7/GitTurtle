//! The app-owned bindings are also the source for menu, palette and help labels.
//! Toolkit text-editing bindings keep their own focused input contexts.
use crate::native_accessibility::LiveRegion as _;
use crate::*;
use gpui_kit::base::ElementExt as _;
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
        ("Project Actions in the Project List", "shift-f10"),
        ("Commit Actions in History", "shift-f10"),
        ("File Actions in a File List", "shift-f10"),
        // Linux reports the Menu key; macOS keyboards have none.
        #[cfg(not(target_os = "macos"))]
        ("Branch / Worktree Actions in Navigator", "menu"),
        #[cfg(not(target_os = "macos"))]
        ("Project Actions in the Project List", "menu"),
        #[cfg(not(target_os = "macos"))]
        ("Commit Actions in History", "menu"),
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

/// How many shortcuts the help lists before a search narrows it.
#[cfg(test)]
pub(crate) fn listed_shortcuts() -> usize {
    help_matches("")
        .iter()
        .filter(|row| matches!(row, HelpRow::Entry(_)))
        .count()
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

/// Keyboard hints for the status bar: each key and what it does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct KeyHints(pub(super) Vec<(String, &'static str)>);

impl KeyHints {
    /// The whole form, `Key Action · Key Action`.
    pub(super) fn whole(&self) -> String {
        self.join(|(key, action)| format!("{key} {action}"))
    }

    /// The keys alone, `Key · Key`: the form the hints take where the status
    /// line or the refresh warning needs their room (`GitTurtle::fit_status_bar`).
    pub(super) fn keys(&self) -> String {
        self.join(|(key, _)| key.clone())
    }

    fn join(&self, phrase: impl Fn(&(String, &'static str)) -> String) -> String {
        self.0.iter().map(phrase).collect::<Vec<_>>().join(" · ")
    }
}

impl DialogKeys {
    /// The hints the status bar names, each key with what it does. Escape
    /// is named by the dialog's own button it stands for ([`Self::dismiss`]):
    /// the palette's hints said "Esc Close" beside its Cancel button, while
    /// Quick Open's said "Esc Cancel".
    pub(super) fn key_hints(self) -> KeyHints {
        let escape = ("Esc".to_owned(), self.dismiss());
        KeyHints(match self {
            Self::Palette | Self::QuickOpen => {
                vec![("↑↓".into(), "Select"), (ENTER.into(), "Open"), escape]
            }
            Self::Shortcuts => vec![("↑↓".into(), "Browse"), escape],
            Self::RebaseSequence => vec![
                ("↑↓".into(), "Select"),
                (
                    format!("{} / {}", key_label("alt-up"), key_label("alt-down")),
                    "Reorder",
                ),
                escape,
            ],
        })
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

/// A dialog's inline help line, `text`, drawn with ID `id` as static text
/// read by the whole line, and named for tests by the clause in it that says
/// what Escape does (`dialog-help-escape: <clause>`, empty where the line
/// has none), because GPUI's test scene does not expose glyphs. The line
/// had no node, so the command palette's, Quick Open's and Keyboard
/// Shortcuts' keys were never read.
pub(super) fn dialog_help(
    id: &'static str,
    text: String,
) -> native_accessibility::StaticText<Stateful<Div>> {
    let escape = text
        .split(" · ")
        .find(|clause| clause.starts_with("Escape"))
        .unwrap_or_default()
        .to_owned();
    native_accessibility::static_text(
        div()
            .id(id)
            .debug_selector(move || format!("dialog-help-escape: {escape}"))
            .child(text.clone()),
        text,
    )
}

/// [`dialog_help`] laid out by phrase, each " · "-separated phrase a
/// wrapping item (`crate::phrase_runs`), drawn as `dialog-help-phrase-{index}`:
/// where the line wraps, it breaks between phrases, so Quick Open's hint at
/// 18 pt in a 461 px window no longer leaves "cancels" alone on its last
/// line. On one line it reads as the text does.
pub(super) fn dialog_help_by_phrase(text: String) -> Div {
    let phrases = crate::phrase_runs(&text);
    let escape = text
        .split(" · ")
        .find(|clause| clause.starts_with("Escape"))
        .unwrap_or_default()
        .to_owned();
    div()
        .debug_selector(move || format!("dialog-help-escape: {escape}"))
        .flex()
        .flex_wrap()
        .children(phrases.into_iter().enumerate().map(|(index, phrase)| {
            div()
                .debug_selector(move || format!("dialog-help-phrase-{index}"))
                .child(phrase)
        }))
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
                native_accessibility::dialog_title(
                    "keyboard-shortcuts-title",
                    "Keyboard Shortcuts",
                )
                .mb(-room),
            )
            .width(appearance::dialog_width(appearance::ui_size(650.), window))
            .max_h(appearance::dialog_max_height(window))
            .bg(rgb(surface.raised()))
            .border_color(rgb(surface.raised_edge()))
            // In the dialog's content rather than its scrolling body, whose
            // scroller lays what it holds out at full height: here the view
            // shrinks with the dialog's height cap and its list gives up the
            // height, so the field, the hint and Done stay in view.
            .content({
                let help = help.clone();
                move |content, _, _| content.min_h_0().child(help.clone())
            })
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

/// The line above the shortcut list: how many shortcuts it holds, the count
/// agreeing with its noun ("1 shortcuts" once read for a search that left
/// one), and the platform's modifier keys. The list's status draws it and is
/// named by it.
pub(crate) fn count_line(count: usize) -> String {
    format!(
        "{} · {}",
        crate::counted(count, "shortcut", "shortcuts"),
        if cfg!(target_os = "macos") {
            "⌘ Command  ⌥ Option  ⌃ Control  ⇧ Shift"
        } else {
            "Ctrl, Alt and Shift"
        }
    )
}

/// The shortcut list's height at the default interface size, where the
/// dialog has the room.
const SHORTCUT_LIST_HEIGHT: f32 = 430.;
/// The least the list gives up its height to, at the default interface size:
/// one 42 px row inside its 1 px border, as at 18 pt in a 461 × 490 window,
/// where the hint below it wraps to three lines.
const SHORTCUT_LIST_FLOOR: f32 = 44.;
/// An entry's row, at the default interface size.
const SHORTCUT_ROW: f32 = 42.;
/// A group's heading row, at the default interface size: the group gap
/// above its 11 px name, the name's 14 px line, and 3 px below it to the rule
/// under it, about 30 px at 18 pt. As tall as an entry, the heading's row
/// (58 px at 18 pt) had its top cut by the list's edge after a one-match
/// search in a 461 × 490 window, whose 99 px list held its entry whole below
/// it; heading and entry now take about 88 px there.
const SHORTCUT_HEADING: f32 = 22.;
/// The name's line in a heading row and the room below it, at the default
/// interface size; the rest of the row is the group gap above it.
const SHORTCUT_HEADING_LINE: f32 = 14.;
const SHORTCUT_HEADING_BELOW: f32 = 3.;

/// The shortcut list's height where the dialog has the room, at the
/// interface size.
pub(crate) fn list_height() -> Pixels {
    appearance::ui_size(SHORTCUT_LIST_HEIGHT)
}

/// The height of `row` in the shortcut list, at the interface size.
fn row_height(row: &HelpRow) -> Pixels {
    appearance::ui_size(match row {
        HelpRow::Group(_) => SHORTCUT_HEADING,
        HelpRow::Entry(_) => SHORTCUT_ROW,
    })
}

/// The rows' heights in the list's order at one interface size, as the
/// toolkit's virtual list takes them, and where each row starts.
struct RowLayout {
    sizes: std::rc::Rc<Vec<Size<Pixels>>>,
    /// Each row's top in the list, summed in the order the virtual list sums
    /// them, and then the list's whole height.
    tops: Vec<Pixels>,
    /// The interface size they were measured at, `appearance::ui_scale`.
    scale: f32,
}

impl RowLayout {
    fn new(rows: &[HelpRow]) -> Self {
        let mut tops = Vec::with_capacity(rows.len() + 1);
        let mut top = px(0.);
        let sizes = rows
            .iter()
            .map(|row| {
                let height = row_height(row);
                tops.push(top);
                top += height;
                Size {
                    width: px(0.),
                    height,
                }
            })
            .collect();
        tops.push(top);
        Self {
            sizes: std::rc::Rc::new(sizes),
            tops,
            scale: appearance::ui_scale(),
        }
    }

    /// The list's whole height.
    fn content(&self) -> Pixels {
        self.tops[self.tops.len() - 1]
    }

    /// What the list brings into view with the entry at `index` among `rows`.
    fn span(&self, rows: &[HelpRow], index: usize) -> Option<Span> {
        let bottom = *self.tops.get(index + 1)?;
        let heading = index
            .checked_sub(1)
            .filter(|&above| matches!(rows[above], HelpRow::Group(_)))
            .map(|above| self.tops[above]);
        Some(Span {
            heading,
            top: self.tops[index],
            bottom,
        })
    }
}

/// What the list brings into view with an entry: the entry's top and bottom
/// in the list, and the top of its group's heading where it is the group's
/// first entry, which comes with it where the list holds both, so no reveal
/// leaves the heading cut above a group's first entry.
#[derive(Clone, Copy)]
struct Span {
    heading: Option<Pixels>,
    top: Pixels,
    bottom: Pixels,
}

/// How the list brings the selected entry into view where some of it lies
/// outside the list.
#[derive(Clone, Copy)]
enum Reveal {
    /// Centred, as Up and Down centre the entry they reach, as in the
    /// palette and Quick Open.
    Centre,
    /// Scrolled the least that shows it whole, for a click on it or a list
    /// whose new height would cut it.
    Nearest,
}

/// A key that moves the list's selection through its entries, passing over
/// group headings.
#[derive(Clone, Copy)]
enum Step {
    /// Up: the entry above.
    Previous,
    /// Down: the entry below.
    Next,
    /// Page Up: a page of entries above ([`ShortcutHelpView::page`]).
    PageUp,
    /// Page Down: a page of entries below.
    PageDown,
    /// Home: the list's first entry.
    First,
    /// End: the list's last entry.
    Last,
}

gpui_kit::actions!(
    shortcut_help,
    [
        FirstShortcut,
        LastShortcut,
        PreviousShortcutPage,
        NextShortcutPage
    ]
);

/// Where Home, End, Page Up and Page Down move the list's selection: in the
/// search field, which holds focus while the dialog is open. Bound at the
/// field's own depth and later than the toolkit binds the field's keys, they
/// come before its Home and End, which would move its caret, as macOS's
/// single-line fields leave those keys to the list or view beside them;
/// Shift-Home and Shift-End still select in the field. Up and Down, which a
/// single-line field passes on, reach the view's key handler.
const LIST_KEYS: &str = "GitTurtleShortcuts > Input";

/// Marks [`bind_list_keys`] done for the application.
struct ListBindings;
impl gpui_kit::Global for ListBindings {}

/// Binds the list's keys ([`LIST_KEYS`]) once, as the dialog first opens.
fn bind_list_keys(cx: &mut App) {
    if cx.try_global::<ListBindings>().is_some() {
        return;
    }
    cx.set_global(ListBindings);
    cx.bind_keys([
        KeyBinding::new("home", FirstShortcut, Some(LIST_KEYS)),
        KeyBinding::new("end", LastShortcut, Some(LIST_KEYS)),
        KeyBinding::new("pageup", PreviousShortcutPage, Some(LIST_KEYS)),
        KeyBinding::new("pagedown", NextShortcutPage, Some(LIST_KEYS)),
    ]);
}

impl Span {
    /// How far past the list's edge a row may lie and still count as whole:
    /// the toolkit scrolls a row to an edge in window coordinates, so a row
    /// it placed there can read a rounding error past it here.
    const ROUNDING: Pixels = px(0.01);

    /// Whether the entry itself, without its group's heading, lies whole in
    /// a list `view` tall scrolled by `scrolled`.
    fn whole(self, scrolled: Pixels, view: Pixels) -> bool {
        self.top >= scrolled - Self::ROUNDING && self.bottom <= scrolled + view + Self::ROUNDING
    }

    /// The scroll, from the list's top, that brings the span whole into a
    /// list `view` tall that holds `content` and is scrolled by `scrolled`,
    /// or `None` where it lies whole there already.
    fn reveal(
        self,
        scrolled: Pixels,
        view: Pixels,
        content: Pixels,
        how: Reveal,
    ) -> Option<Pixels> {
        if view <= px(0.) {
            return None;
        }
        let top = self
            .heading
            .filter(|&heading| self.bottom - heading <= view)
            .unwrap_or(self.top);
        let above = top < scrolled - Self::ROUNDING;
        let below = self.bottom > scrolled + view + Self::ROUNDING;
        let to = match how {
            _ if !above && !below => return None,
            Reveal::Centre => (top + self.bottom) / 2. - view / 2.,
            Reveal::Nearest if above => top,
            Reveal::Nearest => self.bottom - view,
        };
        Some(to.min(content - view).max(px(0.)))
    }
}

struct ShortcutHelpView {
    query: Entity<InputState>,
    rows: Vec<HelpRow>,
    /// Each row's height and top, built with the rows and again when the
    /// interface size changes, rather than on every frame or key press.
    layout: RowLayout,
    selected: usize,
    /// A group's heading row is shorter than an entry's, so the list is the
    /// toolkit's virtual list, which takes each row's height, rather than a
    /// `uniform_list`, which gives every row the first one's.
    scroll: gpui_kit::base::VirtualListScrollHandle,
    /// The list's height inside its border as last laid out, and the
    /// interface size its rows were measured at then: when the dialog's
    /// height cap changes the height, the list keeps a selected entry it
    /// showed whole in view.
    list_height: std::rc::Rc<std::cell::Cell<(Pixels, f32)>>,
    return_focus: Option<FocusHandle>,
    closed: bool,
    _subscription: Subscription,
}

impl ShortcutHelpView {
    fn new(return_focus: Option<FocusHandle>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        bind_list_keys(cx);
        let query =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search shortcuts or groups…"));
        let subscription = cx.subscribe(&query, |this, _, event, cx| {
            if matches!(event, InputEvent::Change) {
                this.set_rows(help_matches(&this.query.read(cx).value()));
                this.selected = 1.min(this.rows.len().saturating_sub(1));
                // The list starts at its top, unless the selected entry
                // would not then lie whole in it: it ends at the list's foot
                // instead, with as much of its group's heading as fits above
                // it. Started at the heading, a list the dialog's height cap
                // had given less than both cut the one entry a search left,
                // its key chip at the list's foot; a heading row as short as
                // its line and the group gap lets both fit, as at 18 pt in a
                // 461 × 490 window after a search for "command palette".
                this.scroll.set_offset(point(px(0.), px(0.)));
                this.scroll
                    .scroll_to_item(this.selected, ScrollStrategy::Bottom);
                cx.notify();
            }
        });
        let rows = help_matches("");
        Self {
            query,
            layout: RowLayout::new(&rows),
            rows,
            selected: 1,
            scroll: gpui_kit::base::VirtualListScrollHandle::new(),
            list_height: Default::default(),
            return_focus,
            closed: false,
            _subscription: subscription,
        }
    }

    /// Lists `rows`, with their heights at the interface size.
    fn set_rows(&mut self, rows: Vec<HelpRow>) {
        self.layout = RowLayout::new(&rows);
        self.rows = rows;
    }

    /// Measures the rows again when the interface size has changed since
    /// they were, as when a text size is chosen with the dialog open.
    fn measure(&mut self) {
        if appearance::ui_scale().to_bits() != self.layout.scale.to_bits() {
            self.layout = RowLayout::new(&self.rows);
        }
    }

    /// Brings the selected entry into view, `how` where any of it lies
    /// outside the list, and leaves the list where it lies whole in it: Up
    /// and Down centre it, as the palette's and Quick Open's lists do, and a
    /// click scrolls the least. The toolkit's virtual list centres on every
    /// request, so holding Down kept the highlight at the list's middle and
    /// scrolled the list on each press.
    ///
    /// A group's first entry brings its group's heading with it, as a search
    /// does, where the list holds both (`Span`): counted whole on its own,
    /// the entry left its heading as the wheel had cut it at the list's top,
    /// and nothing brought the heading back. The toolkit brings a single row
    /// into view, so the scroll is set here.
    ///
    /// A key reads the list as laid out for it: GPUI draws a frame still
    /// pending before it dispatches a key (`Window::dispatch_key_event`). The
    /// scroll the press before it set and the list's height after a text size
    /// chosen with the dialog open, which refreshes the window
    /// (`appearance::apply_text_sizes`), are laid out by then, and `measure`
    /// has the rows' heights at that size.
    fn reveal_selected(&mut self, how: Reveal) {
        self.measure();
        let Some(span) = self.layout.span(&self.rows, self.selected) else {
            return;
        };
        // How far the list is scrolled, and its height, as last laid out.
        let (offset, view) = (self.scroll.offset(), self.scroll.bounds().size.height);
        if let Some(to) = span.reveal(-offset.y, view, self.layout.content(), how) {
            self.scroll.set_offset(point(offset.x, -to));
        }
    }

    /// How many entries Page Up and Page Down move: as many as the list's
    /// height holds whole rows, at least one, as a page in a macOS list. A
    /// heading's shorter row is not counted: the selection passes over it.
    fn page(&self) -> usize {
        let view = self.scroll.bounds().size.height;
        ((view / appearance::ui_size(SHORTCUT_ROW)).floor() as usize).max(1)
    }

    /// Moves the selection by `step` and brings the entry it reaches into
    /// view by Up's and Down's rules (`reveal_selected`): an entry outside
    /// the list or cut by its edge is centred, a group's first entry with
    /// its heading, and the list's ends hold it, so End leaves the last entry
    /// whole at the list's foot and Home the first heading at its top. Up and
    /// Down at either end move nothing; Home, End, Page Up and Page Down,
    /// which name a place rather than a neighbour, show their entry even
    /// where the selection is already on it, as after the wheel moved the
    /// list away from it.
    fn step(&mut self, step: Step, cx: &mut Context<Self>) {
        let entries: Vec<usize> = (0..self.rows.len())
            .filter(|&row| matches!(self.rows[row], HelpRow::Entry(_)))
            .collect();
        let (Some(last), Some(at)) = (
            entries.len().checked_sub(1),
            entries.iter().position(|&row| row == self.selected),
        ) else {
            return;
        };
        let to = match step {
            Step::Previous => at.saturating_sub(1),
            Step::Next => (at + 1).min(last),
            Step::PageUp => at.saturating_sub(self.page()),
            Step::PageDown => (at + self.page()).min(last),
            Step::First => 0,
            Step::Last => last,
        };
        if to == at && matches!(step, Step::Previous | Step::Next) {
            return;
        }
        self.selected = entries[to];
        self.reveal_selected(Reveal::Centre);
        cx.notify();
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
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use gpui_kit::prelude::FluentBuilder;
        let p = palette(cx);
        let count = self
            .rows
            .iter()
            .filter(|row| matches!(row, HelpRow::Entry(_)))
            .count();
        // The list asks for its full height and gives up what the dialog's
        // height cap (`appearance::dialog_max_height`) takes, down to its
        // floor, by layout: the chrome around it, such as the hint that wraps
        // in a narrow window, is never estimated.
        let height = list_height();
        self.measure();
        let list = gpui_kit::base::v_virtual_list(
            cx.entity(),
            "shortcut-help-list",
            self.layout.sizes.clone(),
            |this, range: std::ops::Range<usize>, _, cx| {
                let p = palette(cx);
                // Fitted once for the rows drawn, not for each row.
                let (edge, selected_edge) = (p.raised_edge(), p.selected_edge());
                range
                    .map(|index| {
                        let row = div()
                            .id(("shortcut-help-row", index))
                            .h(row_height(&this.rows[index]))
                            .w_full()
                            .px_3()
                            .flex()
                            .gap_4();
                        match &this.rows[index] {
                            // The group gap above the name, which sits on its
                            // rule.
                            HelpRow::Group(group) => row
                                .debug_selector(move || format!("shortcut-help-heading-{index}"))
                                .items_end()
                                .pb(appearance::ui_size(SHORTCUT_HEADING_BELOW))
                                .role(Role::Heading)
                                .aria_label(*group)
                                .text_size(appearance::ui_text(11.))
                                .line_height(appearance::ui_size(SHORTCUT_HEADING_LINE))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(p.muted))
                                // Ruled with the edge, as a menu's
                                // separators are.
                                .border_b_1()
                                .border_color(rgb(edge))
                                .child(*group),
                            HelpRow::Entry(entry) => row
                                .items_center()
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
                                // A click on an entry the list's edge cuts,
                                // or on a group's first entry whose heading
                                // it cuts, scrolls the least that shows it
                                // whole; it once scrolled nothing, leaving
                                // the heading cut.
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.selected = index;
                                    this.reveal_selected(Reveal::Nearest);
                                    cx.notify();
                                })),
                        }
                        .into_any_element()
                    })
                    .collect::<Vec<_>>()
            },
        )
        .track_scroll(&self.scroll)
        .size_full();
        div().id("keyboard-shortcuts-view").debug_selector(|| "keyboard-shortcuts-view".into()).key_context("GitTurtleShortcuts")
            .flex().flex_col().min_h_0().gap_3().pt(appearance::button_ring_room(cx)).text_size(appearance::ui_text(12.))
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
                let step = match event.keystroke.key.as_str() { "down" => Step::Next, "up" => Step::Previous, _ => return };
                this.step(step, cx);
                cx.stop_propagation();
            }))
            .on_action(cx.listener(|this, _: &FirstShortcut, _, cx| this.step(Step::First, cx)))
            .on_action(cx.listener(|this, _: &LastShortcut, _, cx| this.step(Step::Last, cx)))
            .on_action(cx.listener(|this, _: &PreviousShortcutPage, _, cx| this.step(Step::PageUp, cx)))
            .on_action(cx.listener(|this, _: &NextShortcutPage, _, cx| this.step(Step::PageDown, cx)))
            .child(Input::new(&self.query).aria_label("Search keyboard shortcuts").cleanable(true))
            // A status read by the line it draws: it was named "61 matching
            // shortcuts" while it drew "61 shortcuts · Ctrl, Alt and Shift".
            .child({
                let line = count_line(count);
                native_accessibility::static_text(div().id("shortcut-help-count").role(Role::Status)
                    .polite(), line.clone())
                    .text_color(rgb(p.muted)).child(line)
            })
            .child(crate::native_accessibility::clips_children(div().id("shortcut-help-results").debug_selector(|| "shortcut-help-results".into()).role(Role::ListBox)
                .aria_label("Keyboard shortcuts by group").h(height).min_h(appearance::ui_size(SHORTCUT_LIST_FLOOR)).overflow_hidden()
                // Prepainted before the rows, so the scroll lands in the
                // frame whose height changed: a list given up to its floor,
                // as at 18 pt in a 461 × 490 window, shows the selected entry
                // whole rather than its group's heading over a sliver of it.
                // It scrolls the least that shows the entry whole, with its
                // group's heading where it is the group's first and the list
                // holds both: the toolkit's nearest row brought a first entry
                // the wheel had passed back alone to the list's top, its
                // heading cut above it.
                //
                // A window resized under the list moves it only to keep a
                // selected entry it showed whole from being cut, and
                // otherwise leaves it where the wheel or a key left it, as
                // the toolkit clamps it to its rows: the person put it
                // there. It brought the selection back on every frame of a
                // resize, undoing the wheel. As the list opens, and after a
                // text size chosen with it open, which changes every row's
                // height under the scroll, the selection comes into view.
                .on_prepaint({
                    let (seen, scroll) = (self.list_height.clone(), self.scroll.clone());
                    let (span, content, scale) = (self.layout.span(&self.rows, self.selected), self.layout.content(), self.layout.scale);
                    move |bounds, _, _| {
                        // The canvas fills the list inside its 1 px border,
                        // as the rows' list does, so its height is the
                        // list's view. Taken 2 px shorter, it ended an entry
                        // it revealed from below 2 px above the list's foot
                        // and, at the floor, cut the entry's top.
                        let view = bounds.size.height;
                        let (was, measured) = seen.replace((view, scale));
                        if was != view && let Some(span) = span {
                            let offset = scroll.offset();
                            let resized = was > px(0.) && measured.to_bits() == scale.to_bits();
                            let cut = span.whole(-offset.y, was) && !span.whole(-offset.y, view);
                            if (!resized || cut) && let Some(to) = span.reveal(-offset.y, view, content, Reveal::Nearest) {
                                scroll.set_offset(point(offset.x, -to));
                            }
                        }
                    }
                })
                .rounded(px(7.)).border_1().border_color(rgb(p.raised_edge()))
                // The list shows its scroll while it holds more than it
                // draws, as the message and the Reflog's lists do: given up
                // to a few rows, as three of 61 at 18 pt in a 461 × 490
                // window, it showed no sign of the rest. The bar hides while
                // every row fits, and its thumb stands clear of the key chips,
                // which keep the rows' 12 px inset.
                .relative()
                .when(count > 0, |view| view.child(list).child(
                    gpui_kit::base::Scrollbar::vertical(&self.scroll)
                        .id("shortcut-help-scrollbar")
                        .mode(gpui_kit::base::ScrollbarMode::Always)
                        // The track rests on the picker's raised surface, as
                        // the dialogs' list tracks rest on theirs; the kit's
                        // track is the page's canvas.
                        .styles(|styles| styles.track(|track| track.bg(rgb(p.raised()).into()))),
                ))
                .when(count == 0, |view| view.child(native_accessibility::text("shortcut-help-empty",
                    if self.query.read(cx).value().len() > 4096 { "Search is limited to 4,096 bytes." }
                    else { "No shortcuts match. Try tabs, compare, editing or Ctrl." }).p_4().text_color(rgb(p.muted))))))
            .child(div().id("shortcut-help-selected").role(Role::Status)
                .polite()
                .aria_label(match self.rows.get(self.selected) {
                    Some(HelpRow::Entry(entry)) => format!("{}: {}", entry.label, entry.key),
                    _ => String::new(),
                }).h(px(1.)).overflow_hidden())
            .child(div().text_size(appearance::ui_text(11.)).text_color(rgb(p.muted))
                .child(dialog_help("shortcut-help-keys", format!("↑ / ↓ browse · {} · Shortcuts act in the focused view; text editing keeps its own keys.", DialogKeys::Shortcuts.escape_help()))))
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

#[cfg(test)]
mod list_tests {
    use super::*;
    use crate::tags::tests::{draw, scroll_down, tagged_repository, window};
    use core::prelude::v1::test;

    /// How far apart two drawn edges may read and still meet: GPUI rounds
    /// each edge to the device pixel, half a pixel in the test window, and
    /// lays each of the list's rows out on its own, so at 18 pt a row's top
    /// can read half a pixel below the foot of the row above it. A whole
    /// pixel let a list that cut its entry 1 px at its foot pass for one
    /// that ended the entry there. An edge that must not be cut is checked
    /// on its own side, with the list's own allowance (`Shown::whole`).
    const EDGE: Pixels = px(0.5);

    /// The selector of the row at `index` among `rows`.
    fn row_selector(rows: &[HelpRow], index: usize) -> &'static str {
        match rows[index] {
            HelpRow::Group(_) => format!("shortcut-help-heading-{index}"),
            HelpRow::Entry(_) => format!("shortcut-help-row-{index}"),
        }
        .leak()
    }

    /// The list keeps each row's height beside its rows, so a text size
    /// chosen with the dialog open measures them again: at 18 pt each row is
    /// drawn where the one above it ends, the first at the list's top, and
    /// grown with the text, the General heading's 22 px row to about 30 px.
    /// Measured only with the rows, the list would place them at their 13 pt
    /// tops while drawing them at 18 pt, each over the one above.
    #[gpui::test]
    async fn a_text_size_chosen_with_the_list_open_lays_its_rows_out_again(
        cx: &mut TestAppContext,
    ) {
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        let (app, cx) = window(cx, &repo);
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
        cx.simulate_resize(size(px(1200.), px(800.)));
        cx.update(|window, cx| app.update(cx, |app, cx| app.shortcut_help(window, cx)));
        draw(cx);
        let rows = help_matches("");
        let mut heights = Vec::new();
        for text in [13, 18] {
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            draw(cx);
            let list = cx.debug_bounds("shortcut-help-results").expect("the list");
            // Inside the list's 1 px border.
            let mut top = list.top() + px(1.);
            // The first group's heading and the entries below it.
            for index in 0..6 {
                let bounds = cx
                    .debug_bounds(row_selector(&rows, index))
                    .unwrap_or_else(|| panic!("{text} pt: row {index} is drawn"));
                assert!(
                    (bounds.top() - top).abs() <= EDGE,
                    "{text} pt: row {index} {bounds:?} starts where the row above it ends, \
                     {top:?}"
                );
                if text == 13 {
                    heights.push(bounds.size.height);
                } else {
                    let grown = heights[index] * (18. / 13.);
                    assert!(
                        (bounds.size.height - grown).abs() <= EDGE,
                        "18 pt: row {index}, {:?} tall at 13 pt, grows to {grown:?}: {:?}",
                        heights[index],
                        bounds.size.height
                    );
                }
                top = bounds.bottom();
            }
        }
        cx.simulate_keystrokes("escape");
        draw(cx);
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// Opens Keyboard Shortcuts at `text` pt in a `width` × `height` window.
    fn open(
        app: &Entity<GitTurtle>,
        cx: &mut VisualTestContext,
        text: u8,
        (width, height): (f32, f32),
    ) {
        cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
        cx.simulate_resize(size(px(width), px(height)));
        cx.update(|window, cx| app.update(cx, |app, cx| app.shortcut_help(window, cx)));
        draw(cx);
    }

    /// Closes the dialog and restores the default text size.
    fn close(cx: &mut VisualTestContext) {
        cx.simulate_keystrokes("escape");
        draw(cx);
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// Presses `keys`, then draws.
    fn press(cx: &mut VisualTestContext, keys: &str) {
        cx.simulate_keystrokes(keys);
        draw(cx);
    }

    /// The open list's rows for a search, and where the list and its rows
    /// are drawn. Every expectation is read from the drawn rows and the
    /// list's own bounds, never from the list's layout (`RowLayout`), which a
    /// wrong layout would agree with.
    struct Shown {
        rows: Vec<HelpRow>,
        /// The list's top and foot inside its 1 px border, and its centre.
        top: Pixels,
        foot: Pixels,
        centre: Point<Pixels>,
        /// A heading's and an entry's height as drawn.
        heading: Pixels,
        entry: Pixels,
    }

    impl Shown {
        /// Reads the list as it opens, at its top, where its first rows, a
        /// heading and an entry, give the rows' heights.
        fn read(cx: &mut VisualTestContext, query: &str) -> Self {
            let mut shown = Self {
                rows: help_matches(query),
                top: px(0.),
                foot: px(0.),
                centre: point(px(0.), px(0.)),
                heading: px(0.),
                entry: px(0.),
            };
            shown.relist(cx);
            shown.heading = shown.at(cx, 0).size.height;
            shown.entry = shown.at(cx, 1).size.height;
            shown
        }

        /// Reads where the list is drawn again, as after the window's size
        /// changed.
        fn relist(&mut self, cx: &mut VisualTestContext) {
            let list = cx.debug_bounds("shortcut-help-results").expect("the list");
            self.top = list.top() + px(1.);
            self.foot = list.bottom() - px(1.);
            self.centre = list.center();
        }

        /// The list's height inside its border.
        fn view(&self) -> Pixels {
            self.foot - self.top
        }

        /// The least height of the 1200 px wide window whose list is `view`
        /// tall inside its border or more, found by resizing the window,
        /// halving between a 300 px tall one, whose list is at its floor, and
        /// an 800 px one: the dialog's height cap takes a tenth of the
        /// window's height, snapped to the half pixel, besides the chrome.
        fn size_to(&mut self, cx: &mut VisualTestContext, view: Pixels) -> f32 {
            let (mut short, mut tall) = (300., 800.);
            for _ in 0..16 {
                let height = (short + tall) / 2.;
                cx.simulate_resize(size(px(1200.), px(height)));
                draw(cx);
                self.relist(cx);
                if self.view() < view {
                    short = height;
                } else {
                    tall = height;
                }
            }
            tall
        }

        /// The entries, in the list's order.
        fn entries(&self) -> Vec<usize> {
            (0..self.rows.len())
                .filter(|&row| matches!(self.rows[row], HelpRow::Entry(_)))
                .collect()
        }

        /// The rows that are group headings, in the list's order.
        fn headings(&self) -> Vec<usize> {
            (0..self.rows.len())
                .filter(|&row| matches!(self.rows[row], HelpRow::Group(_)))
                .collect()
        }

        /// Where row `index` is drawn, if it is.
        fn drawn(&self, cx: &mut VisualTestContext, index: usize) -> Option<Bounds<Pixels>> {
            cx.debug_bounds(row_selector(&self.rows, index))
        }

        /// Where row `index` is drawn, which it must be.
        fn at(&self, cx: &mut VisualTestContext, index: usize) -> Bounds<Pixels> {
            self.drawn(cx, index)
                .unwrap_or_else(|| panic!("row {index} is drawn"))
        }

        /// Whether row `index` is drawn whole in the list, as the list counts
        /// a row whole, no further past its edge than its own allowance
        /// (`Span::ROUNDING`). The window draws a row at its top rounded to
        /// the half pixel with its height rounded on its own, so its drawn
        /// top is its top rounded but its drawn bottom is not its bottom
        /// rounded. A row the list cuts by half a pixel or more is drawn cut
        /// at either edge, and one it holds whole is never drawn above the
        /// list's top; but where a row's height rounds up by more than a
        /// quarter pixel less the allowance, a row the list holds whole can be
        /// drawn half a pixel past the list's foot. The allowance holds here
        /// because the list tests' text sizes round no row's height up by
        /// more than 0.04 px: at 13 pt the rows are 22 and 42 px, and at 18 pt
        /// about 30.46 px, drawn 30.5, and 58.15 px, drawn 58.
        fn whole(&self, cx: &mut VisualTestContext, index: usize) -> bool {
            self.drawn(cx, index).is_some_and(|row| {
                row.top() >= self.top - Span::ROUNDING && row.bottom() <= self.foot + Span::ROUNDING
            })
        }

        /// The heading that comes into view with `entry`: its group's, where
        /// it is the group's first entry and the list holds both.
        fn heading(&self, entry: usize) -> Option<usize> {
            (matches!(self.rows[entry - 1], HelpRow::Group(_))
                && self.heading + self.entry <= self.view() + EDGE)
                .then_some(entry - 1)
        }

        /// Whether `entry` lies whole in the list with the heading that comes
        /// into view with it.
        fn shown(&self, cx: &mut VisualTestContext, entry: usize) -> bool {
            self.whole(cx, entry)
                && self
                    .heading(entry)
                    .is_none_or(|heading| self.whole(cx, heading))
        }

        /// Asserts that `entry` and the heading that comes into view with it
        /// lie whole in the list, centred together, or as near the centre as
        /// the list's ends let them: above it with the list at its top, its
        /// first row drawn there, or below it with the list at its end, its
        /// last row's foot at the list's.
        fn centred(&self, cx: &mut VisualTestContext, entry: usize, what: &str) {
            let row = self.at(cx, entry);
            let top = self
                .heading(entry)
                .map_or(row.top(), |heading| self.at(cx, heading).top());
            let (above, below) = (top - self.top, self.foot - row.bottom());
            let start = self
                .drawn(cx, 0)
                .is_some_and(|first| (first.top() - self.top).abs() <= EDGE);
            let end = self
                .drawn(cx, self.rows.len() - 1)
                .is_some_and(|last| (last.bottom() - self.foot).abs() <= EDGE);
            assert!(
                above >= -Span::ROUNDING
                    && below >= -Span::ROUNDING
                    && ((above - below).abs() <= EDGE
                        || (start && above < below)
                        || (end && below < above)),
                "{what}: entry {entry} and what comes with it lie {above:?} below the \
                 list's top and {below:?} above its foot"
            );
        }

        /// The entry drawn as the keyboard's cursor, with the selection's
        /// fill, if one is drawn.
        fn selected(&self, cx: &mut VisualTestContext) -> Option<usize> {
            let fill = cx.update(|_, cx| Background::from(Hsla::from(rgb(palette(cx).selected))));
            self.entries().into_iter().find(|&entry| {
                self.drawn(cx, entry)
                    .is_some_and(|row| appearance::painted_row(cx, row).0.contains(&fill))
            })
        }

        /// Wheels the list `by` down it (up where negative), checking that row
        /// `index`, drawn before and after, moved that far.
        fn wheel(&self, cx: &mut VisualTestContext, by: Pixels, index: usize) {
            let before = self.at(cx, index).top();
            scroll_down(cx, self.centre, ScrollDelta::Pixels(point(px(0.), -by)));
            let moved = before - self.at(cx, index).top();
            assert!(
                (moved - by).abs() <= EDGE,
                "the wheel scrolls the list {by:?}: row {index} moved {moved:?}"
            );
        }
    }

    /// Up and Down move the highlight through the entries the list shows
    /// whole without scrolling it, as the palette's and Quick Open's lists
    /// do, and scroll it only for an entry outside it or cut by its edge,
    /// centring that entry, with its group's heading where it is the group's
    /// first: at 13 pt in a 1200 × 800 window, Down from the first entry
    /// keeps the list at its top until it passes the foot, and Up from a
    /// scrolled list keeps it still until it passes the top. The toolkit's
    /// virtual list centred the entry on every press, so holding Down kept
    /// the highlight at the list's middle and scrolled it each time.
    #[gpui::test]
    async fn up_and_down_scroll_the_list_only_past_its_edges(cx: &mut TestAppContext) {
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        let (app, cx) = window(cx, &repo);
        open(&app, cx, 13, (1200., 800.));
        let shown = Shown::read(cx, "");
        let entries = shown.entries();
        let held = entries
            .iter()
            .filter(|&&entry| shown.whole(cx, entry))
            .count();
        assert!(
            held * 3 < entries.len(),
            "the list scrolls: it holds {held} of {} entries",
            entries.len()
        );
        let mut selected = entries[0];
        // Down three times past the list's foot, then Up once past its top.
        for (key, scrolls) in [("down", 3), ("up", 1)] {
            let (mut kept, mut moved) = (0, 0);
            while moved < scrolls {
                let at = entries.iter().position(|&entry| entry == selected).unwrap();
                let next = entries[if key == "down" { at + 1 } else { at - 1 }];
                let inside = shown.shown(cx, next);
                let before = shown.drawn(cx, next).map(|row| row.top());
                press(cx, key);
                let now = shown.at(cx, next).top();
                if inside {
                    let before = before.unwrap();
                    assert!(
                        (now - before).abs() <= EDGE,
                        "{key} to entry {next}, whole in the list, moved it from \
                         {before:?} to {now:?}"
                    );
                    kept += 1;
                } else {
                    let what = format!("{key} to entry {next}, outside the list");
                    shown.centred(cx, next, &what);
                    assert!(
                        before.is_none_or(|before| (now - before).abs() > EDGE),
                        "{what} scrolls it: the entry stayed at {now:?}"
                    );
                    moved += 1;
                }
                selected = next;
            }
            assert!(kept >= 3, "{key} kept the list still for {kept} entries");
        }
        close(cx);
    }

    /// Up onto a group's first entry brings back the group's heading that
    /// the wheel left cut at the list's top, centring the two together: at
    /// 13 pt in a 1200 × 800 window, with each of the first three groups'
    /// headings 10 px above the list's top and the second entry selected,
    /// Up shows the heading whole. Counted whole on its own, the first entry
    /// left the heading cut, and no key brought it back.
    #[gpui::test]
    async fn up_onto_a_groups_first_entry_brings_its_heading_back(cx: &mut TestAppContext) {
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        let (app, cx) = window(cx, &repo);
        open(&app, cx, 13, (1200., 800.));
        let shown = Shown::read(cx, "");
        let entries = shown.entries();
        let headings: Vec<usize> = shown
            .headings()
            .into_iter()
            .filter(|&row| matches!(shown.rows.get(row + 2), Some(HelpRow::Entry(_))))
            .take(3)
            .collect();
        assert_eq!(headings.len(), 3, "three groups of two entries or more");
        let mut selected = entries[0];
        for heading in headings {
            let (first, second) = (heading + 1, heading + 2);
            while selected != second {
                press(cx, "down");
                selected = entries[entries.iter().position(|&e| e == selected).unwrap() + 1];
            }
            // The heading stands an entry and its own height above the second
            // entry; the wheel leaves it 10 px above the list's top.
            let top = shown.at(cx, second).top() - shown.entry - shown.heading;
            shown.wheel(cx, top - (shown.top - px(10.)), second);
            let cut = shown.at(cx, heading);
            assert!(
                (cut.top() - (shown.top - px(10.))).abs() <= EDGE
                    && shown.whole(cx, first)
                    && shown.whole(cx, second),
                "group {heading}: the wheel cuts its heading 10 px, {cut:?} in the list \
                 from {:?}, above its whole entries",
                shown.top
            );
            press(cx, "up");
            selected = first;
            let what = format!("group {heading}: Up onto its first entry, heading cut 10 px");
            shown.centred(cx, first, &what);
        }
        close(cx);
    }

    /// Presses delivered back to back end where they do with a frame drawn
    /// after each, each deciding against the scroll the one before it set:
    /// GPUI draws the frame a press leaves pending before it dispatches the
    /// next key. At 13 pt in a 1200 × 800 window, Down twice past the list's
    /// foot centres the first entry past it and keeps the second whole below,
    /// and Up past the top then Down back leaves the entry above centred.
    #[gpui::test]
    async fn presses_back_to_back_end_where_they_do_a_frame_apiece(cx: &mut TestAppContext) {
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        let (app, cx) = window(cx, &repo);
        let mut ends = Vec::new();
        for one_frame in [false, true] {
            let frames = if one_frame {
                "back to back"
            } else {
                "a frame apiece"
            };
            open(&app, cx, 13, (1200., 800.));
            let shown = Shown::read(cx, "");
            let entries = shown.entries();
            // Down to the last entry the list holds whole at its top.
            let last = entries
                .iter()
                .rposition(|&entry| shown.shown(cx, entry))
                .unwrap();
            let rest = shown.at(cx, entries[last]).top();
            for _ in 0..last {
                press(cx, "down");
            }
            assert!(
                (shown.at(cx, entries[last]).top() - rest).abs() <= EDGE,
                "Down to entry {} keeps the list at its top",
                entries[last]
            );
            let (past, beyond) = (entries[last + 1], entries[last + 2]);
            if one_frame {
                press(cx, "down down");
            } else {
                press(cx, "down");
                press(cx, "down");
            }
            shown.centred(cx, past, &format!("Down twice {frames}"));
            assert!(
                shown.shown(cx, beyond),
                "Down twice {frames} keeps entry {beyond} whole"
            );
            let down = shown.at(cx, beyond).top() - shown.top;
            // The wheel leaves `beyond`, with the heading that comes into
            // view with it, at the list's top, `past` above it.
            let top = match shown.heading(beyond) {
                Some(heading) => shown.at(cx, heading).top(),
                None => shown.at(cx, beyond).top(),
            };
            shown.wheel(cx, top - shown.top, beyond);
            if one_frame {
                press(cx, "up down");
            } else {
                press(cx, "up");
                press(cx, "down");
            }
            shown.centred(cx, past, &format!("Up then Down {frames}"));
            assert!(
                shown.shown(cx, beyond),
                "Up then Down {frames} keeps entry {beyond} whole"
            );
            let back = shown.at(cx, beyond).top() - shown.top;
            ends.push((down, back));
            close(cx);
        }
        assert!(
            (ends[0].0 - ends[1].0).abs() <= EDGE && (ends[0].1 - ends[1].1).abs() <= EDGE,
            "a frame apiece ends with the entry past them at {:?}, back to back at {:?}",
            ends[0],
            ends[1]
        );
    }

    /// Up and Down read a scroll the wheel set: at 13 pt in a 1200 × 800
    /// window, with the wheel leaving an entry cut 3 px by the list's top,
    /// Down and Up through the whole entries below it keep the list still,
    /// and Up onto the cut entry centres it.
    #[gpui::test]
    async fn a_wheeled_list_keeps_whole_entries_still_and_centres_a_cut_one(
        cx: &mut TestAppContext,
    ) {
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        let (app, cx) = window(cx, &repo);
        open(&app, cx, 13, (1200., 800.));
        let shown = Shown::read(cx, "");
        let entries = shown.entries();
        // An entry below the list's first screen, not its group's first,
        // with two entries of its group below it.
        let at = (1..entries.len() - 2)
            .find(|&at| {
                let entry = entries[at];
                shown
                    .drawn(cx, entry)
                    .is_none_or(|row| row.top() >= shown.foot - EDGE)
                    && matches!(shown.rows[entry - 1], HelpRow::Entry(_))
                    && entries[at + 2] == entry + 2
            })
            .expect("an entry the wheel can cut");
        let (cut, below) = (entries[at], entries[at + 1]);
        for _ in 0..=at {
            press(cx, "down");
        }
        let by = shown.at(cx, cut).top() - shown.top + px(3.);
        shown.wheel(cx, by, below);
        let wheeled = shown.at(cx, below).top();
        for (key, entry) in [("down", entries[at + 2]), ("up", below)] {
            press(cx, key);
            let now = shown.at(cx, below).top();
            assert!(
                (now - wheeled).abs() <= EDGE,
                "{key} to entry {entry}, whole in the list the wheel scrolled, moved \
                 entry {below} from {wheeled:?} to {now:?}"
            );
        }
        press(cx, "up");
        shown.centred(
            cx,
            cut,
            &format!("Up to entry {cut}, cut 3 px by the list's top"),
        );
        assert!(
            (shown.at(cx, below).top() - wheeled).abs() > EDGE,
            "Up to entry {cut}, cut 3 px by the list's top, scrolls the list"
        );
        close(cx);
    }

    /// A window resized under a list the wheel moved away from the selected
    /// entry keeps the list where the wheel left it, clamped to its rows:
    /// at 13 and 18 pt, with the second group's first entry selected and the
    /// wheel leaving it 5 px above a 1200 × 800 window's list, a 560 px tall
    /// window and then the 800 px one again leave every row where it was
    /// drawn from the list's top, the entry and its heading still above it;
    /// and with the wheel at the 560 px list's end, the 800 px window keeps
    /// the last row's foot at the list's foot. A resize brought the
    /// selection back on each frame, undoing the wheel.
    #[gpui::test]
    async fn a_resize_keeps_the_wheels_scroll_where_the_selection_lies_outside_the_list(
        cx: &mut TestAppContext,
    ) {
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        let (app, cx) = window(cx, &repo);
        for text in [13, 18] {
            open(&app, cx, text, (1200., 800.));
            let mut shown = Shown::read(cx, "");
            let entries = shown.entries();
            let heading = shown.headings()[1];
            let first = heading + 1;
            for _ in 0..entries.iter().position(|&entry| entry == first).unwrap() {
                press(cx, "down");
            }
            // The wheel leaves the entry 5 px above the list's top, its
            // heading above it; the row below it is cut 5 px there.
            let below = first + 1;
            let by = shown.at(cx, first).bottom() - shown.top + px(5.);
            shown.wheel(cx, by, below);
            let rest = shown.at(cx, below).top() - shown.top;
            for (height, list) in [(560., "shorter"), (800., "taller")] {
                let view = shown.view();
                cx.simulate_resize(size(px(1200.), px(height)));
                draw(cx);
                shown.relist(cx);
                assert!(
                    (shown.view() - view).abs() > EDGE,
                    "{text} pt: a {height} px tall window gives the list {:?}, not {view:?}",
                    shown.view()
                );
                let now = shown.at(cx, below).top() - shown.top;
                assert!(
                    (now - rest).abs() <= EDGE
                        && !shown.whole(cx, first)
                        && !shown.whole(cx, heading),
                    "{text} pt: a {list} list keeps the wheel's scroll, row {below} {rest:?} \
                     below its top, with entry {first} and its heading above it: row {below} \
                     is drawn {now:?} below it"
                );
            }
            cx.simulate_resize(size(px(1200.), px(560.)));
            draw(cx);
            shown.relist(cx);
            // Far past the list's end, which the list holds.
            scroll_down(
                cx,
                shown.centre,
                ScrollDelta::Pixels(point(px(0.), px(-100_000.))),
            );
            let last = shown.rows.len() - 1;
            let ends = |shown: &Shown, cx: &mut VisualTestContext| {
                shown.drawn(cx, last).map(|row| row.bottom() - shown.foot)
            };
            assert!(
                ends(&shown, cx).is_some_and(|off| off.abs() <= EDGE),
                "{text} pt: the wheel takes the list to its end: row {last} ends {:?} off \
                 its foot",
                ends(&shown, cx)
            );
            cx.simulate_resize(size(px(1200.), px(800.)));
            draw(cx);
            shown.relist(cx);
            assert!(
                ends(&shown, cx).is_some_and(|off| off.abs() <= EDGE) && !shown.whole(cx, first),
                "{text} pt: a taller list keeps its end at its foot, entry {first} above \
                 it: row {last} ends {:?} off its foot",
                ends(&shown, cx)
            );
            close(cx);
        }
    }

    /// A window resized under a list that shows the selected entry whole
    /// brings it back only where the new height would cut it: at 13 and
    /// 18 pt, with the second group's first entry selected and the wheel
    /// cutting its heading 10 px at a 1200 × 800 window's list's top, a
    /// 560 px tall window, whose list still holds the entry whole, moves
    /// nothing and leaves the heading cut, and a 300 px one, whose list at
    /// its floor would cut the entry's foot, brings the entry whole to the
    /// list's foot. Before, the 560 px window brought the heading back.
    #[gpui::test]
    async fn a_resize_brings_back_only_a_selected_entry_it_would_cut(cx: &mut TestAppContext) {
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        let (app, cx) = window(cx, &repo);
        for text in [13, 18] {
            open(&app, cx, text, (1200., 800.));
            let mut shown = Shown::read(cx, "");
            let entries = shown.entries();
            let heading = shown.headings()[1];
            let first = heading + 1;
            for _ in 0..entries.iter().position(|&entry| entry == first).unwrap() {
                press(cx, "down");
            }
            let by = shown.at(cx, heading).top() - (shown.top - px(10.));
            shown.wheel(cx, by, first);
            assert!(
                !shown.whole(cx, heading) && shown.whole(cx, first),
                "{text} pt: the wheel cuts group {heading}'s heading above entry {first}"
            );
            let rest = shown.at(cx, heading).top() - shown.top;
            cx.simulate_resize(size(px(1200.), px(560.)));
            draw(cx);
            shown.relist(cx);
            let now = shown.at(cx, heading).top() - shown.top;
            assert!(
                (now - rest).abs() <= EDGE && shown.whole(cx, first),
                "{text} pt: a shorter list that holds entry {first} whole keeps the wheel's \
                 scroll, group {heading}'s heading {rest:?} from its top: {now:?}"
            );
            cx.simulate_resize(size(px(1200.), px(300.)));
            draw(cx);
            shown.relist(cx);
            let drawn = shown.drawn(cx, first);
            assert!(
                drawn.is_some_and(|row| (row.bottom() - shown.foot).abs() <= EDGE)
                    && shown.whole(cx, first),
                "{text} pt: a list at its floor, {:?}, that would cut entry {first} brings it \
                 whole to its foot, {:?}: {drawn:?}",
                shown.view(),
                shown.foot
            );
            close(cx);
        }
    }

    /// Whether a resize cuts the selected entry is judged at the list's
    /// height to the pixel: at 13 and 18 pt, with the wheel bringing the
    /// second group's heading to a 1200 × 800 window's list's top and its
    /// first entry, selected, whole below it, a window whose list is a pixel
    /// taller than the heading and the entry keeps both whole where they
    /// were, and one whose list is a pixel shorter, which would cut the
    /// entry's foot, brings the entry whole to its foot, the heading cut a
    /// pixel above its top. Judged 2 px short, the taller list counted the
    /// entry cut and moved it, its heading above the list; judged 2 px long,
    /// the shorter one left the entry cut at its foot. No other list test's
    /// list lies within 2 px of both.
    #[gpui::test]
    async fn a_list_a_pixel_past_a_heading_and_an_entry_keeps_both_and_one_a_pixel_short_the_entry(
        cx: &mut TestAppContext,
    ) {
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        let (app, cx) = window(cx, &repo);
        for text in [13, 18] {
            open(&app, cx, text, (1200., 800.));
            let mut shown = Shown::read(cx, "");
            let both = shown.heading + shown.entry;
            let windows = [(px(1.), "taller"), (px(-1.), "shorter")]
                .map(|(by, list)| (shown.size_to(cx, both + by), both + by, list));
            close(cx);
            let entries = shown.entries();
            let heading = shown.headings()[1];
            let first = heading + 1;
            for (height, view, list) in windows {
                open(&app, cx, text, (1200., 800.));
                shown.relist(cx);
                for _ in 0..entries.iter().position(|&entry| entry == first).unwrap() {
                    press(cx, "down");
                }
                let by = shown.at(cx, heading).top() - shown.top;
                shown.wheel(cx, by, first);
                cx.simulate_resize(size(px(1200.), px(height)));
                draw(cx);
                shown.relist(cx);
                assert!(
                    (shown.view() - view).abs() < EDGE,
                    "{text} pt: a {height} px tall window gives the list {view:?}, a pixel \
                     {list} than a heading and an entry, {both:?}: {:?}",
                    shown.view()
                );
                // Where the list holds both, the heading stays at its top;
                // else the entry comes whole to its foot.
                let holds = view > both;
                let (row, edge, off) = if holds {
                    let off = shown.drawn(cx, heading).map(|row| row.top() - shown.top);
                    (heading, "top", off)
                } else {
                    let off = shown.drawn(cx, first).map(|row| row.bottom() - shown.foot);
                    (first, "foot", off)
                };
                assert!(
                    off.is_some_and(|off| off.abs() <= EDGE)
                        && shown.whole(cx, first)
                        && shown.whole(cx, heading) == holds,
                    "{text} pt: a list a pixel {list} than group {heading}'s heading and \
                     entry {first} leaves row {row} at its {edge}, with the entry whole and \
                     the heading {}: row {row} is drawn {off:?} off it, entry {first} at {:?}",
                    if holds { "whole" } else { "cut" },
                    shown.drawn(cx, first)
                );
                close(cx);
            }
        }
    }

    /// A shorter list brings the selected entry its foot cuts back from
    /// below, scrolled the least that shows it whole, its foot on the
    /// list's: at 13 and 18 pt, with the last entry a 1200 × 800 window's
    /// list holds whole at its top selected, a 700 px tall window ends the
    /// entry at the shorter list's foot, and a 300 px one, whose list is at
    /// its floor and holds the entry but not its heading too, shows it whole
    /// there; opened at its floor, the list shows its first entry so. Read
    /// 2 px shorter than it is drawn, the list ended the entry 2 px above its
    /// foot and, at the floor, cut its top: 2 px at 13 pt, 1 px at 18 pt.
    #[gpui::test]
    async fn a_shorter_list_brings_an_entry_its_foot_cuts_back_to_its_foot(
        cx: &mut TestAppContext,
    ) {
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        let (app, cx) = window(cx, &repo);
        for text in [13, 18] {
            open(&app, cx, text, (1200., 800.));
            let mut shown = Shown::read(cx, "");
            let entries = shown.entries();
            let last = entries
                .iter()
                .rposition(|&entry| shown.whole(cx, entry))
                .unwrap();
            for _ in 0..last {
                press(cx, "down");
            }
            let selected = entries[last];
            // How far below the list's top the entry ends.
            let mut ends = shown.at(cx, selected).bottom() - shown.top;
            for height in [700., 300.] {
                cx.simulate_resize(size(px(1200.), px(height)));
                draw(cx);
                shown.relist(cx);
                assert!(
                    ends > shown.view(),
                    "{text} pt: a {height} px tall window's list, {:?} inside its border, \
                     cuts entry {selected}, which ended {ends:?} below its top",
                    shown.view()
                );
                let drawn = shown.at(cx, selected);
                assert!(
                    (drawn.bottom() - shown.foot).abs() <= EDGE && shown.whole(cx, selected),
                    "{text} pt: a {height} px tall window brings entry {selected} whole to \
                     its list's foot, {:?}: {drawn:?}",
                    shown.foot
                );
                ends = drawn.bottom() - shown.top;
            }
            // The list is at its floor, which holds an entry but not a
            // heading with it.
            let floor = appearance::ui_size(SHORTCUT_LIST_FLOOR) - px(2.);
            assert!(
                (shown.view() - floor).abs() <= EDGE
                    && shown.entry <= shown.view()
                    && shown.view() < shown.heading + shown.entry,
                "{text} pt: a 300 px tall window gives the list its floor, {floor:?}, \
                 an entry {:?} tall but not a heading too: {:?}",
                shown.entry,
                shown.view()
            );
            close(cx);
            open(&app, cx, text, (1200., 300.));
            shown.relist(cx);
            let first = entries[0];
            let drawn = shown.at(cx, first);
            assert!(
                (drawn.bottom() - shown.foot).abs() <= EDGE && shown.whole(cx, first),
                "{text} pt: the list opened at its floor shows entry {first} whole at its \
                 foot, {:?}: {drawn:?}",
                shown.foot
            );
            close(cx);
        }
    }

    /// A click selects the entry it lands on and brings it into view,
    /// scrolled the least that shows it whole, a group's first entry with its
    /// group's heading: at 13 and 18 pt, with the wheel leaving the second
    /// group's heading cut 10 px by the list's top, a click on the group's
    /// first entry brings the heading back to the top, and a click on an
    /// entry the list's foot cuts by 10 px brings the entry's foot to the
    /// list's. A click scrolled nothing, leaving the heading cut.
    #[gpui::test]
    async fn a_click_brings_its_entry_into_view_with_its_groups_heading(cx: &mut TestAppContext) {
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        let (app, cx) = window(cx, &repo);
        for text in [13, 18] {
            open(&app, cx, text, (1200., 800.));
            let shown = Shown::read(cx, "");
            let entries = shown.entries();
            let heading = shown.headings()[1];
            let (first, second) = (heading + 1, heading + 2);
            for _ in 0..entries.iter().position(|&entry| entry == second).unwrap() {
                press(cx, "down");
            }
            // The heading stands an entry and its own height above the second
            // entry; the wheel leaves it 10 px above the list's top.
            let top = shown.at(cx, second).top() - shown.entry - shown.heading;
            shown.wheel(cx, top - (shown.top - px(10.)), second);
            assert!(
                !shown.whole(cx, heading) && shown.whole(cx, first),
                "{text} pt: the wheel cuts group {heading}'s heading above entry {first}"
            );
            let click = shown.at(cx, first).center();
            cx.simulate_click(click, Modifiers::default());
            draw(cx);
            let drawn = shown.drawn(cx, heading).map(|row| row.top());
            assert!(
                drawn.is_some_and(|top| (top - shown.top).abs() <= EDGE) && shown.whole(cx, first),
                "{text} pt: a click on entry {first} brings group {heading}'s heading \
                 back to the list's top: the heading is drawn at {drawn:?}, the list from \
                 {:?}",
                shown.top
            );
            // The last entry the list holds whole, which the wheel leaves cut
            // 10 px by the list's foot.
            let cut = *entries
                .iter()
                .rev()
                .find(|&&entry| shown.whole(cx, entry))
                .unwrap();
            let by = shown.at(cx, cut).bottom() - (shown.foot + px(10.));
            shown.wheel(cx, by, cut);
            let click = shown.at(cx, cut).center();
            cx.simulate_click(click, Modifiers::default());
            draw(cx);
            let drawn = shown.at(cx, cut);
            assert!(
                (drawn.bottom() - shown.foot).abs() <= EDGE && shown.shown(cx, cut),
                "{text} pt: a click on entry {cut}, cut 10 px by the list's foot, brings \
                 its foot to the list's, {:?}: {drawn:?}",
                shown.foot
            );
            close(cx);
        }
    }

    /// Down to the list's last entry leaves it whole at the list's foot: the
    /// centring Down asks for there lies past the list's end, which holds it,
    /// and Down again there moves nothing. At 13 and 18 pt in a 1200 × 800
    /// window.
    #[gpui::test]
    async fn down_to_the_last_entry_leaves_it_whole_at_the_lists_foot(cx: &mut TestAppContext) {
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        let (app, cx) = window(cx, &repo);
        for text in [13, 18] {
            open(&app, cx, text, (1200., 800.));
            let shown = Shown::read(cx, "");
            let entries = shown.entries();
            let last = entries[entries.len() - 1];
            for presses in [entries.len() - 1, 1] {
                for _ in 0..presses {
                    press(cx, "down");
                }
                let drawn = shown.at(cx, last);
                assert!(
                    (drawn.bottom() - shown.foot).abs() <= EDGE && shown.whole(cx, last),
                    "{text} pt: {presses} Down presses leave the last entry, {last}, whole \
                     at the list's foot, {:?}: {drawn:?}",
                    shown.foot
                );
            }
            close(cx);
        }
    }

    /// Down past the list's foot onto a group's first entry brings the
    /// group's heading with it, the two centred together: at 13 and 18 pt in
    /// a 1200 × 800 window, with the last entry of the group above selected
    /// and the wheel leaving it at the list's foot, Down shows the next
    /// group's heading and first entry whole, centred together. Centred
    /// alone, the entry stood half a heading's height below the list's
    /// centre.
    #[gpui::test]
    async fn down_past_the_foot_onto_a_groups_first_entry_shows_its_heading(
        cx: &mut TestAppContext,
    ) {
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        let (app, cx) = window(cx, &repo);
        for text in [13, 18] {
            open(&app, cx, text, (1200., 800.));
            let shown = Shown::read(cx, "");
            let entries = shown.entries();
            // The first heading at or below the list's foot as it opens, so
            // the wheel can bring the entry above it to the foot.
            let heading = shown
                .headings()
                .into_iter()
                .find(|&heading| {
                    shown
                        .drawn(cx, heading)
                        .is_none_or(|row| row.top() >= shown.foot - EDGE)
                })
                .expect("a heading below the list's first screen");
            let (above, first) = (heading - 1, heading + 1);
            for _ in 0..entries.iter().position(|&entry| entry == above).unwrap() {
                press(cx, "down");
            }
            let by = shown.at(cx, above).bottom() - shown.foot;
            shown.wheel(cx, by, above);
            assert!(
                shown.whole(cx, above) && !shown.whole(cx, heading) && !shown.whole(cx, first),
                "{text} pt: the wheel leaves entry {above} at the list's foot, group \
                 {heading} below it"
            );
            assert!(
                shown.heading(first) == Some(heading),
                "{text} pt: the list holds a heading and an entry"
            );
            press(cx, "down");
            let what =
                format!("{text} pt: Down past the list's foot onto group {heading}'s first entry");
            shown.centred(cx, first, &what);
            close(cx);
        }
    }

    /// End selects the list's last entry and leaves it whole at the list's
    /// foot, and Home selects the first and brings the first group's heading
    /// to the list's top with it, from the search field, which keeps focus:
    /// at 13 and 18 pt in a 1200 × 800 window, and again with the wheel
    /// having moved the list away from an entry already selected. The field
    /// took both keys to move its caret, and the selection stayed put.
    #[gpui::test]
    async fn home_and_end_reach_the_first_heading_and_the_last_entry_whole(
        cx: &mut TestAppContext,
    ) {
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        let (app, cx) = window(cx, &repo);
        for text in [13, 18] {
            open(&app, cx, text, (1200., 800.));
            let shown = Shown::read(cx, "");
            let entries = shown.entries();
            let (first, last) = (entries[0], entries[entries.len() - 1]);
            let heading = shown.headings()[0];
            assert_eq!(
                shown.selected(cx),
                Some(first),
                "{text} pt: the list opens on its first entry"
            );
            // End, then End again after the wheel moved the list two
            // entries up from its foot.
            for again in [false, true] {
                if again {
                    shown.wheel(cx, -shown.entry * 2., entries[entries.len() - 4]);
                    assert!(
                        !shown.whole(cx, last),
                        "{text} pt: the wheel moves entry {last} below the list's foot"
                    );
                }
                press(cx, "end");
                let drawn = shown.drawn(cx, last);
                assert!(
                    shown.selected(cx) == Some(last)
                        && drawn.is_some_and(|row| (row.bottom() - shown.foot).abs() <= EDGE)
                        && shown.whole(cx, last),
                    "{text} pt: End selects the last entry, {last}, whole at the list's foot, \
                     {:?}: it is drawn at {drawn:?}, the selection on {:?}",
                    shown.foot,
                    shown.selected(cx)
                );
            }
            for again in [false, true] {
                if again {
                    shown.wheel(cx, shown.entry * 2., entries[3]);
                    assert!(
                        !shown.whole(cx, heading) && !shown.whole(cx, first),
                        "{text} pt: the wheel moves group {heading} above the list's top"
                    );
                }
                press(cx, "home");
                let drawn = shown.drawn(cx, heading).map(|row| row.top());
                assert!(
                    shown.selected(cx) == Some(first)
                        && drawn.is_some_and(|top| (top - shown.top).abs() <= EDGE)
                        && shown.whole(cx, heading)
                        && shown.whole(cx, first),
                    "{text} pt: Home selects the first entry, {first}, with group {heading}'s \
                     heading whole at the list's top, {:?}: the heading is drawn at {drawn:?}, \
                     the selection on {:?}",
                    shown.top,
                    shown.selected(cx)
                );
            }
            assert_eq!(
                cx.update(|window, cx| app.read(cx).dialog_hints.focused(window)),
                Some(DialogKeys::Shortcuts),
                "{text} pt: the search field keeps focus"
            );
            close(cx);
        }
    }

    /// Page Down and Page Up move the selection by as many entries as the
    /// list's height holds whole rows, passing over headings, as a macOS
    /// list does, and bring the entry they reach into view by Up's and
    /// Down's rules: left where it lies whole, otherwise centred with its
    /// group's heading where it is the group's first, the list's ends
    /// holding it. At 13 and 18 pt in a 1200 × 800 window, Page Down from the
    /// first entry to the last, which it leaves whole at the list's foot and
    /// keeps there, and Page Up back to the first, with the first heading at
    /// the list's top. Neither key moved the selection.
    #[gpui::test]
    async fn page_down_and_page_up_move_by_the_whole_rows_the_list_shows(cx: &mut TestAppContext) {
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        let (app, cx) = window(cx, &repo);
        for text in [13, 18] {
            open(&app, cx, text, (1200., 800.));
            let shown = Shown::read(cx, "");
            let entries = shown.entries();
            let rows = shown.view() / shown.entry;
            let page = rows.floor() as usize;
            assert!(
                page >= 2 && (0.02..0.98).contains(&rows.fract()),
                "{text} pt: the list holds {rows} entry rows, a page of {page} clear of the \
                 rounding of a row's drawn height"
            );
            let mut at = 0;
            for (key, back) in [("pagedown", false), ("pageup", true)] {
                loop {
                    let to = if back {
                        at - page.min(at)
                    } else {
                        (at + page).min(entries.len() - 1)
                    };
                    let entry = entries[to];
                    let inside = shown.shown(cx, entry);
                    let before = shown.drawn(cx, entry).map(|row| row.top());
                    press(cx, key);
                    let what = format!("{text} pt: {key} from entry {} to {entry}", entries[at]);
                    assert_eq!(shown.selected(cx), Some(entry), "{what} selects it");
                    if inside {
                        let now = shown.at(cx, entry).top();
                        assert!(
                            before.is_some_and(|before| (now - before).abs() <= EDGE),
                            "{what}, whole in the list, moved it from {before:?} to {now:?}"
                        );
                    } else {
                        shown.centred(cx, entry, &what);
                    }
                    if to == at {
                        break;
                    }
                    at = to;
                }
                let (end, drawn) = if back {
                    let heading = shown.headings()[0];
                    (heading, shown.at(cx, heading).top() - shown.top)
                } else {
                    let last = entries[entries.len() - 1];
                    (last, shown.at(cx, last).bottom() - shown.foot)
                };
                assert!(
                    drawn.abs() <= EDGE && shown.whole(cx, end),
                    "{text} pt: {key} to the list's end leaves row {end} whole at it, \
                     {drawn:?} off"
                );
            }
            close(cx);
        }
    }
}
