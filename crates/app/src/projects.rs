use crate::native_accessibility::LiveRegion as _;
use crate::{
    GitTurtle,
    focus_reveal::{FocusReveal, Trigger},
    preferences::{Preferences, directory_name, validate_project_name},
};
use gpui_kit::component::{
    Disableable, Icon, Selectable, Sizable, Theme, WindowExt,
    button::{Button, ButtonVariants},
    dialog::{Cancel, Confirm, DialogFooter},
    input::{Input, InputEvent, InputState},
    tooltip::Tooltip,
};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    rc::Rc,
};

/// Where a line was drawn, and how far its scrolling body stood then.
type DrawnLine = (Bounds<Pixels>, Pixels);

/// User intent only. The application executes repository work in the background.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProjectEvent {
    Open(PathBuf),
    /// Ask to choose a folder to open: the application decides whether the
    /// folder picker opens, since the hub cannot see the tab limit.
    ChooseFolder,
    /// Ask to give this project a client-only name. The hub does not save it;
    /// the application owns the preference writer.
    Rename(PathBuf),
    Clone {
        source: String,
        destination: PathBuf,
    },
    Create {
        destination: PathBuf,
        branch: String,
    },
    Back,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ProjectMode {
    Open,
    Clone,
    Create,
}

pub struct ProjectHub {
    recent: Vec<PathBuf>,
    names: HashMap<PathBuf, String>,
    filtered: Vec<PathBuf>,
    /// Each recent row's text, by its project ([`Self::row_text`]).
    row_text: RefCell<HashMap<PathBuf, Rc<RowText>>>,
    /// The recent list's count and its accessible label, with what they
    /// were built from ([`Self::count_text`]).
    count_text: RefCell<Option<CountText>>,
    search: Entity<InputState>,
    source: Entity<InputState>,
    parent: Entity<InputState>,
    name: Entity<InputState>,
    branch: Entity<InputState>,
    branch_edited: bool,
    pending_default_branch: Option<String>,
    // Displaying a path must not lose filename bytes selected by the native picker.
    picked_parent: Option<(String, PathBuf)>,
    suggested_name: Option<String>,
    mode: ProjectMode,
    pending_mode: Option<ProjectMode>,
    busy: bool,
    picker_pending: bool,
    /// Whether the Enter press last going down in the recent search may
    /// open its first project, noted by its key-down
    /// ([`Self::search_key_down`]) for the `PressEnter` it brings, which
    /// tells neither.
    search_enter_opens: bool,
    can_go_back: bool,
    error: Option<String>,
    /// Errors the feedback line has been given, which gives each its own
    /// node, so the same refusal asked for again is announced again, as
    /// `native_accessibility::refusal_line` does.
    errors_shown: usize,
    /// The feedback line last brought into the body's view, by
    /// `errors_shown`: each new one is revealed once, as it appears.
    error_revealed: Rc<Cell<usize>>,
    /// The feedback line of an Open that failed as the page opened on its
    /// first stop, by `errors_shown` ([`Self::opened_on_first_stop`]).
    first_stop_line: Option<usize>,
    /// The feedback line, by `errors_shown`, that stayed out of view so the
    /// recent row that asked keeps its place: heard politely, not as an
    /// alert ([`place_feedback`]), and that row's project.
    error_kept: Rc<RefCell<Option<(usize, PathBuf)>>>,
    /// Where the feedback line was last drawn, and the body's offset then.
    error_line: Rc<Cell<Option<DrawnLine>>>,
    /// Where the recent rows lie, and the one a press is awaiting an answer
    /// for.
    recent_rows: Rc<RefCell<RecentRows>>,
    recent_scroll: UniformListScrollHandle,
    /// The page body's scroll, which reveals the form control focus moves
    /// onto.
    body: FocusReveal,
    action_tabs_focus: FocusHandle,
    /// The Open, Clone and Create tabs' own focus, so the page can open
    /// with focus on the selected one ([`Self::focus_selected_mode`]).
    mode_focus: [FocusHandle; 3],
    parent_field_focus: FocusHandle,
    submit_focus: FocusHandle,
    subscriptions: Vec<Subscription>,
}

impl EventEmitter<ProjectEvent> for ProjectHub {}

