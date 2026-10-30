//! A local reflog is an expiring Git record, separate from app activity.
use crate::*;
use focus_reveal::{FocusReveal, Trigger};
use gitturtle_core::{ReflogEntry, ReflogPage, ReflogRecoveryPlan, WriteCommand};
use gpui_kit::{
    base::ElementExt,
    component::{
        WindowExt,
        dialog::DialogButtonProps,
        scroll::{Scrollbar, ScrollbarMode},
    },
    prelude::FluentBuilder,
};

/// The gap the kit's dialog leaves between its body and its footer, its
/// default 16 px padding.
const DIALOG_FOOTER_GAP: Pixels = px(16.);

/// The text size of a changed-file row at the default interface size.
const FILE_ROW_TEXT: f32 = 12.;
/// A changed-file row's line height in ems, the 1.618 (GPUI's `phi`) the rows
/// inherited before they set their own.
const FILE_ROW_LINE: f32 = std::f32::consts::GOLDEN_RATIO;
/// The changed-file list's height bound at the default interface size.
const FILE_LIST_BOUND: f32 = 110.;

/// A changed-file row's height, its own line height: one line of
/// [`FILE_ROW_LINE`] at every text size, rounded to whole pixels so that GPUI's
/// pixel snapping keeps every row, and the list's bound, a whole multiple.
fn file_row_height() -> Pixels {
    (crate::appearance::ui_text(FILE_ROW_TEXT) * FILE_ROW_LINE).round()
}

/// How many whole changed-file rows the list shows before it scrolls: as many
/// as fit in [`FILE_LIST_BOUND`] at the default text size, kept at every text
/// size, so its last row is never cut through.
fn file_rows_shown() -> usize {
    (FILE_LIST_BOUND / (FILE_ROW_TEXT * FILE_ROW_LINE).round()).floor() as usize
}

/// Where the dialog's text starts, in from its content edge: the kit's
/// medium Input pads its text 10 px inside its 1 px border at every text
/// size, so the title, the explanation and the other labels, the entries'
/// text and the changed files start where the scope and filter fields'
/// text and placeholders do, one inset for all.
const TEXT_INSET: Pixels = appearance::FIELD_TEXT_INSET;

/// The entries whose text the last layout cut, which alone carry a tooltip.
#[derive(Clone, Default)]
struct CutEntries(std::rc::Rc<std::cell::RefCell<HashSet<usize>>>);

/// One Reflog entry. A selected entry shows its selection through the
/// selected surface as well as its selected and toggled state, as the
/// managed worktree rows do.
///
/// Its text starts at [`TEXT_INSET`] and ends in an ellipsis where the entry
/// is too narrow, rather than centred as a kit label is: a centred line
/// wider than its entry would start left of it. The accessible name keeps
/// the whole text, and so does a tooltip, only while the entry cuts it: the
/// text's measured width beyond its room, as `cut` recorded at the last
/// layout. Over a whole line a tooltip only covered the entry above.
fn entry_button(
    index: usize,
    text: String,
    selected: bool,
    cut: &CutEntries,
    browser: WeakEntity<ReflogBrowser>,
    p: appearance::Palette,
) -> Button {
    let shown_cut = cut.0.borrow().contains(&index);
    let record = cut.clone();
    let measured = text.clone();
    Button::new(("reflog-entry", index))
        .debug_selector(move || format!("reflog-entry-{index}"))
        .ghost()
        .w_full()
        .h_auto()
        .min_h(crate::appearance::ui_size(36.))
        .pl(TEXT_INSET)
        .child(
            div()
                .debug_selector(move || format!("reflog-entry-text-{index}"))
                .flex_1()
                .min_w_0()
                .whitespace_nowrap()
                .text_ellipsis()
                .line_height(relative(1.))
                .child(text.clone())
                .on_prepaint(move |room, window, cx| {
                    let style = window.text_style();
                    let size = style.font_size.to_pixels(window.rem_size());
                    let run = style.to_run(measured.len());
                    let width = window
                        .text_system()
                        .shape_line(measured.into(), size, &[run], None)
                        .width();
                    let mut entries = record.0.borrow_mut();
                    let changed = if width > room.size.width {
                        entries.insert(index)
                    } else {
                        entries.remove(&index)
                    };
                    drop(entries);
                    if changed {
                        let _ = browser.update(cx, |_, cx| cx.notify());
                    }
                }),
        )
        .when(shown_cut, |entry| entry.tooltip(text.clone()))
        .accessibility_label(text)
        .selected(selected)
        .toggled(selected)
        .when(selected, |entry| {
            entry
                .bg(rgb(p.selected))
                .hover(|entry| entry.bg(rgb(p.row_hover(true))))
        })
}

/// [`native_accessibility::text`] at the dialog's text inset.
fn label(
    id: &'static str,
    value: impl Into<SharedString>,
) -> native_accessibility::StaticText<Stateful<Div>> {
    native_accessibility::text(id, value).ml(TEXT_INSET)
}

impl GitTurtle {
    /// Opens the browser in its dialog and starts reading HEAD's log on the
    /// browser's own thread. Returns the browser, unless an operation is
    /// running or no repository is open.
    pub(super) fn open_reflog_browser(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<ReflogBrowser>> {
        if self.operation_busy.is_some() {
            return None;
        }
        let repo = self.repository.clone()?;
        let owner = cx.entity().downgrade();
        let browser = cx.new(|cx| ReflogBrowser::new(owner, repo, window, cx));
        browser.update(cx, |this, cx| this.refresh(window, cx));
        ReflogBrowser::show(browser.clone(), window, cx);
        Some(browser)
    }
}

/// The focus sweep (`focus_ring::sweep`) walks the Reflog, and the layout
/// grid measures it, once its log has landed. The read runs on the browser's
/// own thread, which `run_until_parked` cannot wait for: a walk that started
/// while it ran found the loading row's Cancel focused, and the reply then
/// took that row away, leaving focus on a control no frame drew.
#[cfg(test)]
impl crate::focus_ring::sweep::Opened for Option<Entity<ReflogBrowser>> {
    fn finish(self, cx: &mut VisualTestContext) {
        let Some(browser) = self else {
            return;
        };
        // Every read the browser submitted has answered once its thread
        // drains, and the replies land on this thread when it next runs.
        browser.read_with(cx, |browser, _| browser.reader.drain());
        cx.executor().run_until_parked();
        assert!(
            browser.read_with(cx, |browser, _| !browser.pending),
            "the Reflog's read lands"
        );
    }
}
#[cfg(test)]
impl ReflogBrowser {
    /// Opens `app`'s Reflog, reads its log, selects its newest entry and
    /// reads that commit, for tests in other modules.
    pub(crate) async fn open_selected(
        app: &Entity<GitTurtle>,
        cx: &mut gpui::VisualTestContext,
    ) -> Entity<Self> {
        let browser = cx
            .update(|window, cx| app.update(cx, |app, cx| app.open_reflog_browser(window, cx)))
            .expect("the Reflog opens");
        let read = browser.update(cx, |this, _| this.task.take());
        read.expect("the log is read").await;
        let entry = browser.read_with(cx, |this, _| {
            let page = this.page.as_ref().expect("the log loaded");
            page.entries.first().expect("the log has an entry").clone()
        });
        cx.update(|window, cx| browser.update(cx, |this, cx| this.select(entry, window, cx)));
        let read = browser.update(cx, |this, _| this.task.take());
        read.expect("the commit is read").await;
        cx.executor().run_until_parked();
        browser
    }