impl ProjectHub {
    pub fn new(
        recent: Vec<PathBuf>,
        names: HashMap<PathBuf, String>,
        default_branch: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx
            .new(|cx| InputState::new(window, cx).placeholder("Find a project by name or folder"));
        let source =
            cx.new(|cx| InputState::new(window, cx).placeholder("https://host/owner/project.git"));
        let parent = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Choose where your project will live")
        });
        let name = cx.new(|cx| InputState::new(window, cx).placeholder("my-project"));
        let branch = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("main")
                .default_value(default_branch)
        });
        let recent = unique_recent(recent);
        let mut this = Self {
            filtered: recent.clone(),
            recent,
            names,
            row_text: RefCell::default(),
            count_text: RefCell::default(),
            search: search.clone(),
            source: source.clone(),
            parent: parent.clone(),
            name: name.clone(),
            branch: branch.clone(),
            branch_edited: false,
            pending_default_branch: None,
            picked_parent: None,
            suggested_name: None,
            mode: ProjectMode::Open,
            pending_mode: None,
            busy: false,
            picker_pending: false,
            search_enter_opens: false,
            can_go_back: false,
            error: None,
            errors_shown: 0,
            error_revealed: Rc::default(),
            first_stop_line: None,
            error_kept: Rc::default(),
            error_line: Rc::default(),
            recent_rows: Rc::default(),
            recent_scroll: UniformListScrollHandle::new(),
            body: FocusReveal::new(Trigger::AnyFocus),
            action_tabs_focus: cx.focus_handle(),
            mode_focus: [cx.focus_handle(), cx.focus_handle(), cx.focus_handle()],
            parent_field_focus: cx.focus_handle(),
            submit_focus: cx.focus_handle(),
            subscriptions: Vec::new(),
        };
        this.subscriptions.push(cx.subscribe_in(
            &search,
            window,
            |this, _, event, _, cx| match event {
                InputEvent::Change => this.filter_recent(cx),
                // The key-down notes whether this Enter may open
                // (`search_key_down`), which every Enter the field reports
                // takes: Shift and the secondary modifier make another, which
                // opens nothing. Taken by the plain one alone, a modified
                // press left it set for one the field reported with no
                // key-down.
                InputEvent::PressEnter { secondary, shift } => {
                    if std::mem::take(&mut this.search_enter_opens)
                        && !secondary
                        && !shift
                        && let Some(path) = this.filtered.first().cloned()
                    {
                        this.emit_operation(ProjectEvent::Open(path), cx);
                    }
                }
                _ => {}
            },
        ));
        this.subscriptions.push(
            cx.subscribe_in(&source, window, |this, _, event, window, cx| {
                if let InputEvent::Change = event {
                    this.error = None;
                    this.suggest_project_name(window, cx);
                    cx.notify();
                }
            }),
        );
        for input in [&parent, &name] {
            this.subscriptions
                .push(cx.subscribe_in(input, window, |this, _, event, _, cx| {
                    if let InputEvent::Change = event {
                        this.error = None;
                        cx.notify();
                    }
                }));
        }
        this.subscriptions
            .push(cx.subscribe_in(&branch, window, |this, _, event, _, cx| {
                if let InputEvent::Change = event {
                    this.branch_edited = true;
                    this.pending_default_branch = None;
                    this.error = None;
                    cx.notify();
                }
            }));
        this
    }

    pub fn set_recent(&mut self, recent: Vec<PathBuf>, cx: &mut Context<Self>) {
        self.recent = unique_recent(recent);
        self.filter_recent(cx);
    }

    pub fn set_names(&mut self, names: HashMap<PathBuf, String>, cx: &mut Context<Self>) {
        self.names = names;
        self.filter_recent(cx);
    }

    /// The text the recent row `index` draws, kept for its project and
    /// built again only when the project's own name changes ([`RowText`]):
    /// each row formatted its location and both controls' names, and cloned
    /// its path twice, on every frame that drew it. Entries for projects no
    /// longer recent go as the list grows past them, so what is kept is
    /// bounded by the recent list.
    fn row_text(&self, index: usize) -> Rc<RowText> {
        #[cfg(test)]
        let before = crate::test_support::allocations();
        let path = &self.filtered[index];
        let custom = self.names.get(path).map(String::as_str);
        let mut kept = self.row_text.borrow_mut();
        let text = match kept.get(path.as_path()) {
            Some(text) if text.custom.as_deref() == custom => text.clone(),
            _ => {
                let text = Rc::new(RowText::build(path, custom));
                kept.insert(path.clone(), text.clone());
                if kept.len() > self.recent.len() {
                    kept.retain(|path, _| self.recent.contains(path));
                }
                text
            }
        };
        #[cfg(test)]
        RECENT_TEXT.with(|counted| {
            let (lookups, allocations) = counted.get();
            counted.set((
                lookups + 1,
                allocations + crate::test_support::allocations() - before,
            ));
        });
        text
    }

    /// The recent list's count and its accessible label, as `searching`
    /// shows them, built again only when a count changes ([`CountText`]):
    /// formatted on every frame.
    fn count_text(&self, searching: bool) -> (SharedString, SharedString) {
        #[cfg(test)]
        let before = crate::test_support::allocations();
        let counts = (searching, self.filtered.len(), self.recent.len());
        let mut kept = self.count_text.borrow_mut();
        if kept.as_ref().is_some_and(|text| text.counts != counts) {
            *kept = None;
        }
        let text = kept.get_or_insert_with(|| CountText::build(counts));
        let text = (text.count.clone(), text.label.clone());
        #[cfg(test)]
        RECENT_TEXT.with(|counted| {
            let (lookups, allocations) = counted.get();
            counted.set((
                lookups + 1,
                allocations + crate::test_support::allocations() - before,
            ));
        });
        text
    }

    fn display_name(&self, path: &Path) -> String {
        self.names
            .get(path)
            .cloned()
            .unwrap_or_else(|| directory_name(path))
    }

    pub fn set_default_branch(&mut self, branch: String, cx: &mut Context<Self>) {
        if !self.branch_edited {
            // InputState needs a Window; apply this during the next render.
            self.pending_default_branch = Some(branch);
            cx.notify();
        }
    }

    /// The hub sets this before emitting an operation; the owner clears it when finished.
    pub fn set_busy(&mut self, busy: bool, cx: &mut Context<Self>) {
        if !busy {
            // The wait is over, however it ended: a later refusal is not the
            // answer to the row pressed for it. Only `refuse_open` answers
            // that press, and it keeps it (`set_waiting`).
            self.recent_rows.borrow_mut().pressed = None;
        }
        self.set_waiting(busy, cx);
    }

    fn set_waiting(&mut self, busy: bool, cx: &mut Context<Self>) {
        self.busy = busy;
        if !busy {
            self.pending_mode = None;
        }
        cx.notify();
    }

    pub fn set_error(&mut self, error: Option<String>, cx: &mut Context<Self>) {
        if error.is_some() {
            self.errors_shown = self.errors_shown.wrapping_add(1);
        }
        self.error = error;
        cx.notify();
    }

    /// Clears the feedback line where it still says `error`, the opening
    /// error of a repository a quiet read has since found readable.
    pub fn clear_error_matching(&mut self, error: &str, cx: &mut Context<Self>) {
        if self.error.as_deref() == Some(error) {
            self.error = None;
            cx.notify();
        }
    }

    /// What recent row `index` reads in place of its location: the refusal
    /// of its press, while the feedback line that says it is out of view.
    /// The press is kept by its project, so a search or a new recent list
    /// leaves the refusal on that project's row, wherever it now stands, and
    /// on no row while the project is not listed.
    fn refusal_on_row(&self, index: usize) -> Option<SharedString> {
        let kept = self.error_kept.borrow();
        let (line, project) = kept.as_ref()?;
        if *line != self.errors_shown || *project != self.filtered[index] {
            return None;
        }
        let error = self.error.as_ref()?;
        (!feedback_in_view(self.error_line.get()?, self.body.scroll()))
            .then(|| SharedString::from(error.clone()))
    }

    /// Replaces the feedback line where it still says `previous`, the
    /// opening error a tab's mark has since restated
    /// (`GitTurtle::tab_mark_restated`), as a line of its own. The same
    /// words are no new line: Refresh pressed again with nothing changed
    /// gives it no fresh node, alert or reveal, and a recent row's refusal
    /// kept by that line still matches.
    pub fn replace_error_matching(
        &mut self,
        previous: &str,
        error: String,
        cx: &mut Context<Self>,
    ) {
        if previous != error && self.error.as_deref() == Some(previous) {
            self.set_error(Some(error), cx);
        }
    }

    /// Ends an Open the hub asked for that the window refused before reading
    /// anything, as at the eight-tab limit, saying why in the hub's
    /// feedback line, announced at once as its failures are, and returns
    /// whether the hub had asked for one. Left busy, the hub kept every
    /// action refused under "Opening project…", and only the window's
    /// error, which the hub does not show, said why.
    pub fn refuse_open(&mut self, reason: &str, cx: &mut Context<Self>) -> bool {
        if !(self.busy && self.pending_mode == Some(ProjectMode::Open)) {
            return false;
        }
        self.set_waiting(false, cx);
        self.set_error(Some(reason.to_owned()), cx);
        true
    }

    /// The body's scroll, where a test places what the person scrolled to.
    #[cfg(test)]
    pub(crate) fn body_scroll(&self) -> ScrollHandle {
        self.body.scroll().clone()
    }

    /// Test-only: types `query` into the recent list's search field.
    #[cfg(test)]
    pub(crate) fn search_recent(
        &mut self,
        query: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.search.update(cx, |input, cx| {
            input.set_value(query.to_owned(), window, cx)
        });
        self.filter_recent(cx);
    }

    /// Test-only: the hub's feedback line, as other modules' tests read it.
    #[cfg(test)]
    pub(crate) fn shown_error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Test-only: waits for a Clone, as one the hub asked for does.
    #[cfg(test)]
    pub(crate) fn wait_for_clone(&mut self, cx: &mut Context<Self>) {
        self.pending_mode = Some(ProjectMode::Clone);
        self.set_waiting(true, cx);
    }

    /// Test-only: whether the hub waits on an operation it asked for.
    #[cfg(test)]
    pub(crate) fn shown_busy(&self) -> bool {
        self.busy
    }

    pub fn set_can_go_back(&mut self, can_go_back: bool, cx: &mut Context<Self>) {
        self.can_go_back = can_go_back;
        cx.notify();
    }

    fn unavailable(&self) -> bool {
        self.busy || self.picker_pending
    }

    fn busy_label(&self) -> &'static str {
        match self.pending_mode.unwrap_or(self.mode) {
            ProjectMode::Open => "Opening project…",
            ProjectMode::Clone => "Cloning project…",
            ProjectMode::Create => "Creating project…",
        }
    }

    fn filter_recent(&mut self, cx: &mut Context<Self>) {
        let query = self.search.read(cx).value().trim().to_lowercase();
        self.filtered = self
            .recent
            .iter()
            .filter(|path| {
                path.to_string_lossy().to_lowercase().contains(&query)
                    || self.display_name(path).to_lowercase().contains(&query)
            })
            .cloned()
            .collect();
        self.recent_scroll.scroll_to_item(0, ScrollStrategy::Top);
        cx.notify();
    }

    fn clear_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.unavailable() {
            return;
        }
        self.search.update(cx, |input, cx| {
            input.set_value("", window, cx);
            input.focus(window, cx);
        });
        self.filter_recent(cx);
    }

    fn suggest_project_name(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let current = self.name.read(cx).value();
        if !current.is_empty() && self.suggested_name.as_deref() != Some(current.as_ref()) {
            return;
        }
        if let Some(name) = suggested_project_name(self.source.read(cx).value().as_ref()) {
            self.name
                .update(cx, |input, cx| input.set_value(name.clone(), window, cx));
            self.suggested_name = Some(name);
        }
    }

    /// Focuses the selected action tab, Open, Clone or Create, whose ring
    /// the next frame draws, scrolled into view with the rest of the hub's
    /// focus reveals. The page opened with focus on the window's root, which
    /// draws nothing, so nothing showed where the keyboard stood until a Tab
    /// landed on the tab strip's Menu.
    pub(crate) fn focus_selected_mode(&self, window: &mut Window, cx: &mut App) {
        window.focus(self.selected_mode_focus(), cx);
    }

    /// Marks the feedback line now shown as the failure the page opened on
    /// its first stop with, whose reveal leaves that stop whole
    /// ([`reveal_feedback`]).
    pub(crate) fn opened_on_first_stop(&mut self) {
        self.first_stop_line = Some(self.errors_shown);
    }

    /// The selected action tab's focus: the page's first stop.
    pub(crate) fn selected_mode_focus(&self) -> &FocusHandle {
        &self.mode_focus[match self.mode {
            ProjectMode::Open => 0,
            ProjectMode::Clone => 1,
            ProjectMode::Create => 2,
        }]
    }

    /// The recent search's focus, for a test.
    #[cfg(test)]
    pub(crate) fn search_focus(&self, cx: &App) -> FocusHandle {
        self.search.read(cx).focus_handle(cx)
    }

    /// The recent search, for a test to compose in.
    #[cfg(test)]
    pub(crate) fn search_field(&self) -> Entity<InputState> {
        self.search.clone()
    }

    /// Whether the selected action tab holds focus.
    #[cfg(test)]
    pub(crate) fn selected_mode_focused(&self, window: &Window) -> bool {
        self.selected_mode_focus().is_focused(window)
    }

    /// Selects `mode`'s action tab and moves to its first field. The tab
    /// already selected does nothing, as a selected tab does: pressed by
    /// Enter or Space on the page's first stop, it cleared the feedback
    /// line, a restored marked tab's reason among them, and moved focus to
    /// the recent search, where the next Enter, a held key's repeat
    /// included, opened the first recent project.
    fn change_mode(&mut self, mode: ProjectMode, window: &mut Window, cx: &mut Context<Self>) {
        if self.unavailable() || self.mode == mode {
            return;
        }
        self.mode = mode;
        self.error = None;
        match mode {
            ProjectMode::Open => self.search.update(cx, |input, cx| input.focus(window, cx)),
            ProjectMode::Clone => self.source.update(cx, |input, cx| input.focus(window, cx)),
            ProjectMode::Create => self.name.update(cx, |input, cx| input.focus(window, cx)),
        }
        cx.notify();
    }

    /// Notes whether the Enter going down in the recent search may open the
    /// first project it lists, which the field's `PressEnter` does once its
    /// key-down has run: only a press that began in the field, never a held
    /// key's repeats (marked held on macOS and Wayland), nor an Enter that
    /// began on another control and moved focus here, as under X11 its
    /// repeats arrive as fresh presses (`enter_press_began_on`), nor one
    /// that goes down while an input method composes there, which commits
    /// the composition, nor a modified one. A held Enter at launch, on the
    /// page's first stop, opened it; acted on as a raw key-down, a modified
    /// Enter opened it too, and so did an Enter committing a composition;
    /// noted for a modified press, the plain Enter the field reported next
    /// opened it with no key-down of its own.
    fn search_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use gpui::EntityInputHandler as _;
        self.search_enter_opens = false;
        let focus = self.search.read(cx).focus_handle(cx);
        if event.keystroke.key != "enter"
            || event.keystroke.modifiers.modified()
            || event.is_held
            || !focus.is_focused(window)
            || !gpui_kit::base::enter_press_began_on(&focus, window, cx)
        {
            return;
        }
        let composing = self.search.update(cx, |search, cx| {
            search.marked_text_range(window, cx).is_some()
        });
        self.search_enter_opens = !composing;
    }

    fn emit_operation(&mut self, event: ProjectEvent, cx: &mut Context<Self>) {
        if self.unavailable() {
            return;
        }
        self.recent_rows.borrow_mut().pressed = None;
        self.pending_mode = Some(match &event {
            ProjectEvent::Open(_) => ProjectMode::Open,
            ProjectEvent::Clone { .. } => ProjectMode::Clone,
            ProjectEvent::Create { .. } => ProjectMode::Create,
            ProjectEvent::Back | ProjectEvent::ChooseFolder | ProjectEvent::Rename(_) => return,
        });
        self.error = None;
        self.busy = true;
        cx.emit(event);
        cx.notify();
    }

    pub(crate) fn choose_folder(
        &mut self,
        open: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.unavailable() {
            return;
        }
        self.recent_rows.borrow_mut().pressed = None;
        self.picker_pending = true;
        self.error = None;
        cx.notify();
        let response = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(
                if open {
                    "Open project"
                } else {
                    "Choose parent folder"
                }
                .into(),
            ),
        });
        cx.spawn_in(window, async move |this, cx| {
            let response = crate::folder_picker::selected_path(response).await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.picker_pending = false;
                match response {
                    Ok(Some(path)) => {
                        if open {
                            this.emit_operation(ProjectEvent::Open(path), cx);
                        } else {
                            let display = path.to_string_lossy().into_owned();
                            this.parent.update(cx, |input, cx| {
                                input.set_value(display.clone(), window, cx)
                            });
                            this.picked_parent = Some((display, path));
                            this.name.update(cx, |input, cx| input.focus(window, cx));
                        }
                    }
                    Ok(None) => {}
                    Err(error) => this.set_error(Some(error), cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn destination(&self, cx: &App) -> Result<PathBuf, String> {
        let value = self.parent.read(cx).value();
        let parent = self
            .picked_parent
            .as_ref()
            .filter(|(display, _)| display == value.as_ref())
            .map(|(_, path)| path.clone())
            .unwrap_or_else(|| PathBuf::from(value.as_ref()));
        validate_destination(&parent, self.name.read(cx).value().as_ref())
    }

    /// Return in a Clone or Create field submits the form once per press, on
    /// the conditions a dialog's default action keeps (the toolkit's
    /// `dialog.rs`) for a plain Enter, plus Shift-Enter, which the dialog
    /// refuses: going down, not marked
    /// held, not repeating the press the toolkit recorded
    /// (`enter_press_is_repeat`), in the field focused as that press began
    /// (`enter_press_began_on`). The form's key-down listener asks, after
    /// the field's own Enter has let the key go on. A held key's Enter
    /// key-downs repeat on every platform, and under X11 each arrives as a
    /// fresh press, so a held Return after a clone that failed at once began
    /// another clone, a network action, on each repeat. The fields submitted
    /// on their own `PressEnter`, which carries no held mark, so a press the
    /// window never saw (a Return that confirmed the native folder picker,
    /// whose answer focuses the folder name, or one an input method took to
    /// commit a composition) made its first held repeat look like a fresh
    /// press, and it began one clone or create (decided 2026-11-10). The
    /// secondary Enter (Command-Enter on macOS, Control-Enter elsewhere)
    /// submits nothing: on macOS it records no press, since Cocoa sends no
    /// key-up for a key pressed with Command held, so its repeats could not
    /// be told from presses. Neither does an Alt- or Fn-Enter.
    fn submit_on_enter(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let modifiers = &event.keystroke.modifiers;
        if event.keystroke.key != "enter"
            || event.is_held
            || modifiers.control
            || modifiers.alt
            || modifiers.platform
            || modifiers.function
        {
            return;
        }
        let Some(field) = [&self.source, &self.parent, &self.name, &self.branch]
            .into_iter()
            .map(|field| field.read(cx).focus_handle(cx))
            .find(|field| field.is_focused(window))
        else {
            return;
        };
        if gpui_kit::base::enter_press_is_repeat(window, cx)
            || !gpui_kit::base::enter_press_began_on(&field, window, cx)
        {
            return;
        }
        self.submit(cx);
    }

    fn submit(&mut self, cx: &mut Context<Self>) {
        if self.unavailable() || self.mode == ProjectMode::Open {
            return;
        }
        let operation = (|| {
            let destination = self.destination(cx)?;
            match self.mode {
                ProjectMode::Clone => {
                    let source = self.source.read(cx).value().trim().to_string();
                    validate_source(&source)?;
                    Ok(ProjectEvent::Clone {
                        source,
                        destination,
                    })
                }
                ProjectMode::Create => {
                    let branch = self.branch.read(cx).value().trim().to_string();
                    validate_branch(&branch)?;
                    Ok(ProjectEvent::Create {
                        destination,
                        branch,
                    })
                }
                ProjectMode::Open => unreachable!(),
            }
        })();
        match operation {
            Ok(event) => self.emit_operation(event, cx),
            Err(error) => self.set_error(Some(error), cx),
        }
    }

    fn recent_row(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let colors = Theme::global(cx).colors;
        let palette = crate::appearance::palette(cx);
        let text = self.row_text(index);
        let rename = text.clone();
        // While its refusal line is out of view, the refused row says why
        // where the person acted (DESIGN.md, refusals in place), in its own
        // bounds, the whole text and the location it replaces in its tooltip
        // and the refusal in its name.
        let refused = self.refusal_on_row(index);
        let (location, row_tooltip, row_name) = match &refused {
            Some(refusal) => (
                refusal.clone(),
                SharedString::from(format!("{refusal}\n{}", text.display)),
                SharedString::from(format!("{}. {refusal}", text.open)),
            ),
            None => (
                text.location.clone(),
                text.display.clone(),
                text.open.clone(),
            ),
        };
        #[cfg(test)]
        LOCATION_DRAWN.with_borrow_mut(|drawn| {
            drawn.insert(
                index,
                (
                    location.to_string(),
                    row_tooltip.to_string(),
                    row_name.to_string(),
                ),
            );
        });
        let row = Button::new(("recent-project", index))
            .debug_selector(move || format!("recent-project-{index}"))
            .ghost()
            .h(crate::appearance::ui_size(76.))
            .w_full()
            .px_4()
            .rounded(px(12.))
            .disabled(self.unavailable())
            .accessibility_label(row_name)
            .tooltip(row_tooltip)
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .size(px(42.))
                            .flex_shrink_0()
                            .rounded(px(12.))
                            .bg(rgb(palette.selected))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(hub_icon("folder", 19., colors.primary)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .text_left()
                            .child(
                                div()
                                    .text_size(crate::appearance::ui_text(14.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .truncate()
                                    .child(text.name.clone()),
                            )
                            .child(
                                div()
                                    .text_size(crate::appearance::ui_text(11.))
                                    .map(|line| match &refused {
                                        // Its hue, moved only as far as the row's
                                        // own fills need: the removed token read
                                        // 4.38:1 on Nord's hover fill.
                                        Some(_) => line.text_color(rgb(palette.small_label(
                                            palette.removed,
                                            &palette.control_fills(false, palette.canvas),
                                        ))),
                                        None => line.text_color(colors.muted_foreground),
                                    })
                                    .truncate()
                                    .child(location)
                                    .children(crate::appearance::text_probe(move || {
                                        format!("recent-project-location-{index}")
                                    })),
                            ),
                    )
                    .child(hub_icon("chevron", 14., colors.muted_foreground)),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.emit_operation(ProjectEvent::Open(text.path.clone()), cx);
                if this.busy {
                    this.recent_rows
                        .borrow_mut()
                        .press(index, text.path.clone());
                }
            }));
        let rows = self.recent_rows.clone();
        let line = div()
            .id(("recent-project-row", index))
            .w_full()
            .flex()
            .items_center()
            .gap_1()
            .child(div().flex_1().min_w_0().child(row))
            .child(
                Button::new(("rename-recent-project", index))
                    .debug_selector(move || format!("rename-recent-project-{index}"))
                    .ghost()
                    .flex_shrink_0()
                    .size(crate::appearance::ui_size(30.))
                    .rounded(px(8.))
                    .icon(Icon::default().path("icons/file-renamed.svg").size(px(15.)))
                    .accessibility_label(rename.rename.clone())
                    .tooltip("Rename this project in GitTurtle")
                    .disabled(self.unavailable())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !this.unavailable() {
                            cx.emit(ProjectEvent::Rename(rename.path.clone()));
                        }
                    })),
            );
        gpui_kit::base::ElementExt::on_prepaint(line, move |bounds, _, _| {
            rows.borrow_mut().drawn(index, bounds)
        })
        .into_any_element()
    }

    fn render_recent(&self, compact: bool, cx: &mut Context<Self>) -> AnyElement {
        let colors = Theme::global(cx).colors;
        let query = self.search.read(cx).value();
        let searching = !query.trim().is_empty();
        let count = self.count_text(searching);
        // The list clips its rows to its bounds, so it keeps the focus ring's
        // room inside them around its rows, and the panel's 0.5 rem padding
        // gives the room back: a focused row and its Rename draw their rings
        // whole and nothing moves.
        let room = crate::appearance::button_ring_room(cx);
        let padding = crate::appearance::ui_size(6.5);
        div()
            .debug_selector(|| "project-hub-recent".into())
            .on_key_down(cx.listener(Self::search_key_down))
            .flex_1()
            .min_w_0()
            .h_full()
            .when(compact, |panel| panel.flex_none().w_full().h(px(360.)))
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div().flex().items_center().gap_2()
                            .child(hub_icon("clock", 16., colors.muted_foreground))
                            .child(hub_text("recent-projects-heading", "Recently opened").role(Role::Heading).text_size(crate::appearance::ui_text(15.)).font_weight(FontWeight::SEMIBOLD)),
                    )
                    .child(
                        hub_text("recent-projects-count", count.0)
                            .role(Role::Status)
                            .aria_label(count.1)
                            .polite()
                            .px_2().py_0p5().rounded(px(6.))
                            .bg(colors.secondary).text_size(crate::appearance::ui_text(11.))
                            .text_color(colors.muted_foreground),
                    ),
            )
            .child(Input::new(&self.search).aria_label("Find a recent project by name or folder").disabled(self.unavailable()).prefix(Icon::default().path("icons/search.svg").size(px(15.)))
                .when(!query.is_empty(), |input| input.suffix(
                    Button::new("hub-clear-search-input")
                        .icon(Icon::default().path("icons/close.svg"))
                        .text().xsmall()
                        .accessibility_label("Clear project search")
                        .tooltip("Clear project search")
                        .disabled(self.unavailable())
                        .on_click(cx.listener(|this, _, window, cx| this.clear_search(window, cx))),
                )))
            .child(
                div()
                    .debug_selector(|| "project-hub-recent-list".into())
                    .flex_1()
                    .min_h_0()
                    .rounded(px(16.))
                    .border_1()
                    .border_color(colors.border)
                    .bg(colors.background)
                    .p(padding)
                    .when(self.filtered.is_empty(), |panel| {
                        panel.child(
                            div()
                                .size_full()
                                .p_6()
                                .flex()
                                .flex_col()
                                .justify_center()
                                .items_center()
                                .gap_3()
                                .text_center()
                                .child(hub_icon("folder", 28., colors.muted_foreground))
                                .child(hub_text("recent-projects-empty-heading", if self.recent.is_empty() { "Your next project starts here" } else { "No matching projects" }).role(Role::Heading).text_size(crate::appearance::ui_text(15.)).font_weight(FontWeight::MEDIUM))
                                .child(hub_text("recent-projects-empty-description", if self.recent.is_empty() { "Open, clone, or create a repository. It will be waiting here next time." } else { "Try another project name or folder." }).max_w(px(240.)).text_size(crate::appearance::ui_text(12.)).line_height(relative(1.5)).text_color(colors.muted_foreground))
                                .when(searching, |empty| empty.child(Button::new("hub-clear-search").label("Clear search").disabled(self.unavailable()).on_click(cx.listener(|this, _, window, cx| this.clear_search(window, cx))))),
                        )
                    })
                    .when(!self.filtered.is_empty(), |panel| {
                        panel.p(padding - room).child(
                            uniform_list("recent-projects", self.filtered.len(), cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                                range.map(|index| this.recent_row(index, cx)).collect::<Vec<_>>()
                            }))
                            .size_full()
                            .p(room)
                            .track_scroll(&self.recent_scroll),
                        )
                    }),
            )
            .into_any_element()
    }

    /// Reveals the control `focus` contains, [`focus_clearance`] clear of the
    /// body's edges, whenever focus moves into it.
    fn reveal_on_focus(
        &self,
        focus: FocusHandle,
        cx: &App,
    ) -> impl Fn(Vec<Bounds<Pixels>>, &mut Window, &mut App) + 'static {
        self.body.control(focus, focus_clearance(cx))
    }

    fn field(&self, label: &'static str, input: &Entity<InputState>, cx: &App) -> AnyElement {
        div()
            .on_children_prepainted(self.reveal_on_focus(input.focus_handle(cx), cx))
            .debug_selector(move || label.into())
            .flex()
            .flex_col()
            .gap_2()
            .child(
                hub_text(label, label)
                    .text_size(crate::appearance::ui_text(12.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(Theme::global(cx).colors.foreground),
            )
            .child(
                Input::new(input)
                    .aria_label(label)
                    .disabled(self.unavailable()),
            )
            .into_any_element()
    }

    fn render_action(&self, compact: bool, cx: &mut Context<Self>) -> AnyElement {
        let colors = Theme::global(cx).colors;
        let palette = crate::appearance::palette(cx);
        let (title, description, symbol, tint) = match self.mode {
            ProjectMode::Open => (
                "Open a repository",
                "Choose a project on your computer to explore its history and changes.",
                "folder",
                palette.accent,
            ),
            ProjectMode::Clone => (
                "Clone a repository",
                "Bring a repository from a remote URL or local path into a new folder.",
                "remote",
                palette.hunk,
            ),
            ProjectMode::Create => (
                "Create a repository",
                "Start a project in a new folder, ready for your first commit.",
                "branch-add",
                palette.renamed,
            ),
        };
        // The feedback line stands directly under the action that asked:
        // Choose a repository… on Open, the form's submit on Clone and Create
        // (decided 2026-11-08: at the panel's foot, the refusal lay about
        // 100 px past the view at 18 pt in a 461 × 490 window, and the press
        // looked dead).
        let feedback = self.error.as_ref().map(|error| {
            let revealed = self.error_revealed.clone();
            let shown = self.errors_shown;
            let body = self.body.scroll().clone();
            let arrival = (self.first_stop_line == Some(shown))
                .then(|| (self.body.clone(), self.selected_mode_focus().clone()));
            let (rows, kept, spoken) = (
                self.recent_rows.clone(),
                self.error_kept.clone(),
                SharedString::from(error.clone()),
            );
            // Answering a recent row, where the line stands is not known
            // until this frame's layout, so it is drawn plain for that one
            // frame and as an alert in the next, unless it stays out of view.
            let awaiting = rows.borrow().pressed.is_some() && revealed.get() != shown;
            let quiet = awaiting
                || self
                    .error_kept
                    .borrow()
                    .as_ref()
                    .is_some_and(|(line, _)| *line == shown);
            let drawn_at = self.error_line.clone();
            div()
                .on_children_prepainted(move |bounds, window, cx| {
                    let Some(line) = bounds.first().copied() else {
                        return;
                    };
                    // Recorded here, where a row refused this line reads
                    // it: the wide layout draws the rows before it, so a
                    // resize that moves it left them answering from the last
                    // frame, and no other input asked for another.
                    let drawn = (line, body.offset().y);
                    let last = drawn_at.replace(Some(drawn));
                    let held = kept
                        .borrow()
                        .as_ref()
                        .is_some_and(|(kept, _)| *kept == shown);
                    if held
                        && last.is_some_and(|last| {
                            feedback_in_view(last, &body) != feedback_in_view(drawn, &body)
                        })
                    {
                        window.defer(cx, |window, _| window.refresh());
                    }
                    if revealed.replace(shown) != shown {
                        reveal_feedback(
                            line,
                            shown,
                            body.clone(),
                            arrival.clone(),
                            rows.clone(),
                            kept.clone(),
                            spoken.clone(),
                            window,
                            cx,
                        );
                    }
                })
                .child(
                    div()
                        // A fresh node for each error, heard again however
                        // often the same one is given.
                        .id(("project-hub-error", self.errors_shown))
                        .debug_selector(|| "project-hub-error".into())
                        .when(!quiet, |line| line.alert(error.clone()))
                        .max_h(px(110.))
                        .overflow_y_scroll()
                        .flex_shrink_0()
                        .rounded(px(8.))
                        .border_1()
                        .border_color(colors.danger)
                        .bg(rgb(palette.removed_background))
                        .p_3()
                        .line_height(relative(1.5))
                        .text_size(crate::appearance::ui_text(12.))
                        .text_color(colors.danger)
                        .child(crate::folder_picker::styled_message(error, palette.muted)),
                )
        });
        let (open_feedback, form_feedback) = if self.mode == ProjectMode::Open {
            (feedback, None)
        } else {
            (None, feedback)
        };
        div()
            .debug_selector(|| "project-hub-action".into())
            .w(px(400.))
            .flex_shrink_0()
            .h_full()
            .when(compact, |panel| panel.w_full().h_auto())
            .flex()
            .flex_col()
            .rounded(px(18.))
            .border_1()
            .border_color(colors.border)
            .bg(colors.sidebar)
            .p_5()
            .gap_5()
            .child(
                div()
                    .on_children_prepainted(
                        self.reveal_on_focus(self.action_tabs_focus.clone(), cx),
                    )
                    .id("project-action-tabs")
                    .track_focus(&self.action_tabs_focus)
                    .tab_stop(false)
                    .role(Role::TabList)
                    .aria_label("Project action")
                    .flex()
                    .gap_1()
                    .bg(colors.background)
                    .rounded(px(9.))
                    .p_1()
                    .children(
                        [
                            (ProjectMode::Open, "Open", "folder", &self.mode_focus[0]),
                            (ProjectMode::Clone, "Clone", "remote", &self.mode_focus[1]),
                            (
                                ProjectMode::Create,
                                "Create",
                                "branch-add",
                                &self.mode_focus[2],
                            ),
                        ]
                        .map(|(mode, label, symbol, focus)| {
                            // The shared helper's look, as History's segments:
                            // unselected, transparent at rest with the palette's
                            // hover and pressed fills, so a press steps to almost
                            // the selected fill; selected, the `selected` surface,
                            // tinted on hover rather than faded, which would fade
                            // the focus ring with it. While the hub is busy every
                            // mode keeps the ghost's disabled look.
                            let unavailable = self.unavailable();
                            let selected = self.mode == mode && !unavailable;
                            Button::new(label)
                                .track_focus(focus)
                                .role(Role::Tab)
                                .map(|button| {
                                    if unavailable {
                                        button.ghost()
                                    } else {
                                        button.with_variant(
                                            crate::appearance::control_button_variant(selected),
                                        )
                                    }
                                })
                                .flex_1()
                                .h(crate::appearance::ui_size(32.))
                                .rounded(px(7.))
                                .label(label)
                                .icon(Icon::default().path(hub_icon_path(symbol)).size(px(14.)))
                                .selected(self.mode == mode)
                                .disabled(unavailable)
                                .debug_selector(move || format!("project-mode-{label}"))
                                .when(selected, |button| {
                                    button.hover(crate::appearance::control_selected_hover)
                                })
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.change_mode(mode, window, cx)
                                }))
                        }),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(
                                div()
                                    .size(px(42.))
                                    .flex_shrink_0()
                                    .rounded(px(12.))
                                    .bg(rgba((tint << 8) | 0x18))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(hub_icon(symbol, 21., rgb(tint).into())),
                            )
                            .child(
                                hub_text("project-action-heading", title)
                                    .role(Role::Heading)
                                    .text_size(crate::appearance::ui_text(19.))
                                    .font_weight(FontWeight::SEMIBOLD),
                            ),
                    )
                    .child(
                        hub_text("project-action-description", description)
                            .text_size(crate::appearance::ui_text(12.))
                            .line_height(relative(1.55))
                            .text_color(colors.muted_foreground),
                    ),
            )
            .when(self.mode == ProjectMode::Open, |panel| {
                panel
                    .child(
                        Button::new("hub-open-folder")
                            .debug_selector(|| "hub-open-folder".into())
                            .primary()
                            .h(crate::appearance::ui_size(40.))
                            .w_full()
                            .label(if self.busy {
                                self.busy_label()
                            } else {
                                "Choose a repository…"
                            })
                            .icon(Icon::default().path("icons/folder.svg"))
                            .disabled(self.unavailable())
                            .loading(self.busy)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.recent_rows.borrow_mut().pressed = None;
                                cx.emit(ProjectEvent::ChooseFolder);
                            })),
                    )
                    .children(open_feedback)
                    .child(
                        hub_text(
                            "hub-open-recent-hint",
                            "You can also open any project from the recent list.",
                        )
                        .text_size(crate::appearance::ui_text(11.))
                        .text_color(colors.muted_foreground),
                    )
                    .child(
                        div()
                            .border_t_1()
                            .border_color(colors.border)
                            .pt_5()
                            .flex()
                            .flex_col()
                            .gap_4()
                            .children(
                                // Each feature's icon, and its title and
                                // detail, each with its id,
                                // `hub-open-feature-<symbol>-…`, spelled out
                                // rather than formatted in every frame.
                                [
                                    (
                                        "clock",
                                        ("hub-open-feature-clock-title", "Explore history"),
                                        (
                                            "hub-open-feature-clock-detail",
                                            "Follow branches and inspect any commit.",
                                        ),
                                    ),
                                    (
                                        "changes",
                                        ("hub-open-feature-changes-title", "Review and commit"),
                                        (
                                            "hub-open-feature-changes-detail",
                                            "Compare files, stage changes, and commit.",
                                        ),
                                    ),
                                ]
                                .map(|(symbol, title, detail)| {
                                    div()
                                        .flex()
                                        .items_start()
                                        .gap_3()
                                        .child(hub_icon(symbol, 16., colors.muted_foreground))
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w_0()
                                                .flex()
                                                .flex_col()
                                                .gap_1()
                                                .child(
                                                    hub_text(title.0, title.1)
                                                        .text_size(crate::appearance::ui_text(12.))
                                                        .font_weight(FontWeight::MEDIUM),
                                                )
                                                .child(
                                                    hub_text(detail.0, detail.1)
                                                        .text_size(crate::appearance::ui_text(11.))
                                                        .line_height(relative(1.5))
                                                        .text_color(colors.muted_foreground),
                                                ),
                                        )
                                }),
                            ),
                    )
            })
            .when(self.mode != ProjectMode::Open, |panel| {
                panel
                    .child(
                        div()
                            // Return in a field submits from here, once per
                            // press ([`Self::submit_on_enter`]).
                            .on_key_down(cx.listener(Self::submit_on_enter))
                            .flex()
                            .flex_col()
                            .gap_4()
                            .when(self.mode == ProjectMode::Clone, |form| {
                                form.child(self.field(
                                    "Repository URL or local path",
                                    &self.source,
                                    cx,
                                ))
                            })
                            .child(self.field("Project folder name", &self.name, cx))
                            .child(
                                div()
                                    .on_children_prepainted(
                                        self.reveal_on_focus(self.parent_field_focus.clone(), cx),
                                    )
                                    .id("project-parent-field")
                                    .debug_selector(|| "project-parent-field".into())
                                    .track_focus(&self.parent_field_focus)
                                    .tab_stop(false)
                                    .flex()
                                    .flex_col()
                                    .gap_2()
                                    .child(
                                        hub_text("project-parent-label", "Parent folder")
                                            .text_size(crate::appearance::ui_text(12.))
                                            .font_weight(FontWeight::MEDIUM),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .gap_2()
                                            .child(
                                                div().flex_1().min_w_0().child(
                                                    Input::new(&self.parent)
                                                        .aria_label("Parent folder")
                                                        .disabled(self.unavailable()),
                                                ),
                                            )
                                            .child(
                                                Button::new("hub-choose-parent")
                                                    .label("Browse…")
                                                    .icon(
                                                        Icon::default()
                                                            .path("icons/folder.svg")
                                                            .size(px(14.)),
                                                    )
                                                    .disabled(self.unavailable())
                                                    .on_click(cx.listener(
                                                        |this, _, window, cx| {
                                                            this.choose_folder(false, window, cx)
                                                        },
                                                    )),
                                            ),
                                    ),
                            )
                            .when(self.mode == ProjectMode::Create, |form| {
                                form.child(self.field("Initial branch", &self.branch, cx))
                            }),
                    )
                    .when_some(self.destination(cx).ok(), |panel, destination| {
                        let display = destination.display().to_string();
                        let tooltip = display.clone();
                        panel.child(
                            crate::native_accessibility::static_text(
                                div().id("hub-destination-preview"),
                                format!("New repository folder: {display}"),
                            )
                            .tooltip(move |window, cx| {
                                Tooltip::new(tooltip.clone()).build(window, cx)
                            })
                            .min_w_0()
                            .rounded(px(8.))
                            .bg(colors.background)
                            .px_3()
                            .py_2()
                            .flex()
                            .items_start()
                            .gap_2()
                            .child(hub_icon("folder", 14., colors.muted_foreground))
                            .child(
                                div()
                                    .min_w_0()
                                    .flex_1()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_size(crate::appearance::ui_text(10.))
                                            .text_color(colors.muted_foreground)
                                            .child("New repository folder"),
                                    )
                                    .child(
                                        div()
                                            .text_size(crate::appearance::ui_text(11.))
                                            .truncate()
                                            .child(display),
                                    ),
                            ),
                        )
                    })
                    .child(
                        div()
                            .on_children_prepainted(
                                self.reveal_on_focus(self.submit_focus.clone(), cx),
                            )
                            .id("project-submit-field")
                            .debug_selector(|| "project-submit-field".into())
                            .track_focus(&self.submit_focus)
                            .tab_stop(false)
                            .w_full()
                            .child(
                                Button::new("hub-submit")
                                    .primary()
                                    .h(crate::appearance::ui_size(40.))
                                    .w_full()
                                    .icon(Icon::default().path(hub_icon_path(symbol)).size(px(16.)))
                                    .disabled(self.unavailable())
                                    .loading(self.busy)
                                    .label(if self.busy {
                                        self.busy_label()
                                    } else if self.mode == ProjectMode::Clone {
                                        "Clone project"
                                    } else {
                                        "Create project"
                                    })
                                    .on_click(cx.listener(|this, _, _, cx| this.submit(cx))),
                            ),
                    )
            })
            .children(form_feedback)
            .into_any_element()
    }
}

#[cfg(test)]
thread_local! {
    /// What each recent row last read in its location line, its tooltip and
    /// its accessible name.
    static LOCATION_DRAWN: RefCell<HashMap<usize, (String, String, String)>> = RefCell::default();
}

/// What recent row `index` last read in its location line, its tooltip and
/// its accessible name.
#[cfg(test)]
pub(crate) fn location_drawn(index: usize) -> Option<(String, String, String)> {
    LOCATION_DRAWN.with_borrow(|drawn| drawn.get(&index).cloned())
}

/// Where the recent rows lay as their list last drew them, and the row a
/// press is awaiting an answer for.
#[derive(Default)]
struct RecentRows {
    drawn: HashMap<usize, Bounds<Pixels>>,
    pressed: Option<PressedRow>,
}

/// A recent row a press asked to open, and where it lay in the last two
/// frames that drew it: a refusal judges it where the frame that first draws
/// the refusal puts it, and can hold it where it lay before that frame.
struct PressedRow {
    index: usize,
    /// Its project, which a refusal stays with.
    path: PathBuf,
    before: Option<Bounds<Pixels>>,
    now: Option<Bounds<Pixels>>,
}

impl RecentRows {
    fn press(&mut self, index: usize, path: PathBuf) {
        self.pressed = Some(PressedRow {
            index,
            path,
            before: None,
            now: self.drawn.get(&index).copied(),
        });
    }