    /// Whether the browser started a read: one yet to answer, or one that
    /// answered with a failure, as a review read while a write runs does.
    /// Its reader is drained first, so a read that was started has answered.
    pub(crate) fn started_a_read(&self) -> bool {
        self.reader.drain();
        self.pending || self.task.is_some() || self.error.is_some()
    }
}

impl ReflogBrowser {
    /// The filter's focus handle, for the focus sweep.
    #[cfg(test)]
    pub(crate) fn filter_focus(&self, cx: &App) -> FocusHandle {
        self.query.read(cx).focus_handle(cx)
    }
    /// Opens `browser` in its dialog, with focus in its filter field.
    fn show(browser: Entity<Self>, window: &mut Window, cx: &mut App) {
        let filter = browser.read(cx).query.read(cx).focus_handle(cx);
        browser.update(cx, |browser, _| browser.reveal_filter = true);
        window.open_alert_dialog(cx, move |dialog, window, cx| {
            let done = browser.clone();
            let cancel = browser.clone();
            let closing = browser.downgrade();
            let closed = browser.read(cx).closed.clone();
            // The dialog clips its body to the body's bounds, and the browser
            // keeps the focus ring's room inside them below its last control.
            // The footer's gap gives the room back, so the footer stays put.
            dialog
                .title(
                    native_accessibility::dialog_title("reflog-title", "Local reflog and recovery")
                        .ml(TEXT_INSET),
                )
                .width(appearance::dialog_width(px(740.), window))
                .max_h(appearance::dialog_max_height(window))
                .gap(DIALOG_FOOTER_GAP - appearance::button_ring_room(cx))
                .child(browser.clone())
                .button_props(DialogButtonProps::default().ok_text("Done"))
                .on_ok(move |_, _, cx| {
                    done.update(cx, |this, _| this.cancel());
                    true
                })
                .on_cancel(move |_, _, cx| {
                    cancel.update(cx, |this, _| this.cancel());
                    true
                })
                .on_close(move |_, _, cx| {
                    closed.store(true, std::sync::atomic::Ordering::Release);
                    let closing = closing.clone();
                    cx.defer(move |cx| {
                        let _ = closing.update(cx, |this, _| this.cancel());
                    });
                })
        });
        // Opening the dialog focuses its shell, which draws no ring, so the
        // dialog opened with focus nowhere to be seen. The filter is the
        // first stop worth a key: focus it now, as the palette does its
        // query, so keys typed before the next frame reach it and the first
        // frame draws its ring, scrolled into the content's view where the
        // explanation above it fills the content, as at 18 pt in a 461 × 490
        // window. The dialog keeps the opener to return to.
        filter.focus(window, cx);
        window.refresh();
    }
}

/// A read asked for while another is yet to answer, which submits it.
type WaitingRead = Box<dyn FnOnce(&mut ReflogBrowser, &mut Window, &mut Context<ReflogBrowser>)>;

pub(crate) struct ReflogBrowser {
    closed: Arc<std::sync::atomic::AtomicBool>,
    owner: WeakEntity<GitTurtle>,
    repo: GitRepository,
    reader: operations::SerialExecutor,
    task: Option<Task<()>>,
    /// Whether a read submitted to `reader` is yet to answer: the browser
    /// submits one at a time ([`Self::read`]).
    reading: bool,
    /// The latest read asked for while another was yet to answer, which
    /// starts once that one answers.
    waiting: Option<WaitingRead>,
    cancellation: gitturtle_core::HistoryCancellation,
    scope: Entity<InputState>,
    query: Entity<InputState>,
    name: Entity<InputState>,
    page: Option<ReflogPage>,
    selected: Option<ReflogEntry>,
    commit: Option<Commit>,
    changes: Vec<FileChange>,
    message: Option<Entity<EditorState>>,
    /// The content's scroll, which its scrollbar follows and which reveals
    /// the control the keyboard moves focus onto, the entry list as a whole
    /// for an entry.
    content: FocusReveal,
    /// The entry list's scroll, which its scroll cue follows and which
    /// reveals the entry the keyboard moves focus onto.
    entries: FocusReveal,
    /// Whether the next frame scrolls the content to show the filter, if it
    /// holds focus: set as the dialog opens and as each read lands, since
    /// the scope's summary that a read adds above the filter moves it down.
    reveal_filter: bool,
    /// The changed-file list's scroll, which its scrollbar follows.
    files_scroll: ScrollHandle,
    /// The entries whose text the last layout cut.
    cut_entries: CutEntries,
    pending: bool,
    /// The loading row's Cancel, which hands focus to the filter as the row
    /// goes.
    cancel_read: focus_ring::LoadingCancel,
    error: Option<String>,
    _subscriptions: Vec<Subscription>,
}
impl Drop for ReflogBrowser {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}
impl ReflogBrowser {
    fn new(
        owner: WeakEntity<GitTurtle>,
        repo: GitRepository,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let scope = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value("HEAD")
                .placeholder("HEAD or local branch name")
        });
        let query = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Filter log by message, object ID, or selector")
        });
        let scope_subscription =
            cx.subscribe_in(&scope, window, |this: &mut Self, _, event, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.refresh(window, cx);
                }
                cx.notify();
            });
        let query_subscription =
            cx.subscribe_in(&query, window, |this: &mut Self, _, event, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. })
                    && let Some(entry) = this.matches(cx).first()
                {
                    this.select(entry.clone(), window, cx);
                }
                cx.notify();
            });
        Self {
            closed: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            owner,
            repo,
            reader: operations::SerialExecutor::new("reflog-browser"),
            task: None,
            reading: false,
            waiting: None,
            cancellation: Default::default(),
            scope,
            query,
            name: cx.new(|cx| InputState::new(window, cx).placeholder("recovery/my-commit")),
            page: None,
            selected: None,
            commit: None,
            changes: Vec::new(),
            message: None,
            content: FocusReveal::new(Trigger::Keyboard),
            entries: FocusReveal::new(Trigger::Keyboard),
            reveal_filter: false,
            files_scroll: ScrollHandle::new(),
            cut_entries: CutEntries::default(),
            pending: false,
            cancel_read: focus_ring::LoadingCancel::new(cx),
            error: None,
            _subscriptions: vec![scope_subscription, query_subscription],
        }
    }
    /// Cancels the read in progress and drops its reply, and any read
    /// waiting to start after it.
    fn cancel(&mut self) {
        self.cancellation.cancel();
        self.task = None;
        self.reading = false;
        self.waiting = None;
        self.pending = false;
    }
    /// As the loading row goes, focus its Cancel holds moves to the stop the
    /// row's place leaves nearest: a selected entry's read stands in the
    /// details below the list ([`Self::reading_entry`]), so the entry, the
    /// list's stop above it; any other read stands above the list, below the
    /// filter, the first stop below it that every state of the browser draws.
    fn leave_loading_row(&self, window: &mut Window, cx: &mut App) {
        let entry = self
            .selected
            .as_ref()
            .filter(|_| self.commit.is_none())
            .and_then(|entry| self.matches(cx).iter().take(100).position(|e| e == entry))
            .map(|index| self.entries.focus(("reflog-entry", index), cx));
        let to = entry.unwrap_or_else(|| self.query.read(cx).focus_handle(cx));
        self.cancel_read.release_to(&to, window, cx);
    }
    /// Whether the selected entry's commit is yet to land: its read's loading
    /// row, and any error that read ends with, stand in the details below the
    /// list, where the commit lands.
    fn awaiting_entry(&self) -> bool {
        self.selected.is_some() && self.commit.is_none()
    }
    /// Whether the read in progress is the selected entry's, whose commit
    /// lands in the details below the list.
    fn reading_entry(&self) -> bool {
        self.pending && self.awaiting_entry()
    }
    /// Up, Down, Home and End on the entry list select the entry they reach
    /// and move focus, the list's one Tab stop, onto it, as History's list
    /// and the Stashes list select with them; Space and Enter on the entry
    /// select it as a click does.
    fn entry_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.modifiers.modified() {
            return;
        }
        let matches = self.matches(cx);
        let count = matches.len().min(100);
        let Some(current) = (0..count).find(|&index| {
            self.entries
                .focus(("reflog-entry", index), cx)
                .is_focused(window)
        }) else {
            return;
        };
        let target = match event.keystroke.key.as_str() {
            "up" => current.saturating_sub(1),
            "down" => (current + 1).min(count - 1),
            "home" => 0,
            "end" => count - 1,
            _ => return,
        };
        cx.stop_propagation();
        let focus = self.entries.focus(("reflog-entry", target), cx);
        window.focus(&focus, cx);
        if self.selected.as_ref() != Some(&matches[target]) {
            self.select(matches[target].clone(), window, cx);
        }
        cx.notify();
    }
    /// Whether the browser may act now: not while a write runs.
    fn current(&self, cx: &App) -> bool {
        self.owner.upgrade().is_some_and(|owner| {
            let owner = owner.read(cx);
            owner.path.as_deref() == Some(self.repo.path()) && owner.operation_busy.is_none()
        })
    }
    /// Asks the browser's reader for a read, one at a time: while an earlier
    /// read is yet to answer, this one cancels it, so its Git process stops,
    /// and waits in place of any read already waiting, to start once the
    /// earlier one answers, whose reply then lands nowhere. Each arrow press
    /// selects an entry and asks for its read, and the reader's queue holds
    /// eight, so a key held while a read was slow filled it and the presses
    /// after that failed at once with "Background executor is busy…".
    fn read<T: Send + 'static>(
        &mut self,
        operation: impl FnOnce(GitRepository) -> anyhow::Result<T> + Send + 'static,
        receive: impl FnOnce(&mut Self, T, &mut Window, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cancellation.cancel();
        self.pending = true;
        self.error = None;
        if self.reading {
            self.waiting = Some(Box::new(move |this, window, cx| {
                this.submit(operation, receive, window, cx)
            }));
            cx.notify();
        } else {
            self.submit(operation, receive, window, cx);
        }
    }
    /// Submits a read to the browser's reader. Its reply lands it, unless a
    /// later read waits, which the reply starts instead.
    fn submit<T: Send + 'static>(
        &mut self,
        operation: impl FnOnce(GitRepository) -> anyhow::Result<T> + Send + 'static,
        receive: impl FnOnce(&mut Self, T, &mut Window, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let repo = self.repo.clone();
        self.cancellation = Default::default();
        let cancellation = self.cancellation.clone();
        let response = self.reader.submit_read(move || {
            gitturtle_core::run_cancellable_inspection(cancellation, || operation(repo))
        });
        self.reading = true;
        self.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = response.await.unwrap_or_else(|_| {
                Err(anyhow::anyhow!(
                    "Reflog read interrupted. Refresh to try again."
                ))
            });
            let _ = this.update_in(cx, |this, window, cx| {
                this.reading = false;
                if this.closed.load(std::sync::atomic::Ordering::Acquire) {
                    return;
                }
                // A later read waits: this reply is superseded, and starts it.
                if let Some(waiting) = this.waiting.take() {
                    waiting(this, window, cx);
                    return;
                }
                this.pending = false;
                this.reveal_filter = true;
                this.leave_loading_row(window, cx);
                if !this.current(cx) {
                    this.error = Some(
                        "The repository changed or an operation started. Reopen the reflog when it finishes."
                            .into(),
                    );
                } else {
                    match result {
                        Ok(value) => receive(this, value, window, cx),
                        Err(error) => this.error = Some(format!("{error:#}")),
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
    fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let scope = self.scope.read(cx).value().trim().to_owned();
        let reference = if scope == "HEAD" || scope.starts_with("refs/heads/") {
            scope
        } else {
            format!("refs/heads/{scope}")
        };
        self.selected = None;
        self.commit = None;
        self.message = None;
        self.changes.clear();
        self.page = None;
        self.read(
            move |repo| repo.reflog(&reference),
            |this, page, _, _| this.page = Some(page),
            window,
            cx,
        );
    }
    fn matches(&self, cx: &App) -> Vec<ReflogEntry> {
        let query = self.query.read(cx).value().trim().to_lowercase();
        self.page
            .as_ref()
            .map(|page| {
                page.entries
                    .iter()
                    .filter(|entry| {
                        entry.oid.contains(&query)
                            || entry.selector.to_lowercase().contains(&query)
                            || entry.message.to_lowercase().contains(&query)
                    })
                    .take(101)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }
    fn select(&mut self, entry: ReflogEntry, window: &mut Window, cx: &mut Context<Self>) {
        self.selected = Some(entry.clone());
        self.commit = None;
        self.message = None;
        self.changes.clear();
        self.name.update(cx, |input, cx| {
            if input.value().is_empty() {
                input.set_value(format!("recovery/{}", short_oid(&entry.oid)), window, cx);
            }
        });
        self.read(
            move |repo| {
                let commit = repo.inspect_reflog_commit(&entry)?;
                let changes = repo.changes(&entry.oid, 0)?;
                Ok((commit, changes))
            },
            |this, (commit, changes), window, cx| {
                let text = format!("{}\n\n{}", commit.subject, commit.body);
                this.message = Some(text::editor(&text, "text", None, window, cx));
                this.commit = Some(commit);
                this.changes = changes;
            },
            window,
            cx,
        );
    }
    fn recover(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending || self.commit.is_none() || !self.current(cx) {
            return;
        }
        let Some(entry) = self.selected.clone() else {
            return;
        };
        let name = self.name.read(cx).value().trim().to_owned();
        self.read(move |repo| repo.reflog_recovery_plan(&entry, &name), |this, plan: ReflogRecoveryPlan, window, cx| {
            let _ = this.owner.update(cx, |owner, cx| {
                if owner.path.as_deref() != Some(this.repo.path()) || owner.operation_busy.is_some() { return; }
                window.close_dialog(cx);
                owner.confirm_git_write("Create recovery branch".into(), format!("Repository: {}\nNew branch: {}\nCommit: {}\n{}\n\nSelected from {} at {}.\n\nCreate a new local branch pointing to this available commit. Your current branch, index, working files, and active Git operation stay as they are. Nothing is checked out, reset, fetched, or pushed.\n\nThe reflog and commit are revalidated before writing. If the log expires or changes, refresh and review again.", this.repo.path().display(), plan.branch, plan.entry.oid, plan.commit.subject, plan.entry.selector, full_date(plan.entry.timestamp)), "Create recovery branch", WriteCommand::RecoverReflog(Arc::new(plan)), window, cx);
            });
        }, window, cx);
    }
}

impl Render for ReflogBrowser {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let body_height = (window.viewport_size().height - px(240.)).max(px(120.));
        let matches = self.matches(cx);
        // A write refuses the recovery at once, and it dims only once the
        // write looks busy (`workspace::BusyLook`), as everywhere else; it
        // dimmed the moment any write started. The browser closes before its
        // own review submits, so a write it sees is never its own.
        let unavailable = self.pending || !self.current(cx);
        let dimmed = self.pending
            || self.owner.upgrade().is_none_or(|owner| {
                let owner = owner.read(cx);
                owner.path.as_deref() != Some(self.repo.path()) || owner.operation_shown().is_some()
            });
        // The content and its list scroll, so each clips to its bounds on both
        // axes and keeps the focus ring's room inside: the content beside and
        // below its controls, giving the sides back through its margin, and
        // the list around its rows, giving it all back through its margin. The
        // content counts each child's box in what it can scroll, so the list
        // borrows through a wrapper. No control moves.
        //
        // The content scrolls rather than shrinking its children to its height
        // bound, so the list's wrapper, the message editor and the
        // changed-file list never shrink. Each would otherwise give up its
        // whole height, having no automatic minimum: the editor sets a zero
        // one, and a scrolling list has none.
        //
        // The content's scrollbar stands in the dialog's side padding, beside
        // the content and its ring room, so neither its track nor its thumb
        // covers a control or its ring, and nothing moves for it. It paints
        // only while the content overflows; its track once started at the
        // body's edge, over the right side of a focused entry's ring.
        //
        // A control the keyboard moves focus onto scrolls into view in this
        // frame with its ring: the entry list shows the entry, and the
        // content the whole list, which never outgrows the content's view,
        // or the control. Each control reports where it lies to the content.
        self.entries.reveal(window, cx);
        self.content.reveal(window, cx);
        let room = appearance::button_ring_room(cx);
        let content_height = px(590.).min(body_height) + room;
        let list_height = (px(230.) + room * 2.).min(content_height - room * 2.);
        let file_row = file_row_height();
        let files_shown = file_rows_shown();
        let files_scroll = self.changes.len().min(100) > files_shown;
        // The filter follows the explanation, the scope's row and, once a
        // log is read, its summary and any limit notice: the least scroll
        // that shows it and its ring, which its wrapper holds.
        if std::mem::take(&mut self.reveal_filter)
            && self.query.read(cx).focus_handle(cx).is_focused(window)
        {
            let filter = 2
                + usize::from(self.page.is_some())
                + usize::from(self.page.as_ref().is_some_and(|page| page.truncated));
            self.content.scroll().scroll_to_item(filter);
        }
        let scope = self.scope.read(cx).focus_handle(cx);
        let read_log = self.content.focus("refresh-reflog", cx);
        let filter = self.query.read(cx).focus_handle(cx);
        let name = self.name.read(cx).focus_handle(cx);
        let copy = self.content.focus("copy-reflog-oid", cx);
        let review = self.content.focus("review-reflog-recovery", cx);
        let entries: Vec<_> = (0..matches.len().min(100))
            .map(|index| self.entries.focus(("reflog-entry", index), cx))
            .collect();
        // The entry list is one Tab stop: the entry the keyboard is on, else
        // the selected entry, else the first. Tab from it reaches the selected
        // entry's details next; every entry was a stop, so from a selected
        // entry of a 40-entry log Copy commit ID took 41 presses.
        let selected_entry = matches
            .iter()
            .take(100)
            .position(|entry| self.selected.as_ref() == Some(entry));
        let stop = entries
            .iter()
            .position(|focus| focus.is_focused(window))
            .or(selected_entry)
            .unwrap_or(0);
        // A selected entry's read shows its loading row in the details, where
        // its commit lands, and leaves Read log as it was, so selecting
        // entries with the arrow keys never moves the list or dims a control
        // above it; the row stood above the list, which moved down by its
        // height until each read landed. Other reads keep it above the list.
        let reading_entry = self.reading_entry();
        let loading = self.pending.then(|| {
            div()
                .flex()
                .gap_2()
                .on_children_prepainted(
                    self.content
                        .controls(vec![None, Some(self.cancel_read.handle().clone())], room),
                )
                .child(
                    label(
                        "reflog-loading",
                        if reading_entry {
                            "Reading the selected commit…"
                        } else {
                            "Reading local reflog or selected commit…"
                        },
                    )
                    .text_size(crate::appearance::ui_text(12.)),
                )
                .child(
                    button("cancel-reflog-read", "Cancel", "", false)
                        .track_focus(self.cancel_read.handle())
                        .debug_selector(|| "cancel-reflog-read".into())
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.cancel();
                            this.leave_loading_row(window, cx);
                            cx.notify();
                        })),
                )
        });
        let (loading_above, loading_below) = if reading_entry {
            (None, loading)
        } else {
            (loading, None)
        };
        // So does the error such a read ends with, as for a commit pruned
        // since Git logged it: it stood above the list, which moved down by
        // its height with each entry whose read failed.
        let error = self.error.as_ref().map(|error| {
            label("reflog-error", error.clone())
                .text_size(crate::appearance::ui_text(12.))
                .text_color(rgb(p.warning))
        });
        let (error_above, error_below) = if self.awaiting_entry() {
            (None, error)
        } else {
            (error, None)
        };
        let show_entry = self.entries.items(entries.clone(), room);
        let show_list = self.content.group(entries.clone(), room);
        // Past its height the entry list cues the rest with its scrollbar.
        let cue = crate::tags::ScrollCue::new(self.entries.scroll(), room);
        let shown =
            |focus: FocusHandle| div().on_children_prepainted(self.content.control(focus, room));
        let content = div().on_children_prepainted(self.content.observe()).id("reflog-browser-content").debug_selector(|| "reflog-browser-content".into()).flex().flex_col().gap_3().max_h(content_height).px(room).mx(-room).pb(room).overflow_y_scroll().track_scroll(self.content.scroll())
            .child(label("reflog-explanation", "Git records local reference movements here, including actions by other tools. HEAD belongs to this worktree; branch logs are shared. Entries expire, and unreachable objects may be pruned. This is not a permanent backup or a complete activity history.").text_size(crate::appearance::ui_text(12.)).text_color(rgb(p.muted)))
            .child(div().flex().gap_2().on_children_prepainted(self.content.controls(vec![Some(scope), Some(read_log.clone())], room)).child(div().flex_1().child(Input::new(&self.scope).aria_label("Reflog scope: HEAD or local branch name"))).child(button("refresh-reflog", "Read log", "refresh", false).track_focus(&read_log).debug_selector(|| "refresh-reflog".into()).disabled(self.pending && !reading_entry).on_click(cx.listener(|this, _, window, cx| this.refresh(window, cx)))))
            .when_some(self.page.as_ref(), |element, page| element.child(label("reflog-scope-summary", format!("{} · {} retained entries · newest first", page.reference, page.entries.len())).text_size(crate::appearance::ui_text(12.))).when(page.truncated, |element| element.child(label("reflog-limit", "Showing the newest 1,000 records within a 4 MiB tail. Older records are outside this bounded view.").text_size(crate::appearance::ui_text(12.)).text_color(rgb(p.warning)))))
            // The filter stands in the ring's room, given back through its
            // margin, so revealing it shows its ring whole.
            .child(shown(filter).py(room).my(-room).child(Input::new(&self.query).aria_label("Filter loaded reflog entries").cleanable(true)))
            .children(loading_above)
            .children(error_above)
            .child(div().debug_selector(|| "reflog-entries".into()).flex().flex_col().flex_shrink_0().on_children_prepainted(show_list).child(cue.list(div().on_children_prepainted(cue.observe(show_entry)).id("reflog-list").debug_selector(|| "reflog-list".into()).max_h(list_height).p(room).m(-room).overflow_y_scroll().track_scroll(self.entries.scroll()).on_key_down(cx.listener(Self::entry_key)).flex().flex_col().gap_1().children(matches.iter().take(100).zip(entries).enumerate().map(|(index, (entry, focus))| {
                let entry = entry.clone(); let text = format!("{} · {} · {} · {}", entry.selector, short_oid(&entry.oid), full_date(entry.timestamp), if entry.message.is_empty() { "Reference updated" } else { &entry.message });
                entry_button(index, text, self.selected.as_ref() == Some(&entry), &self.cut_entries, cx.entity().downgrade(), p).track_focus(&focus).tab_stop(index == stop).on_click(cx.listener(move |this, _, window, cx| this.select(entry.clone(), window, cx)))
            })).when(matches.is_empty() && self.page.is_some() && !self.pending, |element| element.child(div().ml(TEXT_INSET).child(if self.page.as_ref().is_some_and(|page| page.entries.is_empty()) { EmptyState::new("reflog-empty", "No reflog records for this scope").tooltip("Reflog recording may be disabled, the branch may not exist, or older entries may have expired.") } else { EmptyState::new("reflog-empty", "No loaded entries match this filter") }.block(cx)))), "reflog-list-scrollbar")))
            .when(matches.len() > 100, |element| element.child(label("reflog-match-limit", "Showing 100 matches. Narrow the filter to find another retained entry.").text_size(crate::appearance::ui_text(12.))))
            .when_some(self.selected.as_ref(), |element, entry| {
                let oid = entry.oid.clone(); let mark = copy_feedback::Mark::new("copy-reflog-oid", &entry.oid); let copy_mark = mark.clone();
                element.child(label("reflog-selected-object", format!("{}\nCommit: {}\nPrevious: {}", entry.selector, entry.oid, entry.previous_oid)).text_size(crate::appearance::ui_text(12.)))
                    .child(shown(copy.clone()).flex().flex_col().child(copy_feedback::copy_button(mark, "Copy commit ID", "", cx).track_focus(&copy).on_click(move |_, window, cx| copy_feedback::copy(window, cx, Some(copy_mark.clone()), "commit ID", oid.clone()))))
                    .children(loading_below)
                    .children(error_below)
            })
            .when_some(self.commit.as_ref(), |element, commit| element
                .child(label("reflog-commit-metadata", format!("{} · {}\n{} parent{} · {} against {}", commit.author, full_date(commit.timestamp), commit.parents.len(), if commit.parents.len() == 1 { "" } else { "s" }, crate::changed_files(self.changes.len()), if commit.parents.len() > 1 { "first parent" } else { "parent or empty tree" })).text_size(crate::appearance::ui_text(12.)))
                // The read-only editor draws no edge of its own: holding
                // keyboard focus, it rings its own edge over its lines, as
                // Compare's text does, where it showed no focus at all.
                .when_some(self.message.as_ref(), |element, message| { let focus = message.read(cx).focus_handle(cx); let ring = self.owner.upgrade().and_then(|owner| owner.read(cx).focus_ring.edge(&focus, rgb(p.accent))); element.child(shown(focus).debug_selector(|| "reflog-commit-message".into()).relative().flex_shrink_0().child(crate::editor_find::Editor::new(message).readonly(true).h(px(110.)).aria_label("Reflog commit message")).children(ring)) })
                // Whole rows of one line each, so the list's bound cuts no
                // row; past that bound its scrollbar, in a gutter of its own,
                // cues the rest.
                .child(div().relative().flex_shrink_0().child(native_accessibility::clips_children(div().id("reflog-commit-files").debug_selector(|| "reflog-commit-files".into()).max_h(file_row * files_shown as f32).overflow_y_scroll().track_scroll(&self.files_scroll).flex().flex_col().when(files_scroll, |list| list.pr(Scrollbar::width())).children(self.changes.iter().take(100).enumerate().map(|(index, file)| { let text = format!("{} · {}", file.status.label(), file.path().display()); native_accessibility::static_text(div().id(("reflog-commit-file", index)).debug_selector(move || format!("reflog-commit-file-{index}")), text.clone()).ml(TEXT_INSET).flex_shrink_0().h(file_row).text_size(crate::appearance::ui_text(FILE_ROW_TEXT)).line_height(file_row).whitespace_nowrap().overflow_hidden().text_ellipsis().child(text) })))).when(files_scroll, |list| list.child(Scrollbar::vertical(&self.files_scroll).id("reflog-files-scrollbar").mode(ScrollbarMode::Always))))
                .when(self.changes.len() > 100, |element| element.child(label("reflog-file-limit", "Showing the first 100 changed paths.").text_size(crate::appearance::ui_text(12.))))
                .child(label("reflog-recovery-name-label", "Create a new branch at this commit to keep it reachable").text_size(crate::appearance::ui_text(12.)))
                .child(shown(name).child(Input::new(&self.name).aria_label("New recovery branch name")))
                .child(shown(review.clone()).flex().flex_col().child(button("review-reflog-recovery", "Review recovery branch…", "", false).track_focus(&review).debug_selector(|| "review-reflog-recovery".into()).map(|review| crate::workspace::refusing(review, || "review-reflog-recovery".into(), unavailable, dimmed)).on_click(cx.listener(|this, _, window, cx| this.recover(window, cx))))));
        div()
            .relative()
            .min_h_0()
            .flex()
            .flex_col()
            .child(native_accessibility::clips_children(content))
            .child(
                div()
                    .debug_selector(|| "reflog-browser-scrollbar".into())
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .right(-Scrollbar::width() - room)
                    .w(Scrollbar::width())
                    .child(
                        Scrollbar::vertical(self.content.scroll())
                            .id("reflog-browser-scrollbar")
                            .mode(ScrollbarMode::Always)
                            .viewport_from_layout(),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tags::tests::{
        assert_room_for_rings, draw, hovered_tooltips, kit_dialog_footer_gap, tagged_repository,
        window,
    };

    /// The Reflog dialog gives its room back through the footer gap, taking it
    /// to be the kit's default; any other default would move the footer.
    #[gpui::test]
    fn dialog_footer_gap_is_the_kits(cx: &mut TestAppContext) {
        assert_eq!(kit_dialog_footer_gap(cx), DIALOG_FOOTER_GAP);
    }
    use ::core::prelude::v1::test;

    /// The reflog's content and its list both scroll, so GPUI clips each to
    /// its bounds on both axes. Read log, at the content's side, and the
    /// first and last entries keep their rings whole, and nothing moves.
    #[gpui::test]
    async fn reflog_browser_keeps_room_for_every_focus_ring(cx: &mut TestAppContext) {
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        let (app, cx) = window(cx, &repo);
        let browser = cx.update(|window, cx| {
            let browser =
                cx.new(|cx| ReflogBrowser::new(app.downgrade(), repo.clone(), window, cx));
            browser.update(cx, |this, cx| this.refresh(window, cx));
            ReflogBrowser::show(browser.clone(), window, cx);
            browser
        });
        let read = browser.update(cx, |this, _| this.task.take());
        read.expect("the log is read").await;
        draw(cx);
        assert_room_for_rings(
            cx,
            &app,
            &["refresh-reflog", "reflog-entry-0", "reflog-entry-2"],
            &[
                "reflog-title",
                "reflog-explanation",
                "reflog-scope-summary",
                "reflog-entry-1",
            ],
        );
    }

    /// The Reflog opens with focus in its filter field, which draws its focus
    /// whole: the field's 1 px accent edge and the kit's ring 3 px around it,
    /// every side inside the dialog's clips. It opened with focus on the
    /// dialog's shell, which draws nothing. Escape still closes it and
    /// returns focus to the History list that opened it.
    #[gpui::test]
    async fn the_reflog_opens_with_its_filter_focused_and_ringed(cx: &mut TestAppContext) {
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        let (app, cx) = window(cx, &repo);
        cx.simulate_resize(size(px(1000.), px(680.)));
        let opener = app.read_with(cx, |app, _| app.focus.clone());
        cx.update(|window, cx| window.focus(&opener, cx));
        draw(cx);
        let browser = cx.update(|window, cx| {
            let browser =
                cx.new(|cx| ReflogBrowser::new(app.downgrade(), repo.clone(), window, cx));
            browser.update(cx, |this, cx| this.refresh(window, cx));
            ReflogBrowser::show(browser.clone(), window, cx);
            browser
        });
        let read = browser.update(cx, |this, _| this.task.take());
        read.expect("the log is read").await;
        draw(cx);
        park_pointer(cx);

        let (filter, text) = cx.read(|cx| {
            let query = browser.read(cx).query.read(cx);
            (query.focus_handle(cx), query.input_bounds())
        });
        assert!(
            cx.update(|window, _| filter.is_focused(window)),
            "the filter holds focus as the Reflog opens"
        );
        focus_ring::assert_input_ringed(cx, text, "the Reflog's filter");

        cx.simulate_keystrokes("escape");
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(400));
        draw(cx);
        assert!(!cx.update(|window, cx| window.has_active_dialog(cx)));
        assert!(
            cx.update(|window, _| opener.is_focused(window)),
            "Escape returns focus to the opener"
        );
    }

    /// A read that answers while keyboard focus is on its loading row's
    /// Cancel takes the row away and hands focus to the filter, which draws
    /// its ring whole, and Escape still closes the Reflog. Focus stayed on
    /// the Cancel no frame drew, where Escape reached nothing.
    #[gpui::test]
    async fn a_read_landing_while_its_cancel_holds_focus_leaves_focus_on_the_filter(
        cx: &mut TestAppContext,
    ) {
        let fixture = tempfile::tempdir().unwrap();
        let repo = tagged_repository(fixture.path());
        let (app, cx) = window(cx, &repo);
        cx.simulate_resize(size(px(1000.), px(680.)));
        let opener = app.read_with(cx, |app, _| app.focus.clone());
        cx.update(|window, cx| window.focus(&opener, cx));
        draw(cx);
        let browser = cx.update(|window, cx| {
            let browser =
                cx.new(|cx| ReflogBrowser::new(app.downgrade(), repo.clone(), window, cx));
            browser.update(cx, |this, cx| this.refresh(window, cx));
            ReflogBrowser::show(browser.clone(), window, cx);
            browser
        });
        let read = browser.update(cx, |this, _| this.task.take());
        read.expect("the log is read").await;
        draw(cx);

        // Read log again while the browser's thread is held, as by a slow
        // read, and Tab from the filter to the loading row's Cancel.
        let held = browser.read_with(cx, |this, _| this.reader.hold());
        cx.update(|window, cx| browser.update(cx, |this, cx| this.refresh(window, cx)));
        draw(cx);
        let cancel = browser.read_with(cx, |this, _| this.cancel_read.handle().clone());
        cx.simulate_keystrokes("tab");
        draw(cx);
        assert!(
            cx.update(|window, _| cancel.is_focused(window)),
            "Tab moves from the filter to the loading row's Cancel"
        );
        assert!(cx.debug_bounds("cancel-reflog-read").is_some());

        drop(held);
        let read = browser.update(cx, |this, _| this.task.take());
        read.expect("the log is read again").await;
        draw(cx);
        park_pointer(cx);
        assert!(
            cx.debug_bounds("cancel-reflog-read").is_none(),
            "the reply takes the loading row away"
        );
        let (filter, text) = cx.read(|cx| {
            let query = browser.read(cx).query.read(cx);
            (query.focus_handle(cx), query.input_bounds())
        });
        assert!(
            cx.update(|window, _| filter.is_focused(window)),
            "focus moves from the Cancel to the filter"
        );
        assert!(
            !cx.update(|window, _| window.context_stack().is_empty()),
            "the frame draws the stop that holds focus"
        );
        focus_ring::assert_input_ringed(cx, text, "the Reflog's filter");

        cx.simulate_keystrokes("escape");
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(400));
        draw(cx);
        assert!(
            !cx.update(|window, cx| window.has_active_dialog(cx)),
            "Escape closes the Reflog"
        );
        assert!(
            cx.update(|window, _| opener.is_focused(window)),
            "Escape returns focus to the opener"
        );
    }

    /// The entry list is one Tab stop, as History's list is: Tab from the
    /// filter reaches the first entry, Space selects it as before, and Tab
    /// from the selected entry reaches Copy commit ID next, Shift+Tab the
    /// entry and then the filter; every entry was a stop, so from a selected
    /// entry of a 40-entry log Copy commit ID took 41 presses. Down selects
    /// the next entry and moves focus, its ring whole, onto it, and End the
    /// last, shown inside the list, from where Tab again reaches Copy commit
    /// ID. While a selected entry's commit is read, its loading row and
    /// Cancel stand below the list, which stays where it was; the row stood
    /// above the list and moved it down until each read landed.
    #[gpui::test]
    async fn the_entry_list_is_one_tab_stop_with_arrow_keys(cx: &mut TestAppContext) {
        let fixture = tempfile::tempdir().unwrap();
        let repo = changed_repository(fixture.path(), 1);
        let (_, browser, cx) = opened_browser(cx, &repo).await;
        let entry = |index: usize| Some(ElementId::from(("reflog-entry", index)));
        let in_list = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| browser.read(cx).entries.focused(window))
        };
        let in_content = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| browser.read(cx).content.focused(window))
        };
        let filter_focused = |cx: &mut VisualTestContext| {
            let filter = browser.read_with(cx, |this, cx| this.filter_focus(cx));
            cx.update(|window, _| filter.is_focused(window))
        };
        let selected = |cx: &mut VisualTestContext| {
            browser.read_with(cx, |this, _| {
                let page = this.page.as_ref().expect("the log loaded");
                this.selected
                    .as_ref()
                    .and_then(|chosen| page.entries.iter().position(|entry| entry == chosen))
            })
        };
        let landed = async |cx: &mut VisualTestContext| {
            let read = browser.update(cx, |this, _| this.task.take());
            read.expect("the commit is read").await;
            draw(cx);
        };
        let copy = Some(ElementId::from("copy-reflog-oid"));
        assert!(filter_focused(cx), "the filter holds focus as it opens");

        cx.simulate_keystrokes("tab");
        draw(cx);
        assert_eq!(in_list(cx), entry(0), "Tab reaches the first entry");
        // A focused Button acts on Space as the key comes up.
        let keystroke = Keystroke::parse("space").unwrap();
        cx.simulate_event(KeyDownEvent {
            keystroke: keystroke.clone(),
            is_held: false,
            prefer_character_input: false,
        });
        cx.simulate_event(KeyUpEvent { keystroke });
        landed(cx).await;
        assert_eq!(selected(cx), Some(0), "Space selects it");
        cx.simulate_keystrokes("tab");
        draw(cx);
        assert_eq!(in_content(cx), copy, "Tab from the selected entry");
        cx.simulate_keystrokes("shift-tab");
        draw(cx);
        assert_eq!(in_list(cx), entry(0), "Shift+Tab returns to the entry");
        cx.simulate_keystrokes("shift-tab");
        draw(cx);
        assert!(filter_focused(cx), "and then to the filter");
        cx.simulate_keystrokes("tab");
        draw(cx);
        assert_eq!(in_list(cx), entry(0), "the selected entry is the stop");

        // Down, with the commit's read held as a slow read is.
        let list = rendered(cx, "reflog-list");
        let held = browser.read_with(cx, |this, _| this.reader.hold());
        cx.simulate_keystrokes("down");
        draw(cx);
        assert_eq!(in_list(cx), entry(1), "Down moves focus");
        assert_eq!(selected(cx), Some(1), "and selects the entry");
        crate::tags::tests::assert_rings_whole(cx, &["reflog-entry-1"]);
        assert_eq!(rendered(cx, "reflog-list"), list, "the list stays put");
        let loading = rendered(cx, "cancel-reflog-read");
        assert!(
            loading.top() >= list.bottom(),
            "the entry's loading row {loading:?} stands below the list {list:?}"
        );
        // Its Cancel follows Copy commit ID, and focus it holds when the read
        // lands returns to the entry, the stop above the row.
        cx.simulate_keystrokes("tab tab");
        draw(cx);
        let cancel = browser.read_with(cx, |this, _| this.cancel_read.handle().clone());
        assert!(
            cx.update(|window, _| cancel.is_focused(window)),
            "Tab reaches the entry's Cancel past Copy commit ID"
        );
        drop(held);
        landed(cx).await;
        assert!(cx.debug_bounds("cancel-reflog-read").is_none());
        assert_eq!(in_list(cx), entry(1), "focus returns to the entry");

        cx.simulate_keystrokes("end");
        landed(cx).await;
        assert_eq!(in_list(cx), entry(11), "End moves focus to the last entry");
        assert_eq!(selected(cx), Some(11));
        let list = rendered(cx, "reflog-list");
        assert!(
            shows_whole(list, rendered(cx, "reflog-entry-11")),
            "the last entry is shown inside the list"
        );
        crate::tags::tests::assert_rings_whole(cx, &["reflog-entry-11"]);
        cx.simulate_keystrokes("tab");
        draw(cx);
        assert_eq!(in_content(cx), copy, "Tab from the last entry");
    }

    /// An entry whose commit has been pruned since Git logged it shows its
    /// read's error in the details below the list, where the read's loading
    /// row stood, and the list stays where it was, focus on the entry; the
    /// error stood above the list, which moved down by its height with each
    /// entry whose read failed. The next entry's read takes it away.
    #[gpui::test]
    async fn a_pruned_commits_error_stands_below_the_entry_list(cx: &mut TestAppContext) {
        let fixture = tempfile::tempdir().unwrap();
        let repo = pruned_repository(fixture.path());
        let (_, browser, cx) = opened_browser(cx, &repo).await;
        let entry = |index: usize| Some(ElementId::from(("reflog-entry", index)));
        cx.simulate_keystrokes("tab");
        draw(cx);
        let list = rendered(cx, "reflog-list");

        cx.simulate_keystrokes("down");
        settle_reads(cx, &browser);
        let error = browser
            .read_with(cx, |this, _| this.error.clone())
            .expect("the pruned commit's read fails");
        assert!(error.contains("unavailable locally"), "{error}");
        assert_eq!(rendered(cx, "reflog-list"), list, "the list stays put");
        let shown = rendered(cx, "reflog-error");
        let details = rendered(cx, "reflog-selected-object");
        assert!(
            shown.top() >= details.bottom() && details.top() >= list.bottom(),
            "the error {shown:?} stands below the entry's details {details:?}, \
             below the list {list:?}"
        );
        assert_eq!(
            cx.update(|window, cx| browser.read(cx).entries.focused(window)),
            entry(1),
            "focus stays on the entry"
        );

        cx.simulate_keystrokes("down");
        settle_reads(cx, &browser);
        assert!(cx.debug_bounds("reflog-error").is_none(), "the next read");
        assert!(browser.read_with(cx, |this, _| this.commit.is_some()));
        assert_eq!(rendered(cx, "reflog-list"), list, "the list stays put");
    }

    /// Down held past the last entry while a commit's read is slow selects
    /// each entry it reaches as it goes, but the browser reads one commit at
    /// a time: the entry the key rests on waits for the slow read, and the
    /// entries passed over are never read, so no press is refused. Every
    /// press queued a read on the browser's reader, whose queue holds eight,
    /// so the presses after the eighth failed at once with "Background
    /// executor is busy…". Once the slow read answers, the last entry's
    /// commit lands.
    #[gpui::test]
    async fn held_arrows_wait_for_a_slow_read(cx: &mut TestAppContext) {
        let fixture = tempfile::tempdir().unwrap();
        let repo = changed_repository(fixture.path(), 1);
        let (_, browser, cx) = opened_browser(cx, &repo).await;
        cx.simulate_keystrokes("tab");
        draw(cx);

        // The reader's thread is held, as by a slow read, while Down repeats
        // sixteen times through the twelve entries.
        let held = browser.read_with(cx, |this, _| this.reader.hold());
        let down = Keystroke::parse("down").unwrap();
        for repeat in 0..16 {
            cx.simulate_event(KeyDownEvent {
                keystroke: down.clone(),
                is_held: repeat > 0,
                prefer_character_input: false,
            });
            draw(cx);
        }
        cx.simulate_event(KeyUpEvent { keystroke: down });
        draw(cx);
        let last = browser.read_with(cx, |this, _| {
            this.page.as_ref().expect("the log loaded").entries[11].clone()
        });
        assert_eq!(
            cx.update(|window, cx| browser.read(cx).entries.focused(window)),
            Some(ElementId::from(("reflog-entry", 11_usize))),
            "focus moves with every press"
        );
        let (selected, pending, error) = browser.read_with(cx, |this, _| {
            (this.selected.clone(), this.pending, this.error.clone())
        });
        assert_eq!(selected.as_ref(), Some(&last), "and so does the selection");
        assert_eq!(error, None, "no press is refused");
        assert!(pending, "the last entry's read waits");
        assert!(cx.debug_bounds("reflog-error").is_none());
        assert!(cx.debug_bounds("cancel-reflog-read").is_some());

        drop(held);
        settle_reads(cx, &browser);
        let (commit, error) =
            browser.read_with(cx, |this, _| (this.commit.clone(), this.error.clone()));
        assert_eq!(error, None);
        assert_eq!(
            commit.map(|commit| commit.oid),
            Some(last.oid),
            "the last entry's commit lands"
        );
    }

    /// In a 461 × 490 half tile, at 13 and 18 pt, every Tab stop of the
    /// Reflog draws its ring whole and in view while the pointer moves, with
    /// the log alone and with an entry selected, whose form lies below the
    /// content's fold, and so does every entry the arrow keys move focus onto
    /// in the entry list, which is one Tab stop: Down through all twelve from
    /// the first, past the list's fold and the content's, then Home and End.
    /// Focus moved onto an entry once stayed where the entry lay, past the
    /// list's or the content's fold, the first cut by the fold and the rest
    /// drawing no ring, and the read-only commit message drew no ring at any
    /// size.
    #[gpui::test]
    async fn a_narrow_reflog_shows_every_stop_whole(cx: &mut TestAppContext) {
        use crate::focus_ring::sweep;
        let fixture = tempfile::tempdir().unwrap();
        let repo = changed_repository(fixture.path(), 3);
        let (app, cx) = window(cx, &repo);
        sweep::prepare(&app, cx);
        cx.simulate_resize(size(px(461.), px(490.)));
        let mut failures = Vec::new();
        for interface in sweep::SIZES {
            sweep::interface_size(cx, interface);
            for selected in [false, true] {
                let surface = if selected {
                    "the Reflog with an entry selected"
                } else {
                    "the Reflog"
                };
                let browser = std::rc::Rc::new(std::cell::RefCell::new(None));
                let opened = browser.clone();
                sweep::open_dialog(&app, cx, surface, move |app, window, cx| {
                    let browser = app.open_reflog_browser(window, cx);
                    *opened.borrow_mut() = browser.clone();
                    browser
                })
                .await;
                let browser = browser.borrow().clone().expect("the Reflog opens");
                if selected {
                    select_newest(cx, &browser).await;
                    let filter = browser.read_with(cx, |this, cx| this.filter_focus(cx));
                    cx.update(|window, cx| window.focus(&filter, cx));
                    draw(cx);
                }
                sweep::walk(&app, cx, surface, interface, &mut failures);
                // The walk reaches the one entry the list's stop stands on;
                // the arrows reach the others, from the filter's next stop.
                let filter = browser.read_with(cx, |this, cx| this.filter_focus(cx));
                cx.update(|window, cx| window.focus(&filter, cx));
                draw(cx);
                let at = format!("{surface} at {interface} pt");
                key_to_entry(cx, &browser, "tab", 0, &at, &mut failures);
                for index in 1..12 {
                    key_to_entry(cx, &browser, "down", index, &at, &mut failures);
                }
                key_to_entry(cx, &browser, "home", 0, &at, &mut failures);
                key_to_entry(cx, &browser, "end", 11, &at, &mut failures);
                sweep::close_dialogs(cx, surface, interface, &mut failures);
            }
        }
        sweep::assert_whole(failures);
    }

    /// Twelve entries overflow the entry list, and its scrollbar's thumb
    /// follows the list's offset in a gutter clear of every entry's ring, at
    /// 13 and 18 pt. The list cued nothing beyond its edge, where an AT-SPI
    /// client still reaches the next entry past it (HEAD@{6} in a 640 × 680
    /// window), as AccessKit intends.
    #[gpui::test]
    async fn entry_list_cues_entries_beyond_its_edge(cx: &mut TestAppContext) {
        let fixture = tempfile::tempdir().unwrap();
        let repo = changed_repository(fixture.path(), 1);
        let (app, browser, cx) = opened_browser(cx, &repo).await;
        cx.simulate_resize(size(px(1000.), px(980.)));
        draw(cx);
        let room = cx.update(|_, cx| appearance::button_ring_room(cx));
        let scroll = browser.read_with(cx, |this, _| this.entries.scroll().clone());
        let rows: Vec<&'static str> = (0..12)
            .map(|index| &*format!("reflog-entry-{index}").leak())
            .collect();
        crate::tags::tests::assert_scroll_cue(cx, &app, &scroll, "reflog-list", room, &rows);
    }

    /// The dialog's text starts at one inset at 1480 × 980: the title, the
    /// explanation, the entries' text and the scope and filter fields' text
    /// started at three (the content edge, 8 px and 11 px in). An entry
    /// whose text is whole there shows no tooltip, which covered the entry
    /// above; in a narrow tile, where the entry cuts its text, it does.
    #[gpui::test]
    async fn the_reflog_text_starts_at_one_inset_and_only_cut_entries_have_tooltips(
        cx: &mut TestAppContext,
    ) {
        let fixture = tempfile::tempdir().unwrap();
        let repo = changed_repository(fixture.path(), 3);
        let (_, browser, cx) = opened_browser(cx, &repo).await;
        cx.simulate_resize(size(px(1480.), px(980.)));
        draw(cx);
        let content = rendered(cx, "reflog-browser-content");
        let (scope, query) = cx.read(|cx| {
            let this = browser.read(cx);
            (
                this.scope.read(cx).input_bounds().left(),
                this.query.read(cx).input_bounds().left(),
            )
        });
        let starts = [
            ("title", rendered(cx, "reflog-title").left()),
            ("explanation", rendered(cx, "reflog-explanation").left()),
            ("summary", rendered(cx, "reflog-scope-summary").left()),
            ("entry", rendered(cx, "reflog-entry-text-0").left()),
            ("scope field", scope),
            ("filter", query),
        ];
        // The content keeps the focus ring's room inside its edge.
        let edge = content.left() + cx.update(|_, cx| appearance::button_ring_room(cx));
        for (name, start) in starts {
            assert_eq!(start, edge + TEXT_INSET, "{name}: {starts:?}");
        }
        let whole = rendered(cx, "reflog-entry-text-0");
        assert!(whole.size.width > px(400.), "{whole:?}");
        assert_eq!(hovered_tooltips(cx, "reflog-entry-0"), 0, "a whole entry");

        cx.simulate_resize(size(px(461.), px(700.)));
        draw(cx);
        assert_eq!(hovered_tooltips(cx, "reflog-entry-0"), 1, "a cut entry");
    }

    /// Runs Git in `path` with no configuration beyond the fixture identity,
    /// returning its output without the trailing newline.
    fn git(path: &std::path::Path, args: &[&str]) -> String {
        let output = std::process::Command::new("git")
            .args([
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "-c",
                "commit.gpgSign=false",
                "-c",
                "core.hooksPath=/dev/null",
            ])
            .args(args)
            .current_dir(path)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim_end().into()
    }

    /// [`changed_repository`] with HEAD moved on to one more commit and back,
    /// that commit's object then removed as pruning removes it: HEAD@{1}
    /// names a commit no longer available locally.
    fn pruned_repository(directory: &std::path::Path) -> GitRepository {
        let repo = changed_repository(directory, 1);
        git(
            repo.path(),
            &["commit", "--quiet", "--allow-empty", "-m", "Pruned"],
        );
        let oid = git(repo.path(), &["rev-parse", "HEAD"]);
        git(repo.path(), &["reset", "--quiet", "--hard", "HEAD~1"]);
        let objects = repo.path().join(".git/objects");
        std::fs::remove_file(objects.join(&oid[..2]).join(&oid[2..])).unwrap();
        repo
    }

    /// Lets every read the browser started land, and any read a landing
    /// reply starts: its reader answers on its own thread, which
    /// `run_until_parked` cannot wait for.
    fn settle_reads(cx: &mut VisualTestContext, browser: &Entity<ReflogBrowser>) {
        for _ in 0..4 {
            browser.read_with(cx, |this, _| this.reader.drain());
            cx.executor().run_until_parked();
            if !browser.read_with(cx, |this, _| this.pending) {
                draw(cx);
                return;
            }
        }
        panic!("the Reflog's reads land");
    }

    /// Presses `key` on the Reflog, lets the read it starts land and adds to
    /// `failures`, named by `at`, unless focus moves to entry `index`, whose
    /// Button draws its ring whole, every side inside the list's and the
    /// content's clips, while the pointer moves: an entry left past either
    /// fold is cut by it or draws no ring at all.
    fn key_to_entry(
        cx: &mut VisualTestContext,
        browser: &Entity<ReflogBrowser>,
        key: &str,
        index: usize,
        at: &str,
        failures: &mut Vec<String>,
    ) {
        cx.simulate_keystrokes(key);
        settle_reads(cx, browser);
        draw(cx);
        cx.simulate_mouse_move(point(px(2.), px(2.)), None, Modifiers::default());
        draw(cx);
        let at = format!("{at}, {key} to entry {index}");
        let focused = cx.update(|window, cx| browser.read(cx).entries.focused(window));
        if focused != Some(ElementId::from(("reflog-entry", index))) {
            failures.push(format!("{at}: focus is on {focused:?}"));
            return;
        }
        let selector: &'static str = format!("reflog-entry-{index}").leak();
        let entry = rendered(cx, selector);
        let ring =
            entry.dilate(appearance::BUTTON_FOCUS_RING.gap + appearance::BUTTON_FOCUS_RING.width);
        let accent: Hsla = cx.read(|cx| rgb(palette(cx).accent).into());
        let rings: Vec<_> = crate::focus_ring::painted_outlines_where(cx, |color| {
            color.a > 0.
                && Hsla {
                    a: accent.a,
                    ..color
                } == accent
        })
        .into_iter()
        .filter(|outline| {
            outline.drawn
                && [
                    (outline.bounds.left(), ring.left()),
                    (outline.bounds.top(), ring.top()),
                    (outline.bounds.right(), ring.right()),
                    (outline.bounds.bottom(), ring.bottom()),
                ]
                .into_iter()
                .all(|(drawn, expected)| (drawn - expected).abs() <= px(0.5))
        })
        .collect();
        if rings.is_empty() {
            failures.push(format!(
                "{at}: the entry at {entry:?} paints no ring in view"
            ));
        }
        for outline in rings.iter().filter(|outline| !outline.whole) {
            failures.push(format!(
                "{at}: a clip cuts its ring at {:?}, sides {:?}",
                outline.bounds, outline.sides
            ));
        }
    }

    /// Twelve HEAD reflog entries, as on the native fixture, so the entry list
    /// fills its height; the newest commit changes `files` files.
    fn changed_repository(directory: &std::path::Path, files: usize) -> GitRepository {
        let repo = crate::views::tests::fixture_repository(directory.join("repository"));
        for index in 0..11 {
            let message = format!("Commit {index}");
            git(
                repo.path(),
                &["commit", "--quiet", "--allow-empty", "-m", &message],
            );
        }
        for index in 0..files {
            let name = format!("file-{index}.txt");
            std::fs::write(repo.path().join(&name), format!("{name}\n")).unwrap();
        }
        git(repo.path(), &["add", "--all"]);
        git(
            repo.path(),
            &["commit", "--quiet", "-m", "Add files\n\nWith a body."],
        );
        repo
    }

    /// The application at 1000x680 with the Reflog browser open on `repo`'s
    /// HEAD log, read and drawn.
    async fn opened_browser<'a>(
        cx: &'a mut TestAppContext,
        repo: &GitRepository,
    ) -> (
        Entity<GitTurtle>,
        Entity<ReflogBrowser>,
        &'a mut VisualTestContext,
    ) {
        let (app, cx) = window(cx, repo);
        cx.simulate_resize(size(px(1000.), px(680.)));
        draw(cx);
        let browser = cx.update(|window, cx| {
            let browser =
                cx.new(|cx| ReflogBrowser::new(app.downgrade(), repo.clone(), window, cx));
            browser.update(cx, |this, cx| this.refresh(window, cx));
            ReflogBrowser::show(browser.clone(), window, cx);
            browser
        });
        let read = browser.update(cx, |this, _| this.task.take());
        read.expect("the log is read").await;
        draw(cx);
        park_pointer(cx);
        (app, browser, cx)
    }

    /// Selects the newest entry and waits for its commit and changed files.
    async fn select_newest(cx: &mut VisualTestContext, browser: &Entity<ReflogBrowser>) {
        let entry = browser.read_with(cx, |this, _| {
            let page = this.page.as_ref().expect("the log loaded");
            assert_eq!(page.entries.len(), 12);
            page.entries[0].clone()
        });
        cx.update(|window, cx| browser.update(cx, |this, cx| this.select(entry, window, cx)));
        let read = browser.update(cx, |this, _| this.task.take());
        read.expect("the commit is read").await;
        draw(cx);
        park_pointer(cx);
    }

    fn park_pointer(cx: &mut VisualTestContext) {
        cx.simulate_mouse_move(point(px(1.), px(1.)), None, Modifiers::default());
        draw(cx);
    }

    fn rendered(cx: &mut VisualTestContext, selector: &'static str) -> Bounds<Pixels> {
        cx.debug_bounds(selector)
            .unwrap_or_else(|| panic!("rendered {selector}"))
    }

    /// `inner` lies whole inside `outer`, within float error, and has height.
    fn shows_whole(outer: Bounds<Pixels>, inner: Bounds<Pixels>) -> bool {
        let slack = px(0.01);
        inner.size.height > px(0.)
            && outer.left() <= inner.left() + slack
            && outer.top() <= inner.top() + slack
            && outer.right() + slack >= inner.right()
            && outer.bottom() + slack >= inner.bottom()
    }

    /// At 1000x680 and the default text size, a selected entry whose commit
    /// changes three files takes the content past its height bound. The
    /// content scrolls to reach the branch form, and none of its children
    /// shrinks to make room: the entry list shows whole entries, the
    /// changed-file list whole rows and the message editor its 110 px, each
    /// below the one before.
    #[gpui::test]
    async fn selected_entry_keeps_lists_and_message_whole(cx: &mut TestAppContext) {
        let fixture = tempfile::tempdir().unwrap();
        let repo = changed_repository(fixture.path(), 3);
        let (app, cx) = window(cx, &repo);
        cx.simulate_resize(size(px(1000.), px(680.)));
        draw(cx);
        let browser = cx.update(|window, cx| {
            let browser =
                cx.new(|cx| ReflogBrowser::new(app.downgrade(), repo.clone(), window, cx));
            browser.update(cx, |this, cx| this.refresh(window, cx));
            ReflogBrowser::show(browser.clone(), window, cx);
            browser
        });
        let read = browser.update(cx, |this, _| this.task.take());
        read.expect("the log is read").await;
        let entry = browser.read_with(cx, |this, _| {
            let page = this.page.as_ref().expect("the log loaded");
            assert_eq!(page.entries.len(), 12);
            page.entries[0].clone()
        });
        cx.update(|window, cx| browser.update(cx, |this, cx| this.select(entry, window, cx)));
        let read = browser.update(cx, |this, _| this.task.take());
        read.expect("the commit is read").await;
        assert_eq!(browser.read_with(cx, |this, _| this.changes.len()), 3);
        draw(cx);
        cx.simulate_mouse_move(point(px(1.), px(1.)), None, Modifiers::default());
        draw(cx);

        let content = rendered(cx, "reflog-browser-content");
        let list = rendered(cx, "reflog-list");
        let entry = rendered(cx, "reflog-entry-0");
        assert!(
            shows_whole(list, entry) && shows_whole(content, entry),
            "the first entry {entry:?} lies whole in the list {list:?} and the content {content:?}"
        );
        let files = rendered(cx, "reflog-commit-files");
        let file = rendered(cx, "reflog-commit-file-0");
        assert!(
            shows_whole(files, file),
            "the first changed file {file:?} lies whole in its list {files:?}"
        );
        let message = rendered(cx, "reflog-commit-message");
        assert_eq!(
            message.size.height,
            px(110.),
            "the message editor {message:?}"
        );

        let stack = [
            "reflog-entries",
            "reflog-commit-metadata",
            "reflog-commit-message",
            "reflog-commit-files",
            "reflog-recovery-name-label",
        ];
        for pair in stack.windows(2) {
            let (above, below) = (rendered(cx, pair[0]), rendered(cx, pair[1]));
            assert!(
                above.bottom() <= below.top() + px(0.01),
                "{} {above:?} overlaps {} {below:?}",
                pair[0],
                pair[1]
            );
        }

        let form = rendered(cx, "review-reflog-recovery");
        assert!(
            form.bottom() > content.bottom(),
            "the content {content:?} needs no scrolling to show the branch form {form:?}"
        );
        let position = rendered(cx, "reflog-explanation").center();
        cx.simulate_mouse_move(position, None, Modifiers::default());
        draw(cx);
        cx.simulate_event(ScrollWheelEvent {
            position,
            delta: ScrollDelta::Pixels(point(px(0.), px(-2000.))),
            modifiers: Modifiers::default(),
            touch_phase: TouchPhase::Moved,
        });
        draw(cx);
        assert_eq!(rendered(cx, "reflog-browser-content"), content);
        let form = rendered(cx, "review-reflog-recovery");
        assert!(
            shows_whole(content, form),
            "scrolled to its end, the content {content:?} shows the branch form {form:?}"
        );
        let label = rendered(cx, "reflog-recovery-name-label");
        assert!(
            shows_whole(content, label),
            "scrolled to its end, the content {content:?} shows the branch name label {label:?}"
        );
    }

    /// The filled quads the last frame painted inside `area`, in logical
    /// pixels, in paint order.
    fn fills_inside(cx: &mut VisualTestContext, area: Bounds<Pixels>) -> Vec<Bounds<Pixels>> {
        cx.update(|window, _| {
            let scale = window.scale_factor();
            window
                .painted_quads()
                .into_iter()
                .filter(|quad| !quad.background.is_transparent())
                .map(|quad| {
                    let bounds = quad.bounds;
                    Bounds::from_corners(
                        point(
                            px(bounds.left().as_f32() / scale),
                            px(bounds.top().as_f32() / scale),
                        ),
                        point(
                            px(bounds.right().as_f32() / scale),
                            px(bounds.bottom().as_f32() / scale),
                        ),
                    )
                })
                .filter(|fill| shows_whole(area, *fill))
                .collect()
        })
    }

    /// The scrollbar thumb painted in `track`: the filled quad inside it that
    /// is shorter than the track, which a track's own fill is not.
    fn thumb(cx: &mut VisualTestContext, track: Bounds<Pixels>) -> Option<Bounds<Pixels>> {
        let thumbs: Vec<_> = fills_inside(cx, track)
            .into_iter()
            .filter(|fill| fill.size.height < track.size.height - px(0.5))
            .collect();
        assert!(thumbs.len() <= 1, "one thumb in {track:?}, not {thumbs:?}");
        thumbs.first().copied()
    }

    /// Scrolls the element at `selector` by a wheel step far past its end.
    fn scroll_to_end(cx: &mut VisualTestContext, selector: &'static str) {
        let position = rendered(cx, selector).center();
        cx.simulate_mouse_move(position, None, Modifiers::default());
        draw(cx);
        cx.simulate_event(ScrollWheelEvent {
            position,
            delta: ScrollDelta::Pixels(point(px(0.), px(-2000.))),
            modifiers: Modifiers::default(),
            touch_phase: TouchPhase::Moved,
        });
        draw(cx);
        park_pointer(cx);
    }

    /// Scrolls the content to its end through its handle, wherever the
    /// pointer would land.
    fn content_to_end(cx: &mut VisualTestContext, browser: &Entity<ReflogBrowser>) {
        browser.update(cx, |this, _| {
            let end = this.content.scroll().max_offset().y;
            this.content.scroll().set_offset(point(px(0.), -end));
        });
        cx.update(|window, _| window.refresh());
        draw(cx);
    }

    /// The accessibility nodes the kit's Button writes for `entries`, read as
    /// `worktrees`' row test reads them: the test platform never builds a
    /// window's tree, so a probe renders each entry, while it is prepainted,
    /// to the base Button element that writes its node.
    fn entry_nodes(cx: &mut VisualTestContext, entries: Vec<Button>) -> Vec<gpui::accesskit::Node> {
        use gpui::Element as _;
        type Nodes = std::rc::Rc<std::cell::RefCell<Vec<gpui::accesskit::Node>>>;
        /// `element`, if it is of `witness`'s type.
        fn of_type<'a, E: gpui::Element>(
            _witness: &E,
            element: &'a mut AnyElement,
        ) -> Option<&'a mut E> {
            element.downcast_mut()
        }
        fn node(entry: Button, window: &mut Window, cx: &mut App) -> gpui::accesskit::Node {
            let mut outer = RenderOnce::render(entry, window, cx).into_element();
            let (_, mut layout) = outer.request_layout(None, None, window, cx);
            let mut base = (&mut layout as &mut dyn std::any::Any)
                .downcast_mut::<Option<AnyElement>>()
                .and_then(Option::take)
                .expect("the kit's Button renders a base Button");
            let witness = RenderOnce::render(
                gpui_kit::base::Button::new("entry-node-witness"),
                window,
                cx,
            )
            .into_element();
            let element = of_type(&witness, &mut base)
                .expect("a base Button renders the element that writes its node");
            let mut node =
                gpui::accesskit::Node::new(element.a11y_role().expect("the entry has a role"));
            element.write_a11y_info(&mut node);
            node
        }
        struct Probe {
            entries: Vec<Button>,
            nodes: Nodes,
        }
        impl Render for Probe {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                let entries = std::mem::take(&mut self.entries);
                let nodes = self.nodes.clone();
                canvas(
                    move |_, window, cx| {
                        let rendered = entries.into_iter().map(|entry| node(entry, window, cx));
                        nodes.borrow_mut().extend(rendered);
                    },
                    |_, _, _, _| {},
                )
                .size_full()
            }
        }
        let nodes = Nodes::default();
        let (_, probe) = cx.add_window_view({
            let nodes = nodes.clone();
            move |_, _| Probe { entries, nodes }
        });
        probe.update(|window, cx| window.draw(cx).clear(cx));
        nodes.take()
    }

    /// A selected entry paints the selected surface and keeps its toggled
    /// state; an unselected entry paints nothing and reports it untoggled.
    #[gpui::test]
    async fn selected_entry_paints_the_selected_surface(cx: &mut TestAppContext) {
        let fixture = tempfile::tempdir().unwrap();
        let repo = changed_repository(fixture.path(), 3);
        let (_app, browser, cx) = opened_browser(cx, &repo).await;
        select_newest(cx, &browser).await;

        let p = cx.update(|_, cx| palette(cx));
        let selected = gpui_kit::Background::from(Hsla::from(rgb(p.selected)));
        let entry = rendered(cx, "reflog-entry-0");
        let (fills, _) = appearance::painted_button(cx, entry);
        assert_eq!(fills, [selected], "the selected entry paints {fills:?}");
        let other = rendered(cx, "reflog-entry-1");
        let (fills, _) = appearance::painted_button(cx, other);
        assert!(fills.is_empty(), "an unselected entry paints {fills:?}");

        assert!(
            entry_button(
                0,
                "entry".into(),
                true,
                &CutEntries::default(),
                WeakEntity::new_invalid(),
                p
            )
            .is_selected()
        );
        assert!(
            !entry_button(
                1,
                "entry".into(),
                false,
                &CutEntries::default(),
                WeakEntity::new_invalid(),
                p
            )
            .is_selected()
        );
        let nodes = entry_nodes(
            cx,
            vec![
                entry_button(
                    0,
                    "selected entry".into(),
                    true,
                    &CutEntries::default(),
                    WeakEntity::new_invalid(),
                    p,
                ),
                entry_button(
                    1,
                    "other entry".into(),
                    false,
                    &CutEntries::default(),
                    WeakEntity::new_invalid(),
                    p,
                ),
            ],
        );
        let [selected, other] = nodes.as_slice() else {
            panic!("two nodes, not {nodes:?}");
        };
        assert_eq!(selected.role(), Role::Button);
        assert_eq!(selected.label(), Some("selected entry"));
        assert_eq!(selected.toggled(), Some(gpui::accesskit::Toggled::True));
        assert_eq!(other.toggled(), Some(gpui::accesskit::Toggled::False));
    }

    /// In a 461 px tile the dialog keeps inside the window, and at every
    /// width each entry's text starts at the same inset inside the list,
    /// whatever its length: never centred in its entry, and so never left
    /// of the entry, the list or the window.
    #[gpui::test]
    async fn entry_text_starts_inside_the_list_at_every_width(cx: &mut TestAppContext) {
        let fixture = tempfile::tempdir().unwrap();
        let repo = changed_repository(fixture.path(), 3);
        let (_app, browser, cx) = opened_browser(cx, &repo).await;
        let entries = browser.read_with(cx, |this, _| {
            this.page.as_ref().expect("the log loaded").entries.len()
        });
        assert_eq!(entries, 12);
        let mut inset = None;
        for width in [461., 1000.] {
            cx.simulate_resize(size(px(width), px(680.)));
            draw(cx);
            let list = rendered(cx, "reflog-list");
            assert!(
                list.left() >= px(0.) && list.right() <= px(width),
                "the list {list:?} in a {width} px window"
            );
            for index in 0..entries {
                let entry = rendered(cx, format!("reflog-entry-{index}").leak());
                let text = rendered(cx, format!("reflog-entry-text-{index}").leak());
                assert!(
                    text.left() >= list.left() && text.right() <= entry.right(),
                    "entry {index} at {width}: text {text:?} in {entry:?}, list {list:?}"
                );
                let offset = text.left() - entry.left();
                assert_eq!(
                    offset,
                    *inset.get_or_insert(offset),
                    "entry {index}'s text inset at {width}"
                );
            }
        }
    }

    /// At 1000x680 the content's scrollbar paints beside it, in the dialog's
    /// side padding, only while the content overflows: not for the log
    /// alone, which fits, and at rest once a commit with eight changed files
    /// takes the branch form below the fold. Its track starts beside the
    /// content's ring room, where it once painted over the right side of a
    /// focused entry's ring. Scrolled to its end, the thumb moves down.
    #[gpui::test]
    async fn content_scrollbar_cues_the_fold(cx: &mut TestAppContext) {
        let fixture = tempfile::tempdir().unwrap();
        let repo = changed_repository(fixture.path(), 8);
        let (_app, browser, cx) = opened_browser(cx, &repo).await;
        let track = rendered(cx, "reflog-browser-scrollbar");
        assert!(
            fills_inside(cx, track).is_empty(),
            "the content fits, yet its track {track:?} paints {:?}",
            fills_inside(cx, track)
        );

        select_newest(cx, &browser).await;
        assert_eq!(browser.read_with(cx, |this, _| this.changes.len()), 8);
        let content = rendered(cx, "reflog-browser-content");
        let track = rendered(cx, "reflog-browser-scrollbar");
        assert!(
            rendered(cx, "review-reflog-recovery").bottom() > content.bottom(),
            "the content {content:?} overflows"
        );
        let resting = thumb(cx, track).expect("a thumb at rest");
        assert!(
            resting.left() >= content.right(),
            "the thumb {resting:?} lies beside the content {content:?}"
        );
        // The content's right edge is the body's plus the ring room. The
        // track starts beyond it, so its fill covers no control's ring, as it
        // covered the right side of a focused entry's; its thumb stays in the
        // dialog's side padding, the kit's 16 px beyond the body.
        let room = cx.update(|_, cx| appearance::button_ring_room(cx));
        assert!(
            track.left() >= content.right() - px(0.01),
            "the track {track:?} starts beside the content and its ring room {content:?}"
        );
        assert!(
            resting.right() <= content.right() - room + DIALOG_FOOTER_GAP + px(0.01),
            "the thumb {resting:?} stays inside the dialog's side padding"
        );
        // The kit insets its thumb 4 px from the track's ends.
        let inset = px(4.5);
        assert!(
            (resting.top() - content.top()).abs() < inset,
            "at the top of its scroll the thumb {resting:?} starts with the content {content:?}"
        );

        scroll_to_end(cx, "reflog-explanation");
        let form = rendered(cx, "review-reflog-recovery");
        assert!(shows_whole(content, form), "scrolled to its end, {form:?}");
        let scrolled = thumb(cx, track).expect("a thumb at the end");
        assert!(
            scrolled.top() > resting.top() + px(1.),
            "the thumb moves from {resting:?} to {scrolled:?}"
        );
        assert!(
            (scrolled.bottom() - content.bottom()).abs() < inset,
            "at its end the thumb {scrolled:?} ends with the content {content:?}"
        );
    }

    /// Applies `interface` as the interface text size, as Settings does.
    fn set_interface_text_size(cx: &mut VisualTestContext, app: &Entity<GitTurtle>, interface: u8) {
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.settings.interface_text_size = interface;
                let code = app.settings.code_text_size;
                appearance::apply_text_sizes(interface, code, window, cx);
                cx.notify();
            })
        });
        draw(cx);
        park_pointer(cx);
    }

    /// The selectors of the changed-file rows the tests read.
    const FILE_ROWS: [&str; 8] = [
        "reflog-commit-file-0",
        "reflog-commit-file-1",
        "reflog-commit-file-2",
        "reflog-commit-file-3",
        "reflog-commit-file-4",
        "reflog-commit-file-5",
        "reflog-commit-file-6",
        "reflog-commit-file-7",
    ];

    /// The changed-file list of `total` files is `rows` rows tall and shows
    /// whole rows: its last visible row ends whole with it, and any next row
    /// begins below it.
    fn assert_whole_rows(cx: &mut VisualTestContext, rows: usize, total: usize, size: u8) {
        let list = rendered(cx, "reflog-commit-files");
        let first = rendered(cx, "reflog-commit-file-0");
        let row = first.size.height;
        assert!(
            (list.size.height - row * rows as f32).abs() < px(0.01),
            "at {size} pt the list {list:?} is {rows} rows of {row:?}"
        );
        let last = rendered(cx, FILE_ROWS[rows - 1]);
        assert!(
            shows_whole(list, last) && (last.bottom() - list.bottom()).abs() < px(0.01),
            "at {size} pt the last visible row {last:?} ends whole with the list {list:?}"
        );
        if rows < total {
            let next = rendered(cx, FILE_ROWS[rows]);
            assert!(
                next.top() >= list.bottom() - px(0.01),
                "at {size} pt the next row {next:?} begins below the list {list:?}"
            );
        }
    }

    /// At 11, 13 and 18 pt a commit with eight changed files shows the
    /// number of whole rows that fits in 110 px at 13 pt, and a thumb beside
    /// rows that keep clear of it.
    #[gpui::test]
    async fn changed_files_show_whole_rows(cx: &mut TestAppContext) {
        let fixture = tempfile::tempdir().unwrap();
        let repo = changed_repository(fixture.path(), 8);
        let (app, browser, cx) = opened_browser(cx, &repo).await;
        select_newest(cx, &browser).await;
        let row = rendered(cx, "reflog-commit-file-0").size.height;
        let shown = (110. / row.as_f32()).floor() as usize;
        assert!(
            shown < 8,
            "{shown} rows of {row:?} leave eight files to scroll"
        );
        assert_eq!(shown, file_rows_shown());

        let mut heights = Vec::new();
        for size in [11_u8, 13, 18] {
            set_interface_text_size(cx, &app, size);
            // The list lies below the content's fold; bring it into view.
            content_to_end(cx, &browser);
            let list = rendered(cx, "reflog-commit-files");
            let content = rendered(cx, "reflog-browser-content");
            assert!(
                shows_whole(content, list),
                "at {size} pt {list:?} is in view"
            );
            assert_whole_rows(cx, shown, 8, size);
            let strip = Bounds::from_corners(
                point(list.right() - Scrollbar::width(), list.top()),
                list.bottom_right(),
            );
            let thumb = thumb(cx, strip)
                .unwrap_or_else(|| panic!("at {size} pt the list {list:?} paints a thumb"));
            for selector in &FILE_ROWS[..shown] {
                let text = rendered(cx, selector);
                assert!(
                    text.right() <= strip.left() + px(0.01),
                    "at {size} pt {selector} {text:?} keeps clear of the thumb {thumb:?}"
                );
            }
            heights.push(rendered(cx, "reflog-commit-file-0").size.height);
        }
        assert!(
            heights[0] < heights[1] && heights[1] < heights[2],
            "the rows follow the text size: {heights:?}"
        );
        set_interface_text_size(cx, &app, appearance::DEFAULT_INTERFACE_TEXT_SIZE);
    }

    /// With as many files as it shows, the changed-file list is as tall as its
    /// rows and paints no thumb.
    #[gpui::test]
    async fn changed_files_that_fit_paint_no_thumb(cx: &mut TestAppContext) {
        let shown = file_rows_shown();
        let fixture = tempfile::tempdir().unwrap();
        let repo = changed_repository(fixture.path(), shown);
        let (_app, browser, cx) = opened_browser(cx, &repo).await;
        select_newest(cx, &browser).await;
        assert_eq!(browser.read_with(cx, |this, _| this.changes.len()), shown);
        assert_whole_rows(cx, shown, shown, 13);
        content_to_end(cx, &browser);
        let list = rendered(cx, "reflog-commit-files");
        let content = rendered(cx, "reflog-browser-content");
        assert!(shows_whole(content, list), "{list:?} is in view");
        assert!(
            fills_inside(cx, list).is_empty(),
            "a list of {shown} files {list:?} paints {:?}",
            fills_inside(cx, list)
        );
    }

    /// An AT-SPI client reaches only the entries the Reflog draws, and a
    /// focused one wherever it lies. It reached all of them, `showing`, below
    /// the entry list and below the dialog alike, so a screen reader could
    /// find and activate an entry nobody could see. At 640 × 680, as on the
    /// native capture, the list shows about six of its twelve entries; at
    /// 640 × 490 the dialog's content scrolls and its edge cuts the list, so
    /// entries inside the list's own bounds lie below the dialog. AccessKit
    /// still offers the first entry past an edge, for an assistive technology
    /// to move into, which AT-SPI reports visible but not showing. At 640 ×
    /// 400 the content's fold lies above the list, which the dialog draws none
    /// of, and cuts the filter before it: the list reports a pixel beyond a
    /// corner, past the fold, outside which every entry lies, where its
    /// layout kept every entry inside it. So it does at the tallest height
    /// at which the list still starts on that fold or below it, by less than
    /// a quarter of a device pixel (about 640 × 423.5), where a pixel
    /// against the fold lay inside the list and one against its foot
    /// met the entries it runs on with past it. Each is walked with the
    /// filter the content draws before the list, beside which AccessKit keeps
    /// the list as the child past the fold, visible but not showing and
    /// holding no entry, and without it, where AccessKit leaves the list out.
    ///
    /// The test platform builds no accessibility tree, so the test builds the
    /// part GPUI would: the window, each clipping container's node as it
    /// prepainted, the filter at its text, where the Input writes its node,
    /// and the entries the kit's Buttons write, at their bounds. Without a
    /// clipping node an entry is its window's child, as it was.
    #[cfg(target_os = "linux")]
    #[gpui::test]
    async fn assistive_technology_reaches_only_the_entries_drawn(cx: &mut TestAppContext) {
        use crate::native_accessibility::{
            atspi_state_tests::reached, beyond_a_corner, clipping_node,
        };
        use accesskit_atspi_common::State;
        use gpui::accesskit::{Node, NodeId, Rect, Role as A11yRole, Tree, TreeId, TreeUpdate};

        let fixture = tempfile::tempdir().unwrap();
        let repo = changed_repository(fixture.path(), 1);
        let (_app, browser, cx) = opened_browser(cx, &repo).await;
        let count = browser.read_with(cx, |this, _| this.page.as_ref().unwrap().entries.len());
        assert_eq!(count, 12);
        let selectors: Vec<&'static str> = (0..count)
            .map(|index| &*Box::leak(format!("reflog-entry-{index}").into_boxed_str()))
            .collect();
        let scale = cx.update(|window, _| f64::from(window.scale_factor()));
        let physical = |bounds: Bounds<Pixels>| Rect {
            x0: f64::from(f32::from(bounds.left())) * scale,
            y0: f64::from(f32::from(bounds.top())) * scale,
            x1: f64::from(f32::from(bounds.right())) * scale,
            y1: f64::from(f32::from(bounds.bottom())) * scale,
        };

        // The height at which the list starts on the content's fold
        // follows the text's metrics, and how fast the gap closes with it
        // follows the layout: the fold moves with the window's foot and the
        // list with the dialog's centre, about half as far. So it is found by
        // bisection, from 640 × 400, where the list lies below the fold, to
        // the tallest height at which it still does, where it starts on the
        // fold to within a fraction of a device pixel.
        let gap = |cx: &mut VisualTestContext, height: f32| {
            cx.simulate_resize(size(px(640.), px(height)));
            draw(cx);
            f32::from(
                rendered(cx, "reflog-list").top() - rendered(cx, "reflog-browser-content").bottom(),
            )
        };
        let mut below = 400.;
        let start = gap(cx, below);
        assert!(
            start > 0.,
            "at 640 × 400 the list lies {start} px below the content's fold"
        );
        // A height at which the list reaches over the fold.
        let mut over = below + 2. * start + 8.;
        for tries in 0.. {
            if gap(cx, over) < 0. {
                break;
            }
            assert!(
                tries < 8,
                "at 640 × {over} the list still lies below the content's fold"
            );
            (below, over) = (over, over + 2. * (over - below));
        }
        while over - below > 1. / 64. {
            let middle = (below + over) / 2.;
            if gap(cx, middle) >= 0. {
                below = middle;
            } else {
                over = middle;
            }
        }
        let on_fold = below;
        let rest = gap(cx, on_fold);
        assert!(
            rest < 0.25 / scale as f32,
            "at 640 × {on_fold} the list starts {rest} px below the content's fold, not on it"
        );

        for height in [680., 490., 400., on_fold] {
            crate::native_accessibility::forget_clipping_nodes();
            cx.simulate_resize(size(px(640.), px(height)));
            draw(cx);
            park_pointer(cx);
            let window = Bounds::new(
                point(px(0.), px(0.)),
                cx.update(|window, _| window.viewport_size()),
            );
            let list = rendered(cx, "reflog-list");
            let content = rendered(cx, "reflog-browser-content");
            let filter = cx.read(|cx| browser.read(cx).query.read(cx).input_bounds());
            let entries: Vec<Rect> = selectors
                .iter()
                .map(|selector| physical(rendered(cx, selector)))
                .collect();
            // Where the window draws entries: inside the list, the content
            // and the window, measured from layout alone.
            let drawn_area = physical(list.intersect(&content).intersect(&window));
            let drawn: Vec<bool> = entries
                .iter()
                .map(|entry| !entry.intersect(drawn_area).is_empty())
                .collect();
            // AccessKit's rule: a drawn entry, or the first one past an edge.
            let offered: Vec<bool> = (0..count)
                .map(|index| {
                    drawn[index]
                        || (index > 0 && drawn[index - 1])
                        || drawn.get(index + 1).copied().unwrap_or(false)
                })
                .collect();
            assert!(
                offered.contains(&false),
                "at 640 × {height} some entry is out of reach: {drawn:?}"
            );
            if height < 680. {
                assert!(
                    (0..count).any(|index| !offered[index]
                        && !entries[index].intersect(physical(list)).is_empty()
                        && entries[index].intersect(physical(content)).is_empty()),
                    "at 640 × {height} the dialog's edge cuts the list: {list:?} in {content:?}"
                );
            }
            let hidden = physical(list).intersect(physical(content)).is_empty();
            let filter_drawn = !physical(filter).intersect(physical(content)).is_empty();
            if height == on_fold {
                assert!(
                    hidden && filter_drawn,
                    "at 640 × {height} the content draws the filter {filter:?} and none of \
                     the list {list:?} on its fold {content:?}"
                );
            }
            // With `filter_node`, the content holds the filter before the
            // list.
            let tree = |focus: Option<usize>, filter_node: bool| {
                let mut nodes = Vec::new();
                let mut children: Vec<NodeId> = entries
                    .iter()
                    .enumerate()
                    .map(|(index, bounds)| {
                        let id = NodeId(100 + index as u64);
                        let mut node = Node::new(A11yRole::Button);
                        node.set_label(format!("entry {index}"));
                        node.set_bounds(*bounds);
                        nodes.push((id, node));
                        id
                    })
                    .collect();
                for (id, element) in [(2, "reflog-list"), (1, "reflog-browser-content")] {
                    if let Some(mut node) = clipping_node(element) {
                        if id == 1 && filter_node {
                            let mut input = Node::new(A11yRole::TextInput);
                            input.set_label("Filter loaded reflog entries");
                            input.set_bounds(physical(filter));
                            nodes.push((NodeId(3), input));
                            children.insert(0, NodeId(3));
                        }
                        node.set_children(children);
                        nodes.push((NodeId(id), node));
                        children = vec![NodeId(id)];
                    }
                }
                let mut root = Node::new(A11yRole::Window);
                root.set_bounds(physical(window));
                root.set_children(children);
                nodes.push((NodeId(0), root));
                TreeUpdate {
                    nodes,
                    tree: Some(Tree::new(NodeId(0))),
                    tree_id: TreeId::ROOT,
                    focus: focus.map_or(NodeId(0), |index| NodeId(100 + index as u64)),
                }
            };

            let inside = |inner: accesskit_atspi_common::Rect,
                          outer: accesskit_atspi_common::Rect| {
                inner.x >= outer.x
                    && inner.y >= outer.y
                    && inner.x + inner.width <= outer.x + outer.width
                    && inner.y + inner.height <= outer.y + outer.height
            };
            for filter_node in [false, true] {
                let at = format!(
                    "at 640 × {height} {} the filter",
                    if filter_node { "with" } else { "without" }
                );
                let found = reached(tree(None, filter_node));
                for index in 0..count {
                    let name = format!("entry {index}");
                    let entry = found.iter().find(|node| node.name == name);
                    assert_eq!(
                        entry.is_some(),
                        offered[index],
                        "{at}, {name} at {:?} is reached: {}, drawn in {drawn_area:?}: {}",
                        entries[index],
                        entry.is_some(),
                        drawn[index]
                    );
                    // An entry the list draws shows; the one past an edge is
                    // visible but off screen.
                    if let Some(entry) = entry {
                        assert_eq!(
                            (
                                entry.state.contains(State::Visible),
                                entry.state.contains(State::Showing)
                            ),
                            (true, drawn[index]),
                            "{name}: {entry:?}"
                        );
                    }
                }
                let panels: Vec<_> = found
                    .iter()
                    .filter(|node| node.role == accesskit_atspi_common::Role::Panel)
                    .collect();
                // AccessKit keeps the list where the content draws some of
                // it, or as the child past the fold, beside the filter the
                // content draws before it.
                let kept = !hidden || (filter_node && filter_drawn);
                match (&panels[..], kept) {
                    ([content_node, list_node], true) if !hidden => assert!(
                        inside(list_node.extents, content_node.extents)
                            && inside(content_node.extents, found[0].extents),
                        "{at} the list {:?} lies in the content {:?} in the window {:?}",
                        list_node.extents,
                        content_node.extents,
                        found[0].extents
                    ),
                    ([content_node, list_node], true) => {
                        assert!(
                            !inside(list_node.extents, content_node.extents)
                                && list_node.state.contains(State::Visible)
                                && !list_node.state.contains(State::Showing),
                            "{at} the list is kept past the content's fold, visible but not \
                             showing: {list_node:?} in {content_node:?}"
                        );
                        assert!(
                            !found.iter().any(|node| node.name.starts_with("entry ")),
                            "{at} the kept list holds no entry: {found:?}"
                        );
                    }
                    ([content_node], false) => {
                        assert!(inside(content_node.extents, found[0].extents));
                    }
                    _ => panic!(
                        "{at} the content and, kept: {kept}, the list are scroll views: {found:?}"
                    ),
                }

                // Focus keeps an entry reachable wherever it lies in a list
                // AccessKit keeps, showing only where the list draws it.
                let last = count - 1;
                let found = reached(tree(Some(last), filter_node));
                let focused = found
                    .iter()
                    .find(|node| node.name == format!("entry {last}"));
                match focused {
                    Some(focused) => assert!(
                        kept && focused.state.contains(State::Visible | State::Focused)
                            && focused.state.contains(State::Showing) == drawn[last],
                        "{at}: {focused:?}"
                    ),
                    None => assert!(!kept, "{at} the focused entry {last} is reached"),
                }
            }

            // The list reports the part of itself the dialog shows, and where
            // it shows none of it, a pixel beyond a corner, past the content's
            // fold: outside itself and the content, which leaves every entry
            // outside it, those it runs on with past its foot included.
            if hidden {
                let reported = clipping_node("reflog-list")
                    .and_then(|node| node.bounds())
                    .expect("the list clips its entries");
                let (layout, content) = (physical(list), physical(content));
                assert!(
                    beyond_a_corner(reported, layout, scale)
                        && reported.intersect(content).is_empty(),
                    "at 640 × {height} the list {layout:?} the content draws none of, below \
                     {content:?}, reports {reported:?}, not a pixel beyond a corner, outside \
                     the content"
                );
                assert!(
                    entries.iter().any(|entry| entry.y0 >= layout.y1),
                    "at 640 × {height} the list runs on past its foot: {entries:?}"
                );
                assert!(
                    entries
                        .iter()
                        .all(|entry| entry.intersect(reported).is_empty()),
                    "at 640 × {height} every entry lies outside the list's {reported:?}"
                );
            }
        }
    }

    /// While the Reflog is open, an assistive technology reaches the dialog
    /// and nothing of the page it covers. AT-SPI reached History's commit
    /// list and its rows beside the dialog, so a screen reader could browse
    /// and activate commits the dialog covers. Escape still closes the Reflog
    /// and returns focus to the History list that opened it, and History is
    /// in reach again.
    ///
    /// The test platform builds no accessibility tree, so the test builds the
    /// part GPUI would: the page's and the scrolling regions' nodes as their
    /// wrappers wrote them, each in the wrapper it was drawn in, the dialog's
    /// own node as its focus trap writes it, beside the page, and the rows
    /// at their bounds in their lists. It walks the tree through AccessKit's
    /// own filter, and on Linux through the AT-SPI adapter too.
    #[gpui::test]
    async fn an_open_reflog_leaves_the_page_it_covers_out_of_reach(cx: &mut TestAppContext) {
        use crate::native_accessibility::{reachable, written};
        use crate::views::tests::{bounds_of, history_script, history_window_from, settle};
        use gpui::accesskit::{Node, Rect, Role as A11yRole, TreeUpdate};

        let (_fixture, app, cx) = history_window_from(cx, &history_script(40, 3)).await;
        // A dialog otherwise slides in on a wall-clock animation.
        cx.update(|_, cx| cx.set_reduce_motion(true));
        cx.simulate_resize(size(px(1000.), px(680.)));
        cx.update(|window, cx| app.update(cx, |app, cx| app.select_commit(0, window, cx)));
        settle(&app, cx).await;
        let opener = app.read_with(cx, |app, _| app.focus.clone());
        cx.update(|window, cx| window.focus(&opener, cx));
        draw(cx);

        let physical = |cx: &mut VisualTestContext, bounds: Bounds<Pixels>| {
            let scale = cx.update(|window, _| f64::from(window.scale_factor()));
            Rect {
                x0: f64::from(f32::from(bounds.left())) * scale,
                y0: f64::from(f32::from(bounds.top())) * scale,
                x1: f64::from(f32::from(bounds.right())) * scale,
                y1: f64::from(f32::from(bounds.bottom())) * scale,
            }
        };
        // The window's tree as the next frame builds it.
        let tree = |cx: &mut VisualTestContext| -> TreeUpdate {
            written::forget();
            draw(cx);
            let mut tree = written::Assembly::default()
                .written("gitturtle-page", None)
                .written("history-pane", None)
                .written("files-pane", None);
            for index in 0..40 {
                if let Some(bounds) = bounds_of(cx, format!("history-row-{index}")) {
                    let mut row = Node::new(A11yRole::ListBoxOption);
                    row.set_label(format!("History commit {index}"));
                    row.set_bounds(physical(cx, bounds));
                    tree = tree.node(&format!("history-row-{index}"), row, Some("history-pane"));
                }
            }
            if cx.update(|window, cx| window.has_active_dialog(cx)) {
                let mut dialog = Node::new(A11yRole::Dialog);
                dialog.set_label("Reflog");
                dialog.set_modal();
                tree = tree
                    .node("dialog", dialog, None)
                    .written("reflog-browser-content", Some("dialog"))
                    .written("reflog-list", Some("dialog"));
                for index in 0..100 {
                    if let Some(bounds) = bounds_of(cx, format!("reflog-entry-{index}")) {
                        let mut entry = Node::new(A11yRole::Button);
                        entry.set_label(format!("Reflog entry {index}"));
                        entry.set_bounds(physical(cx, bounds));
                        tree =
                            tree.node(&format!("reflog-entry-{index}"), entry, Some("reflog-list"));
                    }
                }
            }
            let window = Bounds::new(
                point(px(0.), px(0.)),
                cx.update(|window, _| window.viewport_size()),
            );
            tree.tree(physical(cx, window), None)
        };
        // The names an assistive technology reaches in `update`, the dialog's
        // with its role.
        let names = |update: TreeUpdate| -> Vec<String> {
            #[cfg(target_os = "linux")]
            let atspi: Vec<String> =
                crate::native_accessibility::atspi_state_tests::reached(update.clone())
                    .into_iter()
                    .map(|node| match node.role {
                        accesskit_atspi_common::Role::Dialog => format!("{} dialog", node.name),
                        _ => node.name,
                    })
                    .collect();
            let names: Vec<String> = reachable(update)
                .into_iter()
                .filter_map(|node| match node.role {
                    A11yRole::Dialog => node.label.map(|label| format!("{label} dialog")),
                    _ => node.label,
                })
                .collect();
            #[cfg(target_os = "linux")]
            assert_eq!(
                atspi
                    .iter()
                    .filter(|name| !name.is_empty())
                    .collect::<Vec<_>>(),
                names
                    .iter()
                    .filter(|name| !name.is_empty())
                    .collect::<Vec<_>>(),
                "AT-SPI reaches what AccessKit's filter keeps"
            );
            names
        };
        let history = |names: &[String]| {
            names
                .iter()
                .filter(|name| name.starts_with("History commit"))
                .count()
        };

        let reached = names(tree(cx));
        assert!(history(&reached) > 2, "History is in reach: {reached:?}");

        let browser = cx.update(|window, cx| {
            let repo = app.read(cx).repository.clone().expect("a repository");
            let browser = cx.new(|cx| ReflogBrowser::new(app.downgrade(), repo, window, cx));
            browser.update(cx, |this, cx| this.refresh(window, cx));
            ReflogBrowser::show(browser.clone(), window, cx);
            browser
        });
        let read = browser.update(cx, |this, _| this.task.take());
        read.expect("the log is read").await;
        let reached = names(tree(cx));
        assert_eq!(
            history(&reached),
            0,
            "nothing of the page is in reach while the Reflog is open: {reached:?}"
        );
        assert!(
            reached.iter().any(|name| name == "Reflog dialog")
                && reached.iter().any(|name| name.starts_with("Reflog entry")),
            "the Reflog and its entries are in reach: {reached:?}"
        );
        // The page holds History's list, the dialog does not lie in it, and
        // it is hidden only while the dialog is open.
        let page = written::get("gitturtle-page").expect("the page writes a node");
        assert!(page.parent.is_none() && page.node.is_hidden(), "{page:?}");
        assert!(
            written::lies_in("history-pane", "gitturtle-page"),
            "History's list lies in the page"
        );
        assert!(
            written::get("reflog-browser-content").is_some()
                && !written::lies_in("reflog-browser-content", "gitturtle-page"),
            "the Reflog is drawn beside the page"
        );

        cx.simulate_keystrokes("escape");
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(400));
        draw(cx);
        assert!(!cx.update(|window, cx| window.has_active_dialog(cx)));
        assert!(
            cx.update(|window, _| opener.is_focused(window)),
            "Escape returns focus to the opener"
        );
        let reached = names(tree(cx));
        assert!(
            history(&reached) > 2,
            "History is in reach again: {reached:?}"
        );
        let page = written::get("gitturtle-page").expect("the page writes a node");
        assert!(!page.node.is_hidden(), "{page:?}");
    }
}