    fn drawn(&mut self, index: usize, bounds: Bounds<Pixels>) {
        self.drawn.insert(index, bounds);
        if let Some(pressed) = self.pressed.as_mut().filter(|row| row.index == index) {
            pressed.before = pressed.now.replace(bounds);
        }
    }

    /// The pressed row's project and its place before this frame and in
    /// it, ending the wait.
    fn take_pressed(&mut self) -> Option<(PathBuf, Bounds<Pixels>, Bounds<Pixels>)> {
        let pressed = self.pressed.take()?;
        let (before, now) = pressed.before.zip(pressed.now)?;
        Some((pressed.path, before, now))
    }
}

/// Whether a feedback line drawn at `drawn` lies in the `body`'s view as the
/// body now stands: a scroll since then carries the line with it.
fn feedback_in_view((line, at): DrawnLine, body: &ScrollHandle) -> bool {
    let moved = line + point(px(0.), body.offset().y - at);
    let view = body.bounds();
    moved.top() >= view.top() && moved.bottom() <= view.bottom() + px(0.5)
}

/// Where the body goes for a feedback line first drawn at `line`: how far it
/// moves, and whether the line is then in view.
#[derive(Clone, Copy, Debug, PartialEq)]
struct FeedbackPlace {
    delta: Pixels,
    shows_line: bool,
}

/// The least scroll that shows `line` whole in `view`, where a view too
/// short for it shows its top. Answering a recent row (`pressed`, its place
/// before this frame and in it), the body moves only so far as that row stays
/// in view (DESIGN.md: the least that shows the outcome, only so far as what
/// the person is on stays): where both cannot be seen, within the `reach` the
/// body scrolls from `offset`, it keeps its place, undoing what the line's
/// coming moved the row, and the line stays out of view.
fn place_feedback(
    line: Bounds<Pixels>,
    pressed: Option<(Bounds<Pixels>, Bounds<Pixels>)>,
    view: Bounds<Pixels>,
    offset: Pixels,
    reach: Pixels,
) -> FeedbackPlace {
    let Some((before, now)) = pressed else {
        let (below, above) = (line.bottom() - view.bottom(), view.top() - line.top());
        let delta = if below <= Pixels::ZERO && above <= Pixels::ZERO {
            Pixels::ZERO
        } else if above > Pixels::ZERO || line.size.height >= view.size.height {
            // Where the view cannot hold it whole, its top stands in view.
            above
        } else {
            -below
        };
        return FeedbackPlace {
            delta,
            shows_line: true,
        };
    };
    let (top, bottom) = (line.top().min(now.top()), line.bottom().max(now.bottom()));
    let delta = if bottom - top > view.size.height {
        None
    } else if top < view.top() {
        Some(view.top() - top)
    } else if bottom > view.bottom() {
        Some(view.bottom() - bottom)
    } else {
        Some(Pixels::ZERO)
    };
    let slack = px(0.5);
    match delta.filter(|delta| offset + *delta <= slack && offset + *delta >= -reach - slack) {
        Some(delta) => FeedbackPlace {
            delta,
            shows_line: true,
        },
        None => FeedbackPlace {
            delta: before.top() - now.top(),
            shows_line: false,
        },
    }
}

/// What a focused control's ring leaves between itself and the body's edges:
/// the 8 px step of the spacing rhythm at the default interface size
/// (DESIGN.md, Typography and density), scaled with it as the controls are.
const FIRST_STOP_CLEARANCE: f32 = 8.;

/// How far clear of the body's edges the page shows a focused control: its
/// focus ring, as the theme installs it, and [`FIRST_STOP_CLEARANCE`] past
/// that. A control focus moves into comes into view so far
/// ([`ProjectHub::reveal_on_focus`]), and a failed Open's reason leaves the
/// first stop so far below the view's top ([`reveal_feedback`]), so the two
/// agree. The controls' reveal once kept a literal 12 px, whatever the ring.
fn focus_clearance(cx: &App) -> Pixels {
    crate::appearance::button_ring_room(cx) + crate::appearance::ui_size(FIRST_STOP_CLEARANCE)
}

/// Places the body for the feedback line first drawn at `line`, once this
/// frame has laid out the pressed row as well, and says the line politely
/// where it stays out of view ([`place_feedback`]). Later frames leave the
/// scroll to the person.
///
/// `arrival`, for the line of an Open that failed as the page opened on its
/// first stop (`GitTurtle::show_projects_for_failed_open`), holds what keeps
/// that stop whole: the line comes only so far as it does, as far as a
/// pressed row's does. Revealed past it at 18 pt in a 461 × 490 window, the
/// line scrolled the focused tab, ring and all, under the page's header. A
/// refusal the person asked for later, as under Choose a repository… with
/// focus still on that stop, is not one: it shows whole.
#[allow(clippy::too_many_arguments)]
fn reveal_feedback(
    line: Bounds<Pixels>,
    shown: usize,
    body: ScrollHandle,
    arrival: Option<(FocusReveal, FocusHandle)>,
    rows: Rc<RefCell<RecentRows>>,
    kept: Rc<RefCell<Option<(usize, PathBuf)>>>,
    spoken: SharedString,
    window: &mut Window,
    cx: &mut App,
) {
    window.defer(cx, move |window, cx| {
        let pressed = rows.borrow_mut().take_pressed();
        let reach = body.max_offset().y;
        let mut offset = body.offset();
        let places = pressed.as_ref().map(|(_, before, now)| (*before, *now));
        let view = body.bounds();
        let mut place = place_feedback(line, places, view, offset.y, reach);
        if let Some((reveal, first_stop)) = arrival
            && pressed.is_none()
            && first_stop.is_focused(window)
            && let Some(stop) = reveal.focused_bounds(window, cx)
        {
            // The stop's ring, as the theme installs it, and the rhythm's
            // step clear of it below the view's top.
            let room = focus_clearance(cx);
            place.delta = place.delta.max(view.top() + room - stop.top());
        }
        if !place.shows_line {
            *kept.borrow_mut() = pressed.map(|(project, ..)| (shown, project));
            crate::copy_feedback::announce(window, cx, spoken);
        }
        offset.y = (offset.y + place.delta).clamp(-reach, Pixels::ZERO);
        body.set_offset(offset);
        window.refresh();
    });
}

impl Render for ProjectHub {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // A form control focus has moved into scrolls into view in this
        // frame.
        self.body.reveal(window, cx);
        if let Some(branch) = self.pending_default_branch.take() {
            self.branch
                .update(cx, |input, cx| input.set_value(branch, window, cx));
        }
        let colors = Theme::global(cx).colors;
        // The hub spans the window's body, which inside a client-decorated
        // frame is the frame's insets narrower than the window.
        let width = crate::views::body_size(window).width;
        let compact = width < px(900.);
        let narrow = width < px(640.);
        div()
            .size_full()
            .bg(colors.background)
            .text_color(colors.foreground)
            .flex()
            .flex_col()
            .child(
                div()
                    .h(crate::appearance::ui_size(56.))
                    .px_4()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap_3()
                    .border_b_1()
                    .border_color(colors.border)
                    .when(self.can_go_back, |header| {
                        header.child(
                            Button::new("hub-back")
                                .ghost()
                                .label("Back to repository")
                                .icon(Icon::default().path("icons/arrow-left.svg").size(px(15.)))
                                .disabled(self.unavailable())
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if !this.unavailable() {
                                        cx.emit(ProjectEvent::Back);
                                    }
                                })),
                        )
                    })
                    .child(crate::app_icon(32.))
                    // The breadcrumb: the app, then the page.
                    .child(
                        hub_text("project-hub-app", "GitTurtle")
                            .text_size(crate::appearance::ui_text(16.))
                            .font_weight(FontWeight::SEMIBOLD),
                    )
                    .child(
                        div()
                            .w(px(1.))
                            .h(crate::appearance::ui_size(18.))
                            .bg(colors.border),
                    )
                    .child(
                        hub_text("project-hub-page", "Projects")
                            .text_size(crate::appearance::ui_text(12.))
                            .text_color(colors.muted_foreground),
                    )
                    .child(div().flex_1()),
            )
            .child(crate::native_accessibility::clips_children(
                div()
                    // Record even focus outside the form, so returning to the
                    // same field reveals it again. Scrolling alone leaves the
                    // record unchanged and never snaps the view back.
                    .on_children_prepainted(self.body.observe())
                    .id("project-hub-body")
                    .debug_selector(|| "project-hub-body".into())
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(self.body.scroll())
                    .flex()
                    .flex_col()
                    .items_center()
                    .px_6()
                    .py_8()
                    .when(narrow, |body| body.px_4().py_5())
                    .child(
                        div()
                            .w_full()
                            .max_w(px(1060.))
                            .flex_shrink_0()
                            .flex()
                            .flex_col()
                            .gap_6()
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_2()
                                    .child(
                                        hub_text("project-hub-heading", "Your projects")
                                            .role(Role::Heading)
                                            .text_size(crate::appearance::ui_text(32.))
                                            .font_weight(FontWeight::SEMIBOLD),
                                    )
                                    .child(
                                        hub_text(
                                            "project-hub-description",
                                            "Pick up where you left off, or start something new.",
                                        )
                                        .text_size(crate::appearance::ui_text(14.))
                                        .text_color(colors.muted_foreground),
                                    ),
                            )
                            .child(
                                div()
                                    .w_full()
                                    .h(px(if self.error.is_some() {
                                        750.
                                    } else if self.mode == ProjectMode::Open {
                                        590.
                                    } else {
                                        650.
                                    }))
                                    .flex_shrink_0()
                                    .flex()
                                    .gap_6()
                                    .when(compact, |body| {
                                        body.flex_col()
                                            .h_auto()
                                            .child(self.render_action(true, cx))
                                            .child(self.render_recent(true, cx))
                                    })
                                    .when(!compact, |body| {
                                        body.child(self.render_recent(false, cx))
                                            .child(self.render_action(false, cx))
                                    }),
                            ),
                    ),
            ))
    }
}

/// What a recent row draws for its project ([`ProjectHub::row_text`]).
struct RowText {
    /// The project's own name the text was built from, if it has one.
    custom: Option<String>,
    /// The project, which the row opens and its Rename renames.
    path: PathBuf,
    /// Its name: its own, or its folder's.
    name: SharedString,
    /// Where it is: its parent folder, after its folder's name where its own
    /// name differs, so a renamed project stays identifiable on disk.
    location: SharedString,
    /// Its whole path, the row's tooltip.
    display: SharedString,
    /// The row's accessible name.
    open: SharedString,
    /// Its Rename control's accessible name.
    rename: SharedString,
}

impl RowText {
    fn build(path: &Path, custom: Option<&str>) -> Self {
        #[cfg(test)]
        RECENT_ROWS_BUILT.set(RECENT_ROWS_BUILT.get() + 1);
        let display = path.display().to_string();
        let folder = directory_name(path);
        let name = custom.map_or_else(|| folder.clone(), str::to_owned);
        let parent = path.parent().unwrap_or(path).display().to_string();
        let location = if name == folder {
            parent
        } else {
            format!("{folder} · {parent}")
        };
        Self {
            custom: custom.map(str::to_owned),
            path: path.to_path_buf(),
            open: format!("Open {name}, {display}").into(),
            rename: format!("Rename {name}, {display}").into(),
            name: name.into(),
            location: location.into(),
            display: display.into(),
        }
    }
}

/// The recent list's count and its accessible label
/// ([`ProjectHub::count_text`]), with whether a search narrows the list and
/// the matching and recent projects' counts they were built from.
struct CountText {
    counts: (bool, usize, usize),
    count: SharedString,
    label: SharedString,
}

impl CountText {
    fn build(counts: (bool, usize, usize)) -> Self {
        #[cfg(test)]
        RECENT_COUNTS_BUILT.set(RECENT_COUNTS_BUILT.get() + 1);
        let (searching, matching, recent) = counts;
        let recent_label = crate::counted(recent, "recent project", "recent projects");
        let (count, label) = if searching {
            (
                format!("{matching} of {recent}"),
                format!(
                    "{} out of {recent_label}",
                    crate::counted(matching, "matching project", "matching projects")
                ),
            )
        } else {
            (recent.to_string(), recent_label)
        };
        Self {
            counts,
            count: count.into(),
            label: label.into(),
        }
    }
}

#[cfg(test)]
thread_local! {
    /// Test-only: the recent rows' and count's text looked up
    /// ([`ProjectHub::row_text`], [`ProjectHub::count_text`]), and the
    /// allocations the lookups made.
    static RECENT_TEXT: std::cell::Cell<(usize, u64)> =
        const { std::cell::Cell::new((0, 0)) };
    /// Test-only: how many times a recent row's text was built.
    static RECENT_ROWS_BUILT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// Test-only: how many times the recent count's text was built.
    static RECENT_COUNTS_BUILT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// The hub's icon asset for `symbol`, `icons/<symbol>.svg`, as the shared
/// helpers keep it ([`crate::icon_path`]): formatted the first time the
/// symbol is drawn, by the hub or any other view, and kept, so a frame at
/// rest formats none. The hub formatted every icon's path, its action tabs',
/// its panel's and each recent row's, on every frame.
fn hub_icon_path(symbol: &str) -> SharedString {
    #[cfg(test)]
    let before = crate::test_support::allocations();
    let path = crate::icon_path(symbol);
    #[cfg(test)]
    HUB_ICON_PATHS.with(|counted| {
        let (paths, allocations) = counted.get();
        counted.set((
            paths + 1,
            allocations + crate::test_support::allocations() - before,
        ));
    });
    path
}

#[cfg(test)]
thread_local! {
    /// Test-only: the icon paths the hub has drawn ([`hub_icon_path`]), and
    /// the allocations it made finding them.
    static HUB_ICON_PATHS: std::cell::Cell<(usize, u64)> = const { std::cell::Cell::new((0, 0)) };
}

fn hub_icon(name: &str, dimension: f32, color: Hsla) -> Svg {
    svg()
        .path(hub_icon_path(name))
        .size(px(dimension))
        .text_color(color)
        .flex_shrink_0()
}

/// `text` as static text, with no debug selector of its own: a field's
/// caption takes the field's name as its ID, which the field's selector is.
fn hub_text(
    id: impl Into<ElementId>,
    text: impl Into<SharedString>,
) -> crate::native_accessibility::StaticText<Stateful<Div>> {
    let text = text.into();
    crate::native_accessibility::static_text(div().id(id).child(text.clone()), text)
}

/// Decide what a submitted rename means. Empty input, or the folder name
/// typed back, clears the custom name instead of storing a duplicate of it.
fn chosen_name(value: &str, folder: &str) -> Result<Option<String>, String> {
    let value = value.trim();
    if value.is_empty() || value == folder {
        return Ok(None);
    }
    validate_project_name(value)?;
    Ok(Some(value.to_owned()))
}

fn unique_recent(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    paths
        .into_iter()
        .filter(|path| seen.insert(path.clone()))
        .collect()
}

fn suggested_project_name(source: &str) -> Option<String> {
    let source = source
        .trim()
        .split(['?', '#'])
        .next()?
        .trim_end_matches('/');
    let name = source.rsplit(['/', ':']).next()?.trim_end_matches(".git");
    if name.is_empty() || matches!(name, "." | "..") || name.chars().any(char::is_control) {
        None
    } else {
        Some(name.to_string())
    }
}

fn validate_destination(parent: &Path, name: &str) -> Result<PathBuf, String> {
    if parent.as_os_str().is_empty() {
        return Err("Choose a parent folder for your project.".into());
    }
    if !parent.is_absolute() {
        return Err("Use an absolute parent-folder path, or choose one with Browse.".into());
    }
    if name.trim().is_empty() {
        return Err("Give your project folder a name.".into());
    }
    if matches!(name, "." | "..") || name.contains('/') || name.chars().any(char::is_control) {
        return Err("Use a single folder name without slashes or control characters.".into());
    }
    Ok(parent.join(name))
}

fn validate_source(source: &str) -> Result<(), String> {
    if source.is_empty() {
        return Err("Enter a repository URL or local path to clone.".into());
    }
    if source.starts_with('-') || source.chars().any(char::is_control) {
        return Err(
            "Enter a repository URL or path without control characters or a leading dash.".into(),
        );
    }
    Ok(())
}

fn validate_branch(branch: &str) -> Result<(), String> {
    if branch.is_empty() {
        return Err("Enter a name for the initial branch.".into());
    }
    if branch.starts_with('-')
        || matches!(branch, "@" | "HEAD")
        || branch.ends_with('.')
        || branch.contains("..")
        || branch.contains("@{")
        || branch
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || "~^:?*[\\".contains(c))
        || branch
            .split('/')
            .any(|part| part.is_empty() || part.starts_with('.') || part.ends_with(".lock"))
    {
        return Err("Use a valid branch name, such as main or feature/start.".into());
    }
    Ok(())
}

impl GitTurtle {
    /// The name this project shows in the client. Everything that lists a
    /// repository goes through here, so a rename is consistent across the
    /// hub, tabs, menus and status text.
    pub(super) fn project_name(&self, path: &Path) -> String {
        self.project_names
            .get(path)
            .cloned()
            .unwrap_or_else(|| directory_name(path))
    }

    pub(super) fn open_rename_project(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if window.has_active_dialog(cx) || window.has_active_sheet(cx) {
            return;
        }
        let current = self.project_names.get(&path).cloned();
        // Each opening starts with the saved value, including after Cancel.
        let owner = cx.entity().downgrade();
        let form = cx.new(|cx| RenameProjectForm::new(owner, path, current, window, cx));
        let focus_form = form.downgrade();
        self.rename_project = Some(form.clone());
        window.open_alert_dialog(cx, move |dialog, window, cx| {
            let submit = form.clone();
            let cancel = form.clone();
            let pending = form.read(cx).pending;
            dialog
                .title("Rename project")
                .width(crate::appearance::dialog_width(px(520.), window))
                .max_h(crate::appearance::dialog_max_height(window))
                .child(form.clone())
                .keyboard(!pending)
                .footer(
                    DialogFooter::new()
                        .child(
                            crate::button("cancel-rename-project", "Cancel", "", false)
                                .disabled(pending)
                                .on_click(|_, window, cx| {
                                    window.dispatch_action(Box::new(Cancel), cx)
                                }),
                        )
                        .child(
                            crate::button(
                                "save-project-name",
                                if pending { "Saving…" } else { "Save name" },
                                "",
                                false,
                            )
                            .primary()
                            .disabled(pending)
                            .on_click(|_, window, cx| {
                                window.dispatch_action(Box::new(Confirm { secondary: false }), cx)
                            }),
                        ),
                )
                .on_ok(move |_, window, cx| {
                    submit.update(cx, |form, cx| form.submit(window, cx));
                    false
                })
                .on_cancel(move |_, _, cx| {
                    cancel.update(cx, |form, _| {
                        if form.pending {
                            return false;
                        }
                        form.visible = false;
                        true
                    })
                })
        });
        window.refresh();
        window.on_next_frame(move |window, cx| {
            let _ = focus_form.update(cx, |form, cx| {
                if form.visible {
                    form.input.update(cx, |input, cx| {
                        input.focus(window, cx);
                        input.select_all(window, cx);
                    });
                }
            });
        });
    }

    /// App data only. `None` restores the folder name. The write shares the
    /// serialized preference executor with settings and recents.
    pub(super) fn rename_project(
        &mut self,
        path: PathBuf,
        name: Option<String>,
        form: WeakEntity<RenameProjectForm>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let response = self
            .preferences_writer
            .submit(move || Preferences::save_project_name(&path, name.as_deref()));
        cx.spawn_in(window, async move |this, cx| {
            let result = response.await.unwrap_or_else(|_| {
                Err(anyhow::anyhow!(
                    "The settings writer stopped before reporting a result"
                ))
            });
            let _ = this.update_in(cx, |this, window, cx| {
                this.finish_project_rename(form, result, window, cx);
            });
        })
        .detach();
    }

    fn finish_project_rename(
        &mut self,
        form: WeakEntity<RenameProjectForm>,
        result: anyhow::Result<Preferences>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let visible = self.rename_project.as_ref().is_some_and(|current| {
            current.entity_id() == form.entity_id() && current.read(cx).visible
        });
        match result {
            Ok(preferences) => {
                self.project_names = preferences.project_names;
                let names = self.project_names.clone();
                self.hub.update(cx, |hub, cx| hub.set_names(names, cx));
                self.rebuild_navigation(cx);
                self.rebuild_project_rows();
                let _ = form.update(cx, |form, cx| {
                    form.pending = false;
                    form.visible = false;
                    cx.notify();
                });
                if visible {
                    window.close_dialog(cx);
                    self.rename_project = None;
                }
            }
            Err(error) => {
                let _ = form.update(cx, |form, cx| {
                    form.pending = false;
                    form.refuse(format!("Could not save the project name: {error:#}"));
                    cx.notify();
                });
            }
        }
        cx.notify();
    }
}

/// Editing a project's client-side name. The folder on disk is never touched,
/// so this form validates presentation only and keeps its text on failure.
pub struct RenameProjectForm {
    owner: WeakEntity<GitTurtle>,
    path: PathBuf,
    folder: String,
    named: bool,
    input: Entity<InputState>,
    error: Option<String>,
    /// How many times the error line has said why a name was not saved,
    /// which keys its node: the same reason again is a fresh node, which an
    /// assistive technology announces again.
    errors_shown: usize,
    pending: bool,
    visible: bool,
    _subscription: Subscription,
}

impl RenameProjectForm {
    fn new(
        owner: WeakEntity<GitTurtle>,
        path: PathBuf,
        current: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let folder = directory_name(&path);
        let named = current.is_some();
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(folder.clone())
                .default_value(current.unwrap_or_else(|| folder.clone()))
        });
        // Return in the field submits through the dialog's default action
        // (`on_ok`), which the field passes the key on to and which acts
        // once for a held Enter, under X11 too. The field's own `PressEnter`
        // submitted as well, on every repeat, held or not, so a hold after
        // a refused save began another save on each.
        let subscription = cx.subscribe(&input, |this, _, event, cx| {
            if let InputEvent::Change = event {
                this.error = None;
                cx.notify();
            }
        });
        Self {
            owner,
            path,
            folder,
            named,
            input,
            error: None,
            errors_shown: 0,
            pending: false,
            visible: true,
            _subscription: subscription,
        }
    }

    /// Says why the name was not saved, heard again however often the same
    /// reason is given.
    fn refuse(&mut self, error: String) {
        self.errors_shown = self.errors_shown.wrapping_add(1);
        self.error = Some(error);
    }

    /// Keep the reviewed target and text until persistence reports success.
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending || !self.visible {
            return;
        }
        let name = match chosen_name(self.input.read(cx).value().as_ref(), &self.folder) {
            Ok(name) => name,
            Err(error) => {
                self.refuse(error);
                cx.notify();
                return;
            }
        };
        let path = self.path.clone();
        let form = cx.entity().downgrade();
        self.pending = true;
        self.error = None;
        if self
            .owner
            .update(cx, |owner, cx| {
                owner.rename_project(path, name, form, window, cx)
            })
            .is_err()
        {
            self.pending = false;
            self.refuse("The project window is no longer available.".into());
        }
        cx.notify();
    }
}

impl Render for RenameProjectForm {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = Theme::global(cx).colors;
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                hub_text(
                    "rename-project-description",
                    "This name is shown in GitTurtle only. The folder on disk keeps its own name.",
                )
                .text_size(crate::appearance::ui_text(12.))
                .line_height(relative(1.5))
                .text_color(colors.muted_foreground),
            )
            .child(
                hub_text(
                    "rename-project-folder",
                    format!("Folder: {}", self.path.display()),
                )
                .text_size(crate::appearance::ui_text(11.))
                .text_color(colors.muted_foreground),
            )
            .child(
                hub_text("rename-project-label", "Project name")
                    .text_size(crate::appearance::ui_text(12.))
                    .font_weight(FontWeight::MEDIUM),
            )
            .child(
                Input::new(&self.input)
                    .aria_label("Project name")
                    .disabled(self.pending),
            )
            .child(
                hub_text(
                    "rename-project-hint",
                    if self.named {
                        format!("Leave this empty to go back to {}.", self.folder)
                    } else {
                        format!("Empty keeps the folder name {}.", self.folder)
                    },
                )
                .text_size(crate::appearance::ui_text(11.))
                .text_color(colors.muted_foreground),
            )
            .children(self.error.as_ref().map(|error| {
                div()
                    // A fresh node for each refusal, heard again however
                    // often the same one is given.
                    .id(("rename-project-error", self.errors_shown))
                    .alert(error.clone())
                    .rounded(px(8.))
                    .border_1()
                    .border_color(colors.danger)
                    .p_2()
                    .text_size(crate::appearance::ui_text(12.))
                    .text_color(colors.danger)
                    .child(error.clone())
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use std::{cell::RefCell, rc::Rc};

    /// Projects opens with focus on its selected action tab, whose ring the
    /// first frame draws whole, at 13 pt in a full window and 18 pt in a
    /// 461 × 490 half tile, and the application's keys still reach the
    /// window. It opened with focus on the window's root, which draws
    /// nothing, so the page showed no focus until a Tab landed on Menu.
    #[gpui::test]
    async fn projects_open_with_the_selected_action_tab_focused_and_ringed(
        cx: &mut TestAppContext,
    ) {
        let (_fixture, app, cx) = crate::views::tests::history_window(cx).await;
        crate::focus_ring::sweep::prepare(&app, cx);
        for (text, width, height) in [(13, 1280., 800.), (18, 461., 490.)] {
            let at = format!("{text} pt in {width} × {height}");
            cx.update(|window, cx| crate::appearance::apply_text_sizes(text, 12, window, cx));
            cx.simulate_resize(size(px(width), px(height)));
            cx.update(|window, cx| app.update(cx, |app, cx| app.show_projects(window, cx)));
            crate::tags::tests::draw(cx);
            cx.update(|window, cx| {
                let app = app.read(cx);
                assert!(app.page == crate::AppPage::Projects, "{at}");
                assert!(
                    app.hub.read(cx).selected_mode_focused(window),
                    "{at}: the selected action tab holds focus"
                );
                assert!(
                    app.app_focus.contains_focused(window, cx),
                    "{at}: inside the window's handlers"
                );
            });
            crate::tags::tests::assert_ring_painted(cx, "project-mode-Open", &at);
            crate::tags::tests::assert_rings_whole(cx, &["project-mode-Open"]);
            cx.update(|window, cx| app.update(cx, |app, cx| app.return_from_page(window, cx)));
            crate::tags::tests::draw(cx);
        }
        cx.update(|window, cx| crate::appearance::apply_text_sizes(13, 12, window, cx));
    }

    #[gpui::test]
    fn keyboard_focus_reveals_project_fields_and_submit_at_small_window(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            Theme::global_mut(cx).font_size = px(18.);
        });
        let captured = Rc::new(RefCell::new(None));
        let observed = captured.clone();
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let hub =
                cx.new(|cx| ProjectHub::new(vec![], HashMap::new(), "main".into(), window, cx));
            *captured.borrow_mut() = Some(hub.clone());
            gpui_kit::component::Root::new(hub, window, cx)
        });
        let hub = observed.borrow_mut().take().unwrap();
        cx.simulate_resize(size(px(1000.), px(680.)));

        fn settle(cx: &mut VisualTestContext) {
            for _ in 0..2 {
                cx.update(|window, cx| window.draw(cx).clear(cx));
                cx.run_until_parked();
            }
        }
        fn assert_visible(
            cx: &mut VisualTestContext,
            hub: &Entity<ProjectHub>,
            selector: &'static str,
        ) {
            let bounds = cx.debug_bounds(selector).expect("rendered focus target");
            let viewport = cx.read(|cx| hub.read(cx).body.scroll().bounds());
            assert!(
                bounds.top() >= viewport.top() && bounds.bottom() <= viewport.bottom(),
                "{selector}: {bounds:?} must be inside {viewport:?}"
            );
        }

        for mode in [ProjectMode::Clone, ProjectMode::Create] {
            cx.update(|window, cx| {
                hub.update(cx, |hub, cx| hub.change_mode(mode, window, cx));
            });
            settle(cx);
            let first = if mode == ProjectMode::Clone {
                "Repository URL or local path"
            } else {
                "Project folder name"
            };
            assert_visible(cx, &hub, first);
            if mode == ProjectMode::Clone {
                cx.simulate_keystrokes("tab");
                settle(cx);
                cx.update(|window, cx| {
                    assert!(hub.read(cx).name.focus_handle(cx).is_focused(window))
                });
                assert_visible(cx, &hub, "Project folder name");
            }
            cx.simulate_keystrokes("tab");
            settle(cx);
            cx.update(|window, cx| {
                assert!(hub.read(cx).parent.focus_handle(cx).is_focused(window))
            });
            assert_visible(cx, &hub, "project-parent-field");
            cx.simulate_keystrokes("tab");
            settle(cx);
            cx.update(|window, cx| {
                assert!(hub.read(cx).parent_field_focus.contains_focused(window, cx));
                assert!(!hub.read(cx).parent.focus_handle(cx).is_focused(window));
            });
            assert_visible(cx, &hub, "project-parent-field");
            if mode == ProjectMode::Create {
                cx.simulate_keystrokes("tab");
                settle(cx);
                cx.update(|window, cx| {
                    assert!(hub.read(cx).branch.focus_handle(cx).is_focused(window))
                });
                assert_visible(cx, &hub, "Initial branch");
            }
            cx.simulate_keystrokes("tab");
            settle(cx);
            cx.update(|window, cx| assert!(hub.read(cx).submit_focus.contains_focused(window, cx)));
            assert_visible(cx, &hub, "project-submit-field");

            // A user may scroll away from a focused control. Unchanged focus
            // must not schedule another reveal or continuously redraw.
            cx.update(|window, cx| {
                hub.read(cx).body.scroll().set_offset(point(px(0.), px(0.)));
                window.refresh();
            });
            settle(cx);
            cx.read(|cx| assert_eq!(hub.read(cx).body.scroll().offset().y, px(0.)));
            cx.simulate_keystrokes("shift-tab");
            settle(cx);
            assert_visible(
                cx,
                &hub,
                if mode == ProjectMode::Clone {
                    "project-parent-field"
                } else {
                    "Initial branch"
                },
            );
        }
    }

    /// Every frame painted after focus moves into a form field, by Tab or
    /// by a click on a field the page shows only in part, the first frame
    /// that draws the focus included, shows the field [`focus_clearance`]
    /// clear of the page's edges, or as near as the page's reach allows, and
    /// a field Tab scrolls to comes only so far: the clearance a failed
    /// Open's reason leaves the first stop, the ring's room and the spacing
    /// rhythm's 8 px step scaled with the interface size, as at 18 pt. The
    /// fields once kept a literal 12 px, and then an unscaled 8 px.
    #[gpui::test]
    fn project_hub_reveals_fields_in_the_frame_that_draws_focus(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            Theme::global_mut(cx).font_size = px(18.);
        });
        let captured = Rc::new(RefCell::new(None));
        let observed = captured.clone();
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let hub =
                cx.new(|cx| ProjectHub::new(vec![], HashMap::new(), "main".into(), window, cx));
            *captured.borrow_mut() = Some(hub.clone());
            gpui_kit::component::Root::new(hub, window, cx)
        });
        let hub = observed.borrow_mut().take().unwrap();
        cx.update(|window, cx| crate::appearance::apply_text_sizes(18, 12, window, cx));
        cx.simulate_resize(size(px(1000.), px(680.)));
        // What the page keeps clear of its edges at 18 pt: the ring's room
        // and the 8 px step, scaled as the controls are.
        fn clearance(cx: &App) -> Pixels {
            crate::appearance::button_ring_room(cx) + crate::appearance::ui_size(8.)
        }
        // Scaled, the clearance falls between the window's device pixels,
        // to which the page snaps the field's top and its height each: up
        // to one device pixel either way, half a logical pixel at the test
        // window's factor of 2.
        const SLACK: Pixels = px(0.5);
        assert!(
            cx.read(clearance) > cx.read(crate::appearance::button_ring_room) + px(8.),
            "the step scales at 18 pt"
        );

        fn settle(cx: &mut VisualTestContext) {
            for _ in 0..2 {
                cx.update(|window, cx| window.draw(cx).clear(cx));
                cx.run_until_parked();
            }
        }
        /// Every frame painted since the last call shows the focused field
        /// [`focus_clearance`] clear of the page's edges; returns the last.
        fn assert_clear(
            cx: &mut VisualTestContext,
            hub: &Entity<ProjectHub>,
            after: &str,
        ) -> crate::focus_reveal::tests::Painted {
            let mut frames = cx.read(|cx| hub.read(cx).body.take_painted());
            assert!(!frames.is_empty(), "{after} paints the page");
            let (margin, slack) = (cx.read(clearance), SLACK);
            for (index, frame) in frames.iter().enumerate() {
                let field = frame
                    .control
                    .unwrap_or_else(|| panic!("frame {index} after {after} draws the field"));
                let viewport = frame.viewport;
                let top =
                    field.top() - margin + slack >= viewport.top() || frame.offset >= Pixels::ZERO;
                let bottom = field.bottom() + margin <= viewport.bottom() + slack
                    || frame.offset <= -frame.max_offset + slack;
                assert!(
                    top && bottom,
                    "frame {index} of {} after {after} paints the field at {field:?}, less \
                     than {margin:?} inside the page, {viewport:?}, at offset {:?}",
                    frames.len(),
                    frame.offset
                );
            }
            frames.pop().expect("a frame")
        }

        cx.update(|window, cx| {
            hub.update(cx, |hub, cx| {
                hub.change_mode(ProjectMode::Create, window, cx)
            });
        });
        settle(cx);
        let mut scrolled = false;
        let margin = cx.read(clearance);
        for _ in 0..4 {
            cx.read(|cx| hub.read(cx).body.take_painted());
            let before = cx.read(|cx| hub.read(cx).body.scroll().offset().y);
            cx.simulate_keystrokes("tab");
            settle(cx);
            let last = assert_clear(cx, &hub, "tab");
            let after = cx.read(|cx| hub.read(cx).body.scroll().offset().y);
            if after < before && after > -last.max_offset + px(0.01) {
                let field = last.control.expect("the field");
                assert!(
                    (last.viewport.bottom() - margin - field.bottom()).abs() < SLACK,
                    "Tab reveals the field {margin:?} clear of the page's foot, no more: \
                     {field:?} in {:?}",
                    last.viewport
                );
            }
            scrolled |= after < px(0.);
        }
        assert!(scrolled, "Tab scrolls the page");
        for _ in 0..3 {
            cx.read(|cx| hub.read(cx).body.take_painted());
            cx.simulate_keystrokes("shift-tab");
            settle(cx);
            assert_clear(cx, &hub, "shift-tab");
        }

        // In a shorter window, scroll until the folder name field's input
        // shows only its upper part at the page's lower edge, then click
        // there.
        cx.simulate_resize(size(px(1000.), px(420.)));
        settle(cx);
        let field = cx.debug_bounds("Project folder name").expect("the field");
        let viewport = cx.read(|cx| hub.read(cx).body.scroll().bounds());
        let offset = cx.read(|cx| hub.read(cx).body.scroll().offset().y);
        let away = offset + (viewport.bottom() - px(15.) - field.bottom()) + field.size.height / 2.;
        cx.update(|window, cx| {
            let body = hub.read(cx).body.scroll();
            assert!(away < px(0.) && -away <= body.max_offset().y);
            body.set_offset(point(px(0.), away));
            window.refresh();
        });
        settle(cx);
        let field = cx.debug_bounds("Project folder name").expect("the field");
        assert!(field.top() < viewport.bottom() && field.bottom() > viewport.bottom());
        cx.read(|cx| hub.read(cx).body.take_painted());
        cx.simulate_click(
            point(field.center().x, viewport.bottom() - px(4.)),
            Modifiers::default(),
        );
        settle(cx);
        cx.update(|window, cx| assert!(hub.read(cx).name.focus_handle(cx).is_focused(window)));
        assert_clear(cx, &hub, "a click");
        cx.update(|window, cx| crate::appearance::apply_text_sizes(13, 12, window, cx));
    }

    #[gpui::test]
    fn populated_project_inputs_keep_their_names_across_modes(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (hub, cx) = cx.add_window_view(|window, cx| {
            ProjectHub::new(vec![], HashMap::new(), "main".into(), window, cx)
        });
        for mode in [ProjectMode::Clone, ProjectMode::Create, ProjectMode::Open] {
            cx.update(|window, cx| {
                hub.update(cx, |hub, cx| {
                    hub.change_mode(mode, window, cx);
                    hub.parent.update(cx, |input, cx| {
                        input.set_value("/fixture/projects", window, cx)
                    });
                    hub.name.update(cx, |input, cx| {
                        input.set_value("A multilingual 项目", window, cx)
                    });
                });
                window.draw(cx).clear(cx);
            });
            let inputs = hub.read_with(cx, |hub, _| {
                let mut inputs = vec![(
                    hub.search.clone(),
                    "Find a recent project by name or folder",
                )];
                if mode != ProjectMode::Open {
                    inputs.extend([
                        (hub.parent.clone(), "Parent folder"),
                        (hub.name.clone(), "Project folder name"),
                    ]);
                    inputs.push(if mode == ProjectMode::Clone {
                        (hub.source.clone(), "Repository URL or local path")
                    } else {
                        (hub.branch.clone(), "Initial branch")
                    });
                }
                inputs
            });
            for (input, label) in inputs {
                // Built inside a draw, so the field's elements go with the
                // test's app (`views::tests::input_node`).
                let node = crate::views::tests::input_node(cx, &input);
                assert_eq!(node.role(), Role::TextInput);
                assert_eq!(node.label(), Some(label));
                assert!(node.supports_action(gpui::AccessibleAction::SetValue));
            }
        }
    }

    #[test]
    fn clone_names_support_https_ssh_and_local_sources() {
        for source in [
            "https://example.com/team/turtle.git",
            "git@example.com:team/turtle.git",
            "/tmp/turtle.git/",
            "https://example.com/team/turtle.git?ref=main",
        ] {
            assert_eq!(suggested_project_name(source).as_deref(), Some("turtle"));
        }
        assert_eq!(suggested_project_name("../.."), None);
    }

    #[test]
    fn clone_source_accepts_git_transports_and_rejects_empty_or_hidden_input() {
        for source in [
            "https://example.com/team/turtle.git",
            "git@example.com:team/turtle.git",
            "/tmp/local repo",
        ] {
            assert!(validate_source(source).is_ok());
        }
        for source in [
            "",
            "--upload-pack=command",
            "host/repo\nother",
            "host/repo\0",
        ] {
            assert!(validate_source(source).is_err());
        }
    }

    #[test]
    fn destination_keeps_the_project_under_the_chosen_parent() {
        assert_eq!(
            validate_destination(Path::new("/tmp/projects"), "my project").unwrap(),
            PathBuf::from("/tmp/projects/my project")
        );
        for name in ["", " ", ".", "..", "../outside", "/absolute", "bad\0name"] {
            assert!(validate_destination(Path::new("/tmp/projects"), name).is_err());
        }
        assert!(validate_destination(Path::new("relative"), "project").is_err());
    }

    #[test]
    fn initial_branch_rejects_invalid_refs_before_submission() {
        for branch in ["main", "feature/start", "release-1.0"] {
            assert!(validate_branch(branch).is_ok());
        }
        for branch in [
            "",
            "HEAD",
            "@",
            "-main",
            "two words",
            "main..next",
            "main@{1}",
            ".hidden",
            "feature/.hidden",
            "main.lock",
            "one//two",
            "one/",
            "one\\two",
        ] {
            assert!(validate_branch(branch).is_err(), "{branch}");
        }
    }

    #[gpui::test]
    fn the_recent_row_rename_control_asks_for_its_own_project(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let client = PathBuf::from("/projects/turtle-client");
        let core = PathBuf::from("/projects/turtle-core");
        let recent = vec![client.clone(), core.clone()];
        let names = HashMap::from([(client.clone(), "Shell".to_owned())]);
        let captured = Rc::new(RefCell::new(None));
        let observed = captured.clone();
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let hub = cx.new(|cx| ProjectHub::new(recent, names, "main".into(), window, cx));
            *captured.borrow_mut() = Some(hub.clone());
            gpui_kit::component::Root::new(hub, window, cx)
        });
        let hub = observed.borrow_mut().take().unwrap();
        let events = Rc::new(RefCell::new(Vec::new()));
        let recorder = events.clone();
        let subscription = cx.update(|_, cx| {
            cx.subscribe(&hub, move |_, event: &ProjectEvent, _| {
                recorder.borrow_mut().push(event.clone())
            })
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();

        // Each row asks for the project on that row, not the first one.
        for (selector, path) in [
            ("rename-recent-project-0", &client),
            ("rename-recent-project-1", &core),
        ] {
            let bounds = cx.debug_bounds(selector).expect("rendered rename control");
            cx.simulate_click(bounds.center(), Modifiers::default());
            cx.run_until_parked();
            assert_eq!(
                events.borrow().last(),
                Some(&ProjectEvent::Rename(path.clone()))
            );
        }
        // Asking to rename must never start opening a project.
        assert!(
            !events
                .borrow()
                .iter()
                .any(|event| matches!(event, ProjectEvent::Open(_)))
        );
        cx.read(|cx| assert!(!hub.read(cx).busy));
        drop(subscription);
    }

    /// The hub draws every icon from the path the shared helpers keep for
    /// its symbol ([`hub_icon_path`]): frames at rest find each icon's path
    /// without an allocation, on Open, its tabs', its panel's, its two
    /// features' and each recent row's, and on Clone, whose panel and submit
    /// draw the tab's own symbol, which a mode change formats again nowhere;
    /// with the kept paths forgotten, the next frame formats each symbol
    /// once, however many icons draw it, and frames after it none. The hub
    /// formatted every icon's path in every frame, at least eleven on Open.
    #[gpui::test]
    fn the_hub_formats_no_icon_path_at_rest(cx: &mut TestAppContext) {
        // Empty, as a GPUI test's seeds share the thread and its cache.
        crate::forget_symbol_text();
        cx.update(gpui_kit::init);
        let recent = vec![
            PathBuf::from("/projects/turtle-client"),
            PathBuf::from("/projects/turtle-core"),
        ];
        let captured = Rc::new(RefCell::new(None));
        let observed = captured.clone();
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let hub =
                cx.new(|cx| ProjectHub::new(recent, HashMap::new(), "main".into(), window, cx));
            *captured.borrow_mut() = Some(hub.clone());
            gpui_kit::component::Root::new(hub, window, cx)
        });
        let hub = observed.borrow_mut().take().unwrap();
        cx.simulate_resize(size(px(1280.), px(800.)));
        let draw = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                window.refresh();
                window.draw(cx).clear(cx);
            });
            cx.run_until_parked();
        };
        let formatted = || crate::SYMBOL_TEXT_FORMATTED.get();
        let kept = || crate::SYMBOL_TEXT.with_borrow(|kept| kept.len());
        // The icon paths each frame at rest draws, none allocating or
        // formatting.
        let at_rest = |at: &str, cx: &mut VisualTestContext| {
            let before = formatted();
            HUB_ICON_PATHS.set((0, 0));
            for _ in 0..3 {
                hub.update(cx, |_, cx| cx.notify());
                draw(cx);
            }
            let (paths, allocations) = HUB_ICON_PATHS.get();
            assert_eq!(paths % 3, 0, "{at}: each frame draws the same icons");
            assert_eq!(allocations, 0, "{at}: frames at rest allocate no icon path");
            assert_eq!(formatted(), before, "{at}: frames at rest format no symbol");
            paths / 3
        };
        draw(cx);
        assert!(formatted() > 0, "the first frame formats the hub's symbols");
        let open = at_rest("Open", cx);
        assert!(
            open >= 11,
            "each frame draws at least Open's three tabs', its panel's, two \
             features', the recent heading's and two rows' two icons each: {open}"
        );

        let before = formatted();
        cx.update(|window, cx| {
            hub.update(cx, |hub, cx| {
                hub.change_mode(ProjectMode::Clone, window, cx)
            })
        });
        draw(cx);
        assert_eq!(
            formatted(),
            before,
            "Clone's panel and submit draw its tab's symbol, kept"
        );
        let clone = at_rest("Clone", cx);
        assert!(
            clone >= 10,
            "each frame draws at least Clone's three tabs', its panel's, its \
             submit's, the recent heading's and two rows' two icons each: {clone}"
        );

        crate::forget_symbol_text();
        let before = formatted();
        HUB_ICON_PATHS.set((0, 0));
        draw(cx);
        let (paths, allocations) = HUB_ICON_PATHS.get();
        assert!(allocations > 0, "forgotten, the paths are formatted again");
        assert_eq!(
            formatted() - before,
            kept(),
            "each symbol drawn is formatted once"
        );
        assert!(
            paths > kept(),
            "though some symbols draw more than one icon: {paths} paths, {} symbols",
            kept()
        );
        assert_eq!(at_rest("Clone again", cx), clone);
        assert_eq!(
            hub_icon_path("remote").as_ref(),
            "icons/remote.svg",
            "a symbol's kept path"
        );
    }

    /// A recent row's text is kept for its project ([`ProjectHub::row_text`])
    /// and the list's count with its label ([`ProjectHub::count_text`]):
    /// with three recent projects, one renamed, the first frame builds each
    /// row's text and the count once, and frames at rest build none of it,
    /// their lookups allocating nothing; renaming another project rebuilds
    /// that row's text once and no other, and a project newly opened builds
    /// its own row's text and the count once, the others kept; with one
    /// project dropped as another opens, the dropped one's text goes and
    /// every row is still drawn; and a search every project matches, one
    /// that narrows the list and a cleared one each build the count and its
    /// label once and no row's text. Each row's text is kept as it now
    /// reads, and the count's label is written to its node as it now reads.
    /// Every frame formatted each row's location and its controls' names,
    /// cloned its path twice, and formatted the count.
    #[gpui::test]
    fn the_hub_builds_a_recent_row_s_text_only_when_its_project_changes(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let client = PathBuf::from("/projects/turtle-client");
        let core = PathBuf::from("/projects/turtle-core");
        let docs = PathBuf::from("/projects/turtle-docs");
        let recent = vec![client.clone(), core.clone(), docs.clone()];
        let names = HashMap::from([(client.clone(), "Shell".to_owned())]);
        let captured = Rc::new(RefCell::new(None));
        let observed = captured.clone();
        let built = || (RECENT_ROWS_BUILT.get(), RECENT_COUNTS_BUILT.get());
        let before = built();
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let hub = cx.new(|cx| ProjectHub::new(recent, names, "main".into(), window, cx));
            *captured.borrow_mut() = Some(hub.clone());
            gpui_kit::component::Root::new(hub, window, cx)
        });
        let hub = observed.borrow_mut().take().unwrap();
        cx.simulate_resize(size(px(1280.), px(800.)));
        let draw = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                window.refresh();
                window.draw(cx).clear(cx);
            });
            cx.run_until_parked();
        };
        let since = |before: (usize, usize)| {
            let after = built();
            (after.0 - before.0, after.1 - before.1)
        };
        let at_rest = |at: &str, rows: usize, cx: &mut VisualTestContext| {
            let before = built();
            RECENT_TEXT.set((0, 0));
            for _ in 0..3 {
                hub.update(cx, |_, cx| cx.notify());
                draw(cx);
            }
            let (lookups, allocations) = RECENT_TEXT.get();
            assert!(
                lookups >= 3 * (rows + 1),
                "{at}: each frame draws every row's text and the count: {lookups}"
            );
            assert_eq!(allocations, 0, "{at}: frames at rest allocate none of it");
            assert_eq!(
                since(before),
                (0, 0),
                "{at}: frames at rest build none of it"
            );
        };
        // A row's name, location and controls' names, and the count's label,
        // as drawn.
        let row = |path: &Path, cx: &mut VisualTestContext| {
            hub.read_with(cx, |hub, _| {
                let kept = hub.row_text.borrow();
                let text = kept.get(path).expect("the row's text");
                (
                    text.name.to_string(),
                    text.location.to_string(),
                    text.open.to_string(),
                    text.rename.to_string(),
                )
            })
        };
        // The count as drawn, and its label as its node was last written.
        let count = |cx: &mut VisualTestContext| {
            let drawn = hub.read_with(cx, |hub, _| {
                hub.count_text
                    .borrow()
                    .as_ref()
                    .map(|text| text.count.to_string())
            });
            let label = crate::native_accessibility::written::get("recent-projects-count")
                .and_then(|written| written.node.label().map(str::to_owned));
            (drawn, label)
        };
        let recent_label =
            |recent: usize| crate::counted(recent, "recent project", "recent projects");
        // Types `query` into the search, which narrows the list.
        let search = |query: &'static str, cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                hub.update(cx, |hub, cx| {
                    hub.search
                        .update(cx, |input, cx| input.set_value(query, window, cx));
                    hub.filter_recent(cx);
                })
            });
            draw(cx);
        };
        draw(cx);
        assert_eq!(since(before), (3, 1), "each row's text and the count once");
        assert_eq!(
            row(&client, cx),
            (
                "Shell".to_owned(),
                "turtle-client · /projects".to_owned(),
                "Open Shell, /projects/turtle-client".to_owned(),
                "Rename Shell, /projects/turtle-client".to_owned(),
            )
        );
        assert_eq!(row(&core, cx).1, "/projects", "a folder's own name");
        at_rest("three recent projects", 3, cx);

        let before = built();
        hub.update(cx, |hub, cx| {
            hub.set_names(
                HashMap::from([
                    (client.clone(), "Shell".to_owned()),
                    (core.clone(), "Engine".to_owned()),
                ]),
                cx,
            )
        });
        draw(cx);
        assert_eq!(since(before), (1, 0), "the renamed row's text once");
        assert_eq!(
            row(&core, cx),
            (
                "Engine".to_owned(),
                "turtle-core · /projects".to_owned(),
                "Open Engine, /projects/turtle-core".to_owned(),
                "Rename Engine, /projects/turtle-core".to_owned(),
            )
        );
        at_rest("a project renamed", 3, cx);

        let site = PathBuf::from("/projects/turtle-site");
        let before = built();
        hub.update(cx, |hub, cx| {
            hub.set_recent(
                vec![site.clone(), client.clone(), core.clone(), docs.clone()],
                cx,
            )
        });
        draw(cx);
        assert_eq!(
            since(before),
            (1, 1),
            "the new row's text and the count once"
        );
        assert_eq!(row(&site, cx).2, "Open turtle-site, /projects/turtle-site");
        assert_eq!(count(cx), (Some("4".to_owned()), Some(recent_label(4))));
        at_rest("a project opened", 4, cx);

        // One project dropped and another opened: the row built for the new
        // one leaves only the recent projects' text kept, and every row the
        // list draws is still drawn.
        let blog = PathBuf::from("/projects/turtle-blog");
        let shown = vec![blog.clone(), site.clone(), client.clone(), core.clone()];
        let before = built();
        hub.update(cx, |hub, cx| hub.set_recent(shown.clone(), cx));
        draw(cx);
        assert_eq!(since(before), (1, 0), "the new row's text once");
        let kept = hub.read_with(cx, |hub, _| {
            let mut kept: Vec<PathBuf> = hub.row_text.borrow().keys().cloned().collect();
            kept.sort();
            kept
        });
        let mut recent = shown.clone();
        recent.sort();
        assert_eq!(kept, recent, "the dropped project's text goes");
        let rows = [
            "recent-project-0",
            "recent-project-1",
            "recent-project-2",
            "recent-project-3",
        ];
        for (row, path) in rows.into_iter().zip(&shown) {
            assert!(
                cx.debug_bounds(row).is_some(),
                "the row of {path:?} is drawn"
            );
        }
        assert_eq!(row(&blog, cx).2, "Open turtle-blog, /projects/turtle-blog");
        at_rest("a project dropped", 4, cx);

        // A search that every project matches, one that narrows the list and
        // a cleared one each build the count and its label once, and no row.
        let matching = |matching: usize| {
            format!(
                "{} out of {}",
                crate::counted(matching, "matching project", "matching projects"),
                recent_label(4)
            )
        };
        for (query, rows, expected) in [
            ("turtle", 4, (Some("4 of 4".to_owned()), Some(matching(4)))),
            ("core", 1, (Some("1 of 4".to_owned()), Some(matching(1)))),
            ("", 4, (Some("4".to_owned()), Some(recent_label(4)))),
        ] {
            let before = built();
            search(query, cx);
            assert_eq!(since(before), (0, 1), "{query:?}: the count once");
            assert_eq!(count(cx), expected, "{query:?}");
            at_rest(query, rows, cx);
        }
    }

    #[test]
    fn a_submitted_rename_clears_the_name_for_empty_or_folder_text() {
        assert_eq!(chosen_name("  ", "turtle").unwrap(), None);
        assert_eq!(chosen_name("  turtle  ", "turtle").unwrap(), None);
        assert_eq!(
            chosen_name("  Turtle Client  ", "turtle").unwrap(),
            Some("Turtle Client".to_owned())
        );
        assert!(chosen_name("two\nlines", "turtle").is_err());
        assert!(chosen_name(&"x".repeat(129), "turtle").is_err());
    }

    #[gpui::test]
    fn renamed_projects_show_and_filter_by_their_chosen_name(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let client = PathBuf::from("/projects/turtle-client");
        let core = PathBuf::from("/projects/turtle-core");
        let recent = vec![client.clone(), core.clone()];
        let names = HashMap::from([(client.clone(), "Shell 客户端".to_owned())]);
        let (hub, cx) = cx.add_window_view(move |window, cx| {
            ProjectHub::new(recent, names, "main".into(), window, cx)
        });
        cx.update(|_, cx| {
            assert_eq!(hub.read(cx).display_name(&client), "Shell 客户端");
            assert_eq!(hub.read(cx).display_name(&core), "turtle-core");
        });

        // A chosen name is searchable; the folder name still finds the project.
        for (query, expected) in [
            ("客户端", vec![client.clone()]),
            ("shell", vec![client.clone()]),
            ("turtle-client", vec![client.clone()]),
            ("turtle-", vec![client.clone(), core.clone()]),
            ("core", vec![core.clone()]),
        ] {
            cx.update(|window, cx| {
                hub.update(cx, |hub, cx| {
                    hub.search
                        .update(cx, |input, cx| input.set_value(query, window, cx));
                    hub.filter_recent(cx);
                    assert_eq!(hub.filtered, expected, "{query}");
                });
            });
        }

        // Clearing the name returns the project to its folder name.
        cx.update(|_, cx| {
            hub.update(cx, |hub, cx| {
                hub.set_names(HashMap::new(), cx);
                assert_eq!(hub.display_name(&client), "turtle-client");
            });
        });
    }

    #[test]
    fn recent_projects_keep_order_without_duplicate_paths() {
        let recent = unique_recent(vec!["/tmp/b".into(), "/tmp/a".into(), "/tmp/b".into()]);
        assert_eq!(
            recent,
            vec![PathBuf::from("/tmp/b"), PathBuf::from("/tmp/a")]
        );
    }

    /// In two dark and two light palettes, two of them with a moved control
    /// label, every mode looks like History's segments, the shared helper.
    /// Selected, it rests on the palette's `selected`, hovers to its
    /// `selected_hover`, and keeps the whole Button, focus ring included, at
    /// full opacity. Unselected, it has no fill at rest, the palette's control
    /// fill of `hover` under the pointer and of `selected` while held pressed,
    /// rather than the ghost's translucent fills. While the hub is busy every
    /// mode keeps the ghost's disabled look and paints no fill, at rest, under
    /// the pointer or pressed. Only fills and rings are checked; the test
    /// platform paints no text, so a label's color goes unchecked.
    #[gpui::test]
    fn every_mode_rests_hovers_and_presses_like_the_shared_helper_unless_busy(
        cx: &mut TestAppContext,
    ) {
        use crate::appearance::{
            SelectedState, ThemeChoice, UnselectedState, assert_selected_button,
            assert_unselected_button,
        };
        fn draw(cx: &mut VisualTestContext) {
            cx.update(|window, cx| {
                window.simulate_next_frame(cx);
                window.draw(cx).clear(cx);
            });
            cx.run_until_parked();
        }
        cx.update(gpui_kit::init);
        let (hub, cx) = cx.add_window_view(|window, cx| {
            ProjectHub::new(vec![], HashMap::new(), "main".into(), window, cx)
        });
        draw(cx);
        // Tab from the mode list, which is no tab stop itself, passes Open to
        // reach Clone. Space selecting Clone shows that is where focus is.
        let clone = cx.update(|window, cx| {
            window.focus(&hub.read(cx).action_tabs_focus.clone(), cx);
            window.focus_next(cx);
            window.focus_next(cx);
            window.focused(cx).expect("Tab reaches a mode")
        });
        draw(cx);
        let space = Keystroke::parse("space").expect("a keystroke");
        cx.simulate_event(KeyDownEvent {
            keystroke: space.clone(),
            is_held: false,
            prefer_character_input: false,
        });
        cx.simulate_event(KeyUpEvent { keystroke: space });
        draw(cx);
        assert!(
            cx.read(|cx| hub.read(cx).mode == ProjectMode::Clone),
            "Tab from the mode list reaches Clone, and Space selects it"
        );
        for choice in [
            ThemeChoice::Midnight,
            ThemeChoice::Porcelain,
            ThemeChoice::KanagawaLotus,
            ThemeChoice::OneDark,
        ] {
            cx.update(|window, cx| {
                choice.apply(Some(window), cx);
                window.blur(cx);
            });
            cx.simulate_mouse_move(point(px(0.), px(0.)), None, Modifiers::default());
            draw(cx);
            let segment = cx
                .debug_bounds("project-mode-Clone")
                .expect("rendered Clone mode");
            let other = cx
                .debug_bounds("project-mode-Open")
                .expect("rendered Open mode");
            let name = format!("{choice:?} Clone mode");
            let other_name = format!("{choice:?} Open mode");
            let away = point(px(0.), px(0.));
            assert_selected_button(cx, &name, segment, SelectedState::Resting);
            assert_unselected_button(cx, &other_name, other, UnselectedState::Resting);
            cx.simulate_mouse_move(other.center(), None, Modifiers::default());
            draw(cx);
            assert_unselected_button(cx, &other_name, other, UnselectedState::Hovered);
            cx.simulate_mouse_down(other.center(), MouseButton::Left, Modifiers::default());
            draw(cx);
            assert_unselected_button(cx, &other_name, other, UnselectedState::Pressed);
            // Released off the segment, the press selects nothing.
            cx.simulate_mouse_move(away, Some(MouseButton::Left), Modifiers::default());
            cx.simulate_mouse_up(away, MouseButton::Left, Modifiers::default());
            draw(cx);
            assert!(
                cx.read(|cx| hub.read(cx).mode == ProjectMode::Clone),
                "{other_name} released off the segment selects nothing"
            );
            cx.simulate_mouse_move(segment.center(), None, Modifiers::default());
            draw(cx);
            assert_selected_button(cx, &name, segment, SelectedState::Hovered);
            cx.update(|window, cx| window.focus(&clone, cx));
            draw(cx);
            assert!(
                cx.update(|window, _| clone.is_focused(window)),
                "{name} takes focus back"
            );
            assert_selected_button(cx, &name, segment, SelectedState::FocusedAndHovered);
            // While the hub is busy every mode keeps the ghost's disabled
            // look, transparent at rest, under the pointer and pressed.
            let busy = |cx: &mut VisualTestContext, busy| {
                cx.update(|window, cx| {
                    hub.update(cx, |hub, cx| {
                        hub.busy = busy;
                        cx.notify();
                    });
                    window.blur(cx);
                })
            };
            busy(cx, true);
            for (mode, bounds) in [(&name, segment), (&other_name, other)] {
                for position in [away, bounds.center()] {
                    cx.simulate_mouse_move(position, None, Modifiers::default());
                    draw(cx);
                    let (fills, _) = crate::appearance::painted_button(cx, bounds);
                    assert!(fills.is_empty(), "{mode} busy paints {fills:?}");
                }
                cx.simulate_mouse_down(bounds.center(), MouseButton::Left, Modifiers::default());
                draw(cx);
                let (fills, _) = crate::appearance::painted_button(cx, bounds);
                assert!(fills.is_empty(), "{mode} busy pressed paints {fills:?}");
                cx.simulate_mouse_move(away, Some(MouseButton::Left), Modifiers::default());
                cx.simulate_mouse_up(away, MouseButton::Left, Modifiers::default());
                draw(cx);
            }
            busy(cx, false);
        }
    }

    /// An Open the worker drops unanswered, with no newer read in its
    /// place, fails as an interrupted read does (`GitTurtle::request`): the
    /// window returns to the hub, which is no longer busy, so its actions
    /// are available again, and says why, announced at once, as the window
    /// does. Ended as any other dropped read is, it left the window on the
    /// repository page with nothing read and nothing to say why, and the
    /// hub busy behind it, its actions refused.
    #[gpui::test]
    async fn an_open_dropped_unanswered_fails_as_interrupted(cx: &mut TestAppContext) {
        use crate::native_accessibility::written;
        let (fixture, app, cx) = crate::views::tests::history_window(cx).await;
        let other = crate::views::tests::fixture_repository(fixture.path().join("other"));
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_projects(window, cx)));
        crate::views::tests::draw(cx);
        let hub = app.read_with(cx, |app, _| app.hub.clone());
        let reader = app.read_with(cx, |app, _| app.worker.hold());
        hub.update(cx, |hub, cx| {
            hub.emit_operation(ProjectEvent::Open(other.path().to_owned()), cx)
        });
        cx.executor().run_until_parked();
        assert!(
            app.read_with(cx, |app, _| app.loading.is_some())
                && hub.read_with(cx, |hub, _| hub.busy),
            "the Open waits on the held reader, the hub busy"
        );
        // Dropped unanswered, as a cancel that leaves the read's generation
        // as it was drops it.
        app.read_with(cx, |app, _| app.worker.cancel());
        drop(reader);
        cx.executor().run_until_parked();
        written::forget();
        crate::views::tests::draw(cx);
        assert!(
            app.read_with(cx, |app, _| app.loading.is_none()),
            "the Open ended"
        );
        assert!(
            !hub.read_with(cx, |hub, _| hub.busy),
            "the hub is no longer busy"
        );
        assert!(
            app.read_with(cx, |app, _| app.page == crate::AppPage::Projects),
            "the window is back on the hub"
        );
        let error = hub.read_with(cx, |hub, _| hub.error.clone());
        assert_eq!(
            error.as_deref(),
            Some("Repository read was interrupted. Open the repository again to retry."),
            "the hub says why"
        );
        assert_eq!(
            app.read_with(cx, |app, _| app.error.clone()),
            error,
            "and so does the window"
        );
        let announced = written::announced_from("project-hub-error");
        assert!(
            announced.len() == 1 && announced[0].1 == written::Announced::ASSERTIVE,
            "announced at once: {announced:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn chosen_destination_preserves_non_utf8_parent_bytes() {
        use std::os::unix::ffi::OsStringExt;
        let parent = PathBuf::from(std::ffi::OsString::from_vec(b"/tmp/project-\xff".to_vec()));
        assert_eq!(
            validate_destination(&parent, "child").unwrap(),
            parent.join("child")
        );
    }

    /// A mark restated in the same words is no new line: the feedback line
    /// keeps its node, so it is not announced or revealed again and a recent
    /// row's refusal kept by that line still matches. A new reason is a
    /// line of its own.
    #[gpui::test]
    fn an_unchanged_restatement_leaves_the_feedback_line_alone(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let captured = Rc::new(RefCell::new(None));
        let observed = captured.clone();
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let hub =
                cx.new(|cx| ProjectHub::new(Vec::new(), HashMap::new(), "main".into(), window, cx));
            *captured.borrow_mut() = Some(hub.clone());
            gpui_kit::component::Root::new(hub, window, cx)
        });
        let hub = observed.borrow_mut().take().unwrap();
        let line = |cx: &mut VisualTestContext| {
            hub.read_with(cx, |hub, _| (hub.errors_shown, hub.error.clone()))
        };
        let replace = |previous: &'static str, error: &'static str, cx: &mut VisualTestContext| {
            cx.update(|_, cx| {
                hub.update(cx, |hub, cx| {
                    hub.replace_error_matching(previous, error.into(), cx)
                })
            });
        };
        cx.update(|_, cx| hub.update(cx, |hub, cx| hub.set_error(Some("the reason".into()), cx)));
        let shown = line(cx);
        assert_eq!(shown.1.as_deref(), Some("the reason"));

        replace("the reason", "the reason", cx);
        assert_eq!(line(cx), shown, "the same words are no new line");

        replace("the reason", "a new reason", cx);
        let new = line(cx);
        assert_eq!(new.0, shown.0.wrapping_add(1), "a line of its own");
        assert_eq!(new.1.as_deref(), Some("a new reason"));
    }

    /// One key event as the platform delivers it, with no idle pass after
    /// it: `stroke` going down, marked held or not, or with `None` its key-up.
    fn key(cx: &mut VisualTestContext, stroke: &str, down: Option<bool>) {
        let keystroke = Keystroke::parse(stroke).expect("a keystroke");
        let input = match down {
            Some(is_held) => PlatformInput::KeyDown(KeyDownEvent {
                keystroke,
                is_held,
                prefer_character_input: false,
            }),
            None => PlatformInput::KeyUp(KeyUpEvent { keystroke }),
        };
        cx.update(|window, cx| window.dispatch_event(input, cx));
    }

    /// One Enter key event, as [`key`] sends it.
    fn enter(cx: &mut VisualTestContext, down: Option<bool>) {
        key(cx, "enter", down);
    }

    type Field = fn(&ProjectHub) -> &Entity<InputState>;

    /// A hub with the Clone and Create fields filled, whose owner answers
    /// each clone or create as the hub asks for it, with a failure, as a
    /// clone whose remote refuses at once does, so the hub is ready again
    /// before the next key event.
    struct FailingHub {
        hub: Entity<ProjectHub>,
        attempts: Rc<Cell<usize>>,
        _failing: Subscription,
    }

    impl FailingHub {
        const SOURCE: Field = |hub| &hub.source;
        const PARENT: Field = |hub| &hub.parent;
        const NAME: Field = |hub| &hub.name;
        const BRANCH: Field = |hub| &hub.branch;

        fn new(cx: &mut TestAppContext) -> (Self, &mut VisualTestContext) {
            cx.update(gpui_kit::init);
            let captured = Rc::new(RefCell::new(None));
            let observed = captured.clone();
            let (_, cx) = cx.add_window_view(move |window, cx| {
                let hub =
                    cx.new(|cx| ProjectHub::new(vec![], HashMap::new(), "main".into(), window, cx));
                *captured.borrow_mut() = Some(hub.clone());
                gpui_kit::component::Root::new(hub, window, cx)
            });
            let hub = observed.borrow_mut().take().unwrap();
            cx.simulate_resize(size(px(1000.), px(680.)));
            let attempts = Rc::new(Cell::new(0_usize));
            let failing = cx.update(|_, cx| {
                let attempts = attempts.clone();
                cx.subscribe(&hub, move |hub, event: &ProjectEvent, cx| {
                    if matches!(
                        event,
                        ProjectEvent::Clone { .. } | ProjectEvent::Create { .. }
                    ) {
                        attempts.set(attempts.get() + 1);
                        hub.update(cx, |hub, cx| {
                            hub.set_busy(false, cx);
                            hub.set_error(Some("Could not read from the remote.".into()), cx);
                        });
                    }
                })
            });
            let this = Self {
                hub,
                attempts,
                _failing: failing,
            };
            this.set(cx, Self::SOURCE, "https://example.invalid/demo.git");
            this.set(cx, Self::NAME, "demo");
            this.set(cx, Self::PARENT, "/srv/projects");
            (this, cx)
        }

        /// The clones or creates asked for, and the refusals said.
        fn seen(&self, cx: &mut VisualTestContext) -> (usize, usize) {
            let refusals = self.hub.read_with(cx, |hub, _| hub.errors_shown);
            (self.attempts.get(), refusals)
        }

        fn set(&self, cx: &mut VisualTestContext, field: Field, value: &str) {
            let value = value.to_owned();
            cx.update(|window, cx| {
                let input = field(self.hub.read(cx)).clone();
                input.update(cx, |input, cx| input.set_value(value, window, cx));
            });
        }

        /// Shows `mode` with focus in `field`.
        fn focus(&self, cx: &mut VisualTestContext, mode: ProjectMode, field: Field) {
            cx.update(|window, cx| {
                self.hub
                    .update(cx, |hub, cx| hub.change_mode(mode, window, cx));
                let focus = field(self.hub.read(cx)).focus_handle(cx);
                window.focus(&focus, cx);
            });
            crate::tags::tests::draw(cx);
        }

        fn focused(&self, cx: &mut VisualTestContext, field: Field) -> bool {
            cx.update(|window, cx| field(self.hub.read(cx)).focus_handle(cx).is_focused(window))
        }
    }

    /// A held Return in a Clone or Create field asks for one clone or
    /// create, under X11 too, in each of the forms' fields: with every
    /// attempt failing at once, as a clone whose remote refuses at once
    /// does, Enter goes down and the attempt fails, then the key's repeats
    /// arrive as X11 sends them, fresh presses with no key-up between them
    /// and a release and its press together, and as GPUI marks them held on
    /// macOS and Wayland. The hub asks once and says the failure once; after
    /// the key-up a new press asks again. A form the hub refuses, its source
    /// left empty, says why once per press the same way. The fields
    /// submitted on every key-down, guarded only by the busy state the
    /// failure had cleared, so each repeat began another clone, a network
    /// action, and said its failure again.
    #[gpui::test]
    fn a_held_return_asks_for_one_clone_or_create(cx: &mut TestAppContext) {
        let (hub, cx) = FailingHub::new(cx);
        let cases: [(ProjectMode, &str, Field, (usize, usize)); 5] = [
            (ProjectMode::Clone, "the source", FailingHub::SOURCE, (1, 1)),
            (
                ProjectMode::Clone,
                "the parent folder",
                FailingHub::PARENT,
                (1, 1),
            ),
            (
                ProjectMode::Create,
                "the folder name",
                FailingHub::NAME,
                (1, 1),
            ),
            (
                ProjectMode::Create,
                "the initial branch",
                FailingHub::BRANCH,
                (1, 1),
            ),
            // Refused before the hub asks: an empty source.
            (
                ProjectMode::Clone,
                "an empty source",
                FailingHub::SOURCE,
                (0, 1),
            ),
        ];
        for (mode, at, field, step) in cases {
            if at == "an empty source" {
                hub.set(cx, FailingHub::SOURCE, "");
            }
            hub.focus(cx, mode, field);
            let before = hub.seen(cx);
            let after = (before.0 + step.0, before.1 + step.1);
            enter(cx, Some(false));
            crate::tags::tests::draw(cx);
            assert_eq!(hub.seen(cx), after, "{at}: the press answers once");
            let repeats: [(&str, &[Option<bool>]); 4] = [
                ("an X11 repeat", &[Some(false)]),
                ("another X11 repeat", &[Some(false)]),
                // A release and its press together, with no idle pass between.
                ("a release and its press", &[None, Some(false)]),
                ("a held repeat", &[Some(true)]),
            ];
            for (repeat, events) in repeats {
                for down in events {
                    enter(cx, *down);
                }
                crate::tags::tests::draw(cx);
                assert_eq!(
                    hub.seen(cx),
                    after,
                    "{at}, {repeat}: nothing more is asked or said"
                );
                assert!(
                    hub.focused(cx, field),
                    "{at}, {repeat}: focus stays in the field"
                );
            }
            enter(cx, None);
            crate::tags::tests::draw(cx);
            enter(cx, Some(false));
            crate::tags::tests::draw(cx);
            assert_eq!(
                hub.seen(cx),
                (after.0 + step.0, after.1 + step.1),
                "{at}: a new press after the key-up answers again"
            );
            enter(cx, None);
            crate::tags::tests::draw(cx);
        }
    }

    /// A Return the hub's window never saw go down asks for nothing as its
    /// held repeats arrive, marked held, as on macOS and in GPUI's own
    /// Wayland repeat: one an input method took to commit a composition in
    /// the folder name, and one that confirmed the native folder picker
    /// from Browse, whose answer focuses the folder name. After the key-up
    /// a new press asks once. The fields submitted on their own
    /// `PressEnter`, which carries no held mark, so the first repeat, the
    /// first Enter the window saw, looked like a fresh press and asked for
    /// one clone (security review of FU41-B5, cases E1 and F2).
    #[gpui::test]
    fn held_repeats_of_a_return_the_hub_never_saw_ask_for_nothing(cx: &mut TestAppContext) {
        let (hub, cx) = FailingHub::new(cx);
        for case in ["an input method's commit", "the folder picker's answer"] {
            if case == "the folder picker's answer" {
                hub.focus(cx, ProjectMode::Clone, FailingHub::PARENT);
                // Browse; Return goes down in the picker, another window.
                cx.update(|window, cx| {
                    hub.hub
                        .update(cx, |hub, cx| hub.choose_folder(false, window, cx))
                });
                cx.simulate_path_prompt_response(|_| Some(vec![PathBuf::from("/srv/picked")]));
                crate::tags::tests::draw(cx);
                assert!(
                    hub.focused(cx, FailingHub::NAME),
                    "{case}: the folder name is focused"
                );
            } else {
                // The press that commits the composition reaches the input
                // method only.
                hub.focus(cx, ProjectMode::Clone, FailingHub::NAME);
            }
            let before = hub.seen(cx);
            for repeat in 0..3 {
                enter(cx, Some(true));
                crate::tags::tests::draw(cx);
                assert_eq!(
                    hub.seen(cx),
                    before,
                    "{case}, held repeat {repeat}: nothing is asked or said"
                );
            }
            enter(cx, None);
            crate::tags::tests::draw(cx);
            enter(cx, Some(false));
            crate::tags::tests::draw(cx);
            assert_eq!(
                hub.seen(cx),
                (before.0 + 1, before.1 + 1),
                "{case}: a new press after the key-up asks once"
            );
            enter(cx, None);
            crate::tags::tests::draw(cx);
        }
    }

    /// Shift-Return in a Clone field asks once per press, as Return does,
    /// and Control-Return, the secondary Enter off macOS, asks nothing; the
    /// held repeats of neither ask anything more, as X11 sends them and as
    /// GPUI marks them held.
    #[gpui::test]
    fn shift_return_asks_once_and_control_return_asks_nothing(cx: &mut TestAppContext) {
        let (hub, cx) = FailingHub::new(cx);
        hub.focus(cx, ProjectMode::Clone, FailingHub::SOURCE);
        for (stroke, step) in [("ctrl-enter", (0, 0)), ("shift-enter", (1, 1))] {
            let before = hub.seen(cx);
            let after = (before.0 + step.0, before.1 + step.1);
            key(cx, stroke, Some(false));
            crate::tags::tests::draw(cx);
            assert_eq!(hub.seen(cx), after, "{stroke}: the press");
            for held in [false, false, true] {
                key(cx, stroke, Some(held));
                crate::tags::tests::draw(cx);
                assert_eq!(
                    hub.seen(cx),
                    after,
                    "{stroke}: a repeat (held: {held}) asks nothing more"
                );
            }
            key(cx, stroke, None);
            crate::tags::tests::draw(cx);
        }
    }

    /// A press the hub refuses leaves the next press free to ask: with the
    /// source refused (a leading dash), Return says why once, the source is
    /// fixed while the key is still down, as a paste with the pointer does,
    /// and the key's repeats ask nothing; after the key-up a new press asks
    /// for one clone.
    #[gpui::test]
    fn a_fresh_press_after_a_refused_one_asks_once(cx: &mut TestAppContext) {
        let (hub, cx) = FailingHub::new(cx);
        hub.focus(cx, ProjectMode::Clone, FailingHub::SOURCE);
        hub.set(cx, FailingHub::SOURCE, "-oops");
        crate::tags::tests::draw(cx);
        enter(cx, Some(false));
        crate::tags::tests::draw(cx);
        assert_eq!(hub.seen(cx), (0, 1), "the refused press says why once");
        hub.set(cx, FailingHub::SOURCE, "https://example.invalid/demo.git");
        crate::tags::tests::draw(cx);
        // An X11 repeat, a release and its press together, and a held repeat.
        let repeats: [&[Option<bool>]; 3] = [&[Some(false)], &[None, Some(false)], &[Some(true)]];
        for events in repeats {
            for down in events {
                enter(cx, *down);
            }
            crate::tags::tests::draw(cx);
            assert_eq!(hub.seen(cx), (0, 1), "{events:?}: the repeat asks nothing");
        }
        enter(cx, None);
        crate::tags::tests::draw(cx);
        enter(cx, Some(false));
        crate::tags::tests::draw(cx);
        assert_eq!(hub.seen(cx), (1, 2), "a fresh press asks once");
        enter(cx, None);
        crate::tags::tests::draw(cx);
    }
}

#[cfg(test)]
mod rename_tests;
