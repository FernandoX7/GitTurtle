//! Explicit LFS preview downloads with captured pointer/source review.
use crate::native_accessibility::DrawnButton;
use crate::*;
use gitturtle_core::{LfsDownloadPlan, LfsDownloadTarget, WriteCommand};
use gpui_kit::{component::WindowExt, prelude::FluentBuilder};

use crate::native_accessibility::text as label;

pub(super) fn preview_targets(
    content: &Content,
    file: &FileChange,
) -> Vec<(usize, LfsDownloadTarget)> {
    let pointers = match content {
        Content::Images { old, new } => [old.lfs_pointer.clone(), new.lfs_pointer.clone()],
        // Only a side whose object is truly absent: a pointer whose object
        // is local, whatever kept it from showing, has nothing to download.
        Content::Text {
            old,
            new,
            lfs_missing,
            ..
        } => [(old, lfs_missing[0]), (new, lfs_missing[1])]
            .map(|(bytes, missing)| missing.then(|| bytes.as_bytes().to_vec())),
        // Only a side whose object is truly absent: a model's unusable local
        // object, kept as a retryable pointer, has nothing to download either.
        Content::Rich(preview) => [&preview.old, &preview.new].map(|side| {
            side.lfs_missing
                .then(|| side.captured.as_deref())
                .flatten()
                .and_then(|bytes| {
                    gitturtle_preview::detect_lfs_pointer(bytes).map(|_| bytes.to_vec())
                })
        }),
        _ => [None, None],
    };
    pointers
        .into_iter()
        .enumerate()
        .filter_map(|(index, pointer)| {
            let pointer = pointer?;
            let path = if index == 0 {
                file.old_path.clone()
            } else {
                file.new_path.clone()
            }?;
            let blob_oid = if index == 0 {
                file.old_oid.clone()
            } else {
                file.new_oid.clone()
            };
            Some((
                index,
                LfsDownloadTarget {
                    path,
                    blob_oid,
                    pointer,
                },
            ))
        })
        .collect()
}

/// What is wrong with a side's Git LFS object, in the words that side says
/// wherever it stays its pointer: a whole clause, to follow the side's name.
pub(super) fn failure_words(failure: gitturtle_core::LfsObjectFailure) -> &'static str {
    match failure {
        gitturtle_core::LfsObjectFailure::TooLarge => {
            "this Git LFS object is over the 32 MiB preview limit"
        }
        gitturtle_core::LfsObjectFailure::Corrupt => {
            "the local copy of this Git LFS object is damaged"
        }
        gitturtle_core::LfsObjectFailure::StoreUnreadable => {
            "the local Git LFS store could not be read"
        }
    }
}

/// [`failure_words`] for two sides that failed alike.
fn both_failure_words(failure: gitturtle_core::LfsObjectFailure) -> &'static str {
    match failure {
        gitturtle_core::LfsObjectFailure::TooLarge => {
            "both Git LFS objects are over the 32 MiB preview limit"
        }
        gitturtle_core::LfsObjectFailure::Corrupt => {
            "the local copies of both Git LFS objects are damaged"
        }
        gitturtle_core::LfsObjectFailure::StoreUnreadable => {
            "the local Git LFS store could not be read"
        }
    }
}

/// [`failure_words`] opening a sentence, where no side's name comes first.
pub(super) fn failure_opening(failure: gitturtle_core::LfsObjectFailure) -> String {
    let words = failure_words(failure);
    format!("{}{}", words[..1].to_ascii_uppercase(), &words[1..])
}

/// What a side says in its own place, an image's or a model's: the whole
/// statement, with nothing about a download, which could not help.
pub(super) fn failure_sentence(failure: gitturtle_core::LfsObjectFailure) -> String {
    format!("{}.", failure_opening(failure))
}

/// What the failing sides of a text comparison say above it, the sides named
/// `names`, old then new, as one line however many failed; `None` where
/// neither did.
pub(super) fn side_failures_line(
    failed: [Option<gitturtle_core::LfsObjectFailure>; 2],
    names: [&str; 2],
) -> Option<String> {
    let side = |name: &str, failure| {
        format!(
            "{name}: {}, so its pointer is shown.",
            failure_words(failure)
        )
    };
    Some(match failed {
        [None, None] => return None,
        [Some(failure), None] => side(names[0], failure),
        [None, Some(failure)] => side(names[1], failure),
        [Some(old), Some(new)] if old == new => format!(
            "{} and {}: {}, so their pointers are shown.",
            names[0],
            names[1],
            both_failure_words(old)
        ),
        [Some(old), Some(new)] => format!(
            "{}: {}. {}: {}.",
            names[0],
            failure_words(old),
            names[1],
            failure_words(new)
        ),
    })
}

/// The line a comparison's failing sides say above it ([`side_failures_line`]),
/// which a view keeps from frame to frame: as tall as its words where the
/// room allows, and where the comparison needs the room it scrolls inside
/// its own height rather than being cut (DESIGN.md, "Empty and edge states").
pub(super) struct FailureLine {
    /// The element its words are written on.
    id: &'static str,
    pub(super) scroll: ScrollHandle,
    pub(super) focus: FocusHandle,
    /// The comparison last drawn ([`comparison_key`]): another one starts
    /// from its top, even where it says the same words.
    shown: std::cell::Cell<usize>,
}

/// Which comparison `content` is, for as long as it is the one shown.
pub(super) fn comparison_key(content: &Arc<Content>) -> usize {
    Arc::as_ptr(content).addr()
}

impl FailureLine {
    pub(super) fn new(id: &'static str, cx: &mut App) -> Self {
        Self {
            id,
            scroll: ScrollHandle::new(),
            focus: cx.focus_handle(),
            shown: Default::default(),
        }
    }

    /// The line saying `sentence` of the comparison `key`. While it
    /// overflows it is a Tab stop that Up and Down scroll, ringing its own
    /// edge, as the rewritten-series preview's over-tall failure does.
    fn render<V: 'static>(
        &self,
        key: usize,
        sentence: String,
        ring: Option<&crate::focus_ring::FocusRing>,
        window: &Window,
        cx: &mut Context<V>,
    ) -> Div {
        if self.shown.replace(key) != key {
            self.scroll.set_offset(point(px(0.), px(0.)));
        }
        let id = self.id;
        let scroll = &self.scroll;
        let cue = crate::tags::ScrollCue::new(scroll, Pixels::ZERO);
        let overflows = cue.kept();
        let focus = &self.focus;
        let tracked = overflows || focus.is_focused(window);
        let (warning, accent) = (appearance::labels(cx).warning, palette(cx).accent);
        let keys = scroll.clone();
        let edge = ring.and_then(|ring| ring.edge(focus, rgb(accent)));
        cue.list(
            div()
                .on_children_prepainted(cue.observe(|_, _, _| {}))
                .id(SharedString::from(format!("{id}-region")))
                .debug_selector(move || format!("{id}-region"))
                .size_full()
                .overflow_y_scroll()
                .track_scroll(scroll)
                .when(tracked, |region| {
                    region
                        // The handle carries the Tab stop: GPUI ignores the
                        // `tab_stop` of the div that tracks it.
                        .track_focus(&focus.clone().tab_stop(overflows))
                        .role(Role::Group)
                        .aria_label("Git LFS preview warning")
                        .on_key_down(cx.listener(move |_, event: &KeyDownEvent, window, cx| {
                            let down = match event.keystroke.key.as_str() {
                                "up" => false,
                                "down" => true,
                                _ => return,
                            };
                            let line = crate::empty_state::block_height(1, false, window);
                            let at = -keys.offset().y;
                            let end = keys.max_offset().y.max(Pixels::ZERO);
                            let to =
                                (if down { at + line } else { at - line }).clamp(Pixels::ZERO, end);
                            if to != at {
                                keys.set_offset(point(keys.offset().x, -to));
                                cx.notify();
                            }
                            cx.stop_propagation();
                        }))
                })
                .child(
                    label(id, sentence)
                        .px_3()
                        .pt_2()
                        .text_size(appearance::ui_text(12.))
                        .text_color(rgb(warning)),
                ),
            "lfs-failure-line-scrollbar",
        )
        .relative()
        // Gives up its height before anything beside it does.
        .flex_shrink(crate::views::GIVES_FIRST)
        // Outside the scrolled text, so a line that overflows keeps its
        // distance from a Download row beneath it.
        .pb_2()
        .min_h(crate::empty_state::block_height(1, false, window) + appearance::ui_size(16.))
        .children(edge)
    }
}

/// `body` beneath the line [`side_failures_line`] made, if it made one: the
/// body keeps its rows ([`crate::views::comparison_rows_floor`]) and the line
/// gives way first.
pub(super) fn above_comparison<V: 'static>(
    line: &FailureLine,
    key: usize,
    sentence: Option<String>,
    body: AnyElement,
    ring: Option<&crate::focus_ring::FocusRing>,
    window: &Window,
    cx: &mut Context<V>,
) -> AnyElement {
    above_comparison_with_row(line, key, sentence, None, body, ring, window, cx)
}

/// [`above_comparison`] with a Download row, `row` ([`download_row`]),
/// between the line and `body`, or above `body` where no side failed: as in
/// Compare, the row keeps its height, the line gives way first and the body
/// keeps one whole row beside it.
#[allow(clippy::too_many_arguments)]
pub(super) fn above_comparison_with_row<V: 'static>(
    line: &FailureLine,
    key: usize,
    sentence: Option<String>,
    row: Option<AnyElement>,
    body: AnyElement,
    ring: Option<&crate::focus_ring::FocusRing>,
    window: &Window,
    cx: &mut Context<V>,
) -> AnyElement {
    if sentence.is_none() && row.is_none() {
        return body;
    }
    let floor = if row.is_some() {
        crate::views::comparison_floor(1., window)
    } else {
        crate::views::comparison_rows_floor(window)
    };
    div()
        .size_full()
        .flex()
        .flex_col()
        .children(sentence.map(|sentence| line.render(key, sentence, ring, window, cx)))
        .children(row)
        .child(div().flex_1().min_h(floor).child(body))
        .into_any_element()
}

/// What a Download row says, on one line beside its buttons where it fits
/// there ([`sentence_fits_beside`]), otherwise, as beside a failing side's
/// line, in each button's tooltip and accessible description.
pub(crate) const DOWNLOAD_EXPLANATION: &str = "LFS preview objects require an explicit download.";

const DOWNLOAD_BEFORE: &str = "Download Before…";
const DOWNLOAD_AFTER: &str = "Download After…";
const DOWNLOAD_SOURCE: &str = "Download Source…";

#[cfg(test)]
thread_local! {
    /// Whether a Download row's sentence fits beside its buttons, in place
    /// of [`sentence_fits_beside`]'s measurement, for a test of a
    /// measurement that disagrees with layout.
    pub(crate) static SENTENCE_FITS: std::cell::Cell<Option<bool>> =
        const { std::cell::Cell::new(None) };
}

thread_local! {
    static DOWNLOAD_BEFORE_WIDTH: crate::views::TextWidth = const { std::cell::RefCell::new(None) };
    static DOWNLOAD_AFTER_WIDTH: crate::views::TextWidth = const { std::cell::RefCell::new(None) };
    static DOWNLOAD_SOURCE_WIDTH: crate::views::TextWidth = const { std::cell::RefCell::new(None) };
    static EXPLANATION_WIDTH: crate::views::TextWidth = const { std::cell::RefCell::new(None) };
}

/// The label a Download button reads, with the cache of its width.
fn download_label(
    quick_source: bool,
    side: usize,
) -> (
    &'static str,
    &'static std::thread::LocalKey<crate::views::TextWidth>,
) {
    if quick_source {
        (DOWNLOAD_SOURCE, &DOWNLOAD_SOURCE_WIDTH)
    } else if side == 0 {
        (DOWNLOAD_BEFORE, &DOWNLOAD_BEFORE_WIDTH)
    } else {
        (DOWNLOAD_AFTER, &DOWNLOAD_AFTER_WIDTH)
    }
}

/// Whether a Download row `column` wide draws its sentence, which it draws
/// only on one line beside the buttons of all its `sides`, measured from
/// the interface font in the frame the row is drawn. Wrapped inside the
/// pane, at 18 pt in a 461 × 490 window, it took three lines above one
/// button and left the comparison 4.5 px, and above two stacked buttons it
/// pushed Compare's footer under the status bar; wherever it would not fit
/// beside them, what it says moves to the buttons, as it does beside a line.
fn sentence_fits_beside(
    quick_source: bool,
    sides: impl IntoIterator<Item = usize>,
    column: Pixels,
    window: &Window,
) -> bool {
    let rem = window.rem_size();
    let gap = window.pixel_snap(rem * 0.5);
    let room = column - window.pixel_snap(rem * 0.75) * 2. + px(1e-3);
    let sentence =
        crate::views::cached_text_width(&EXPLANATION_WIDTH, DOWNLOAD_EXPLANATION, window, || {
            crate::views::line_widths(&[(DOWNLOAD_EXPLANATION, appearance::ui_text(12.))], window)
        });
    let buttons = sides
        .into_iter()
        .map(|side| {
            let (label, cache) = download_label(quick_source, side);
            gap + crate::labelled_button_width(label, cache, window)
        })
        .sum::<Pixels>();
    crate::views::laid_out_text_width(sentence, window) + buttons <= room
}

/// The row offering a download of each side among `targets`, whose object
/// is missing, where `quick_source` calls the one side Source, beneath a
/// failing side's line if `line_above`, in a column `column` wide. It looks
/// refused while a write runs (`refused`, and `dimmed` once the write looks
/// busy, `workspace::BusyLook`), and a press asks `download` for the side's
/// target. Its frames stand on the pane's text edge, the line's above it: a
/// 0.75 rem bar holds neither a ghost Button's pull nor its ring (DESIGN.md,
/// "One left edge"). Compare and the rewritten-series review draw it alike.
pub(crate) fn download_row(
    targets: Vec<(usize, LfsDownloadTarget)>,
    quick_source: bool,
    line_above: bool,
    column: Pixels,
    (refused, dimmed): (bool, bool),
    download: impl Fn(LfsDownloadTarget, &mut Window, &mut App) + 'static,
    window: &Window,
) -> AnyElement {
    if targets.is_empty() {
        return div().into_any_element();
    }
    let fits = sentence_fits_beside(
        quick_source,
        targets.iter().map(|(side, _)| *side),
        column,
        window,
    );
    #[cfg(test)]
    let fits = SENTENCE_FITS.get().unwrap_or(fits);
    let sentence_moves = line_above || !fits;
    let download = std::rc::Rc::new(download);
    div()
        .flex_none()
        .px_3()
        // The line above already keeps this row's distance.
        .when(!line_above, |row| row.pt_2())
        .pb_2()
        .flex()
        .flex_wrap()
        .items_center()
        .gap_2()
        // Beside a line, or where it would not fit on one line beside the
        // buttons, the row gives the comparison its height: what this says
        // moves to each button's tooltip and description.
        .when(!sentence_moves, |row| {
            row.child(
                label("lfs-preview-download-explanation", DOWNLOAD_EXPLANATION)
                    .debug_selector(|| "lfs-preview-download-explanation".into())
                    .whitespace_nowrap()
                    // No wider than the row, and cut there with an ellipsis:
                    // should the measurement that stood it here disagree
                    // with layout, it ends inside the pane rather than
                    // running past it.
                    .max_w_full()
                    .overflow_hidden()
                    .text_ellipsis()
                    .text_size(appearance::ui_text(12.)),
            )
        })
        .children(targets.into_iter().map(|(side, target)| {
            let download = download.clone();
            div()
                .debug_selector(move || format!("download-lfs-side-{side}"))
                .child(
                    button(
                        ("download-lfs-side", side),
                        download_label(quick_source, side).0,
                        "pull",
                        false,
                    )
                    .when(sentence_moves, |button| {
                        crate::branch_actions::Described(button)
                            .aria_description(DOWNLOAD_EXPLANATION)
                            .0
                            .tooltip(DOWNLOAD_EXPLANATION)
                    })
                    // `open_lfs_download` refuses while a write runs.
                    .map(|button| {
                        crate::workspace::refusing(
                            button,
                            || format!("download-lfs-side-{side}"),
                            refused,
                            dimmed,
                        )
                    })
                    .on_click(move |_, window, cx| download(target.clone(), window, cx))
                    .node_recorded(),
                )
        }))
        .into_any_element()
}

impl GitTurtle {
    /// What the failing sides of the shown text say above it, where they say
    /// it nowhere else: Rendered and Diagrams say it in the side's own place,
    /// so the line would say it twice and take the comparison's room. A
    /// download could not help, so none of them offers one.
    pub(super) fn lfs_failure_sentence(&self) -> Option<String> {
        let Some(Content::Text {
            lfs_failed,
            markdown,
            diagrams,
            ..
        }) = self.content.as_deref()
        else {
            return None;
        };
        let own_words = (self.text_mode == TextMode::Markdown && markdown.is_some())
            || (self.text_mode == TextMode::Diagrams && diagrams.is_some());
        if own_words {
            return None;
        }
        let names = if self.is_quick_source() {
            ["Source", "Source"]
        } else {
            ["Before", "After"]
        };
        side_failures_line(*lfs_failed, names)
    }

    /// The line above the comparison, where the failing sides say it there:
    /// a child of Compare's own column, which gives way first; a Download
    /// row beside it keeps its height and the comparison keeps one row.
    pub(super) fn render_lfs_failure_line(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let sentence = self.lfs_failure_sentence()?;
        let ring = self.focus_ring.clone();
        let key = self.content.as_ref().map_or(0, comparison_key);
        Some(
            self.lfs_failure_line
                .render(key, sentence, Some(&ring), window, cx),
        )
    }

    fn lfs_download_targets(&self) -> Vec<(usize, LfsDownloadTarget)> {
        let Some(file) = self.selected_file.and_then(|index| self.files.get(index)) else {
            return Vec::new();
        };
        self.content
            .as_deref()
            .map_or_else(Vec::new, |content| preview_targets(content, file))
    }

    /// Whether the shown file has a side to download: its row keeps its
    /// height, so the comparison gives way to it rather than cover it.
    pub(super) fn lfs_download_offered(&self) -> bool {
        !self.lfs_download_targets().is_empty()
    }

    pub(super) fn render_lfs_download_actions(
        &self,
        column: Pixels,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let owner = cx.entity().downgrade();
        download_row(
            self.lfs_download_targets(),
            self.is_quick_source(),
            self.lfs_failure_sentence().is_some(),
            column,
            (self.operation_busy.is_some(), self.operation_shown().is_some()),
            move |target, window, cx| {
                let _ = owner.update(cx, |this, cx| this.open_lfs_download(target, window, cx));
            },
            window,
        )
    }

    pub(super) fn open_lfs_download(
        &mut self,
        target: LfsDownloadTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.operation_busy.is_some() {
            return;
        }
        let Some(repo) = self.repository.clone() else {
            return;
        };
        let owner = cx.entity().downgrade();
        let remote = self.remote_name.read(cx).value().trim().to_owned();
        let remotes = self
            .remotes
            .iter()
            .map(|remote| remote.name.clone())
            .collect();
        let form =
            cx.new(|cx| LfsDownloadForm::new(owner, repo, target, remote, remotes, window, cx));
        form.update(cx, |this, cx| this.prepare(window, cx));
        window.open_alert_dialog(cx, move |dialog, window, cx| {
            let submit = form.clone();
            let cancel = form.clone();
            let shown = form.clone();
            // In the dialog's content, as the profile form is: the form takes
            // what the cap leaves it and scrolls the rest, revealing the
            // control Tab moves onto, with the ring's room below it, which
            // the footer's gap gives back. In the dialog's own scrolling
            // body, at 18 pt in a 461 × 490 window, the form was taller than
            // what the cap left, so that body cut Read configured source's
            // ring at its foot and revealed nothing.
            dialog
                .title(native_accessibility::dialog_title(
                    "lfs-download-title",
                    "Download an LFS preview",
                ))
                .width(appearance::dialog_width(px(650.), window))
                .max_h(appearance::dialog_max_height(window))
                .gap(crate::tags::DIALOG_FOOTER_GAP - appearance::button_ring_room(cx))
                .content(move |content, _, _| content.min_h_0().child(shown.clone()))
                .footer(crate::tags::dialog_footer(
                    "Review download",
                    Some("Cancel"),
                ))
                .on_ok(move |_, window, cx| {
                    submit.update(cx, |this, cx| this.review(window, cx));
                    false
                })
                .on_cancel(move |_, _, cx| {
                    cancel.update(cx, |this, _| this.cancel());
                    true
                })
        });
    }
}

struct LfsDownloadForm {
    owner: WeakEntity<GitTurtle>,
    repo: GitRepository,
    target: LfsDownloadTarget,
    remote: Entity<InputState>,
    remotes: Vec<String>,
    reader: operations::SerialExecutor,
    task: Option<Task<()>>,
    cancellation: gitturtle_core::HistoryCancellation,
    plan: Option<LfsDownloadPlan>,
    pending: bool,
    error: Option<String>,
    _subscription: Subscription,
    /// The form's scroll, which reveals the control Tab moves onto.
    body: crate::focus_reveal::FocusReveal,
}
impl Drop for LfsDownloadForm {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}
impl LfsDownloadForm {
    fn new(
        owner: WeakEntity<GitTurtle>,
        repo: GitRepository,
        target: LfsDownloadTarget,
        remote: String,
        remotes: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let remote = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(remote)
                .placeholder("Configured remote name")
        });
        let subscription = cx.subscribe_in(
            &remote,
            window,
            |this: &mut Self, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::Change) {
                    this.cancel();
                    this.plan = None;
                }
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.prepare(window, cx);
                }
                cx.notify();
            },
        );
        Self {
            owner,
            repo,
            target,
            remote,
            remotes,
            reader: operations::SerialExecutor::new("lfs-preview-source"),
            task: None,
            cancellation: Default::default(),
            plan: None,
            pending: false,
            error: None,
            _subscription: subscription,
            body: crate::focus_reveal::FocusReveal::new(crate::focus_reveal::Trigger::Keyboard),
        }
    }
    fn cancel(&mut self) {
        self.cancellation.cancel();
        self.task = None;
        self.pending = false;
    }
    fn prepare(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.cancel();
        self.plan = None;
        self.error = None;
        let remote = self.remote.read(cx).value().trim().to_owned();
        if remote.is_empty() {
            crate::tags::set_error_line(
                &mut self.error,
                "Choose a configured remote for this LFS object. Add a remote from the branch menu if this repository has none.",
                window,
                cx,
            );
            cx.notify();
            return;
        }
        self.pending = true;
        self.cancellation = Default::default();
        let cancellation = self.cancellation.clone();
        let repo = self.repo.clone();
        let target = self.target.clone();
        let response = self.reader.submit_read(move || {
            gitturtle_core::run_cancellable_inspection(cancellation, || {
                repo.lfs_download_plan(&target, &remote)
            })
        });
        self.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = response.await.unwrap_or_else(|_| {
                Err(anyhow::anyhow!(
                    "LFS source inspection was interrupted. Read the source again."
                ))
            });
            let _ = this.update_in(cx, |this, window, cx| {
                this.pending = false;
                match result {
                    Ok(plan) => this.plan = Some(plan),
                    Err(error) => crate::tags::set_error_line(
                        &mut this.error,
                        format!("{error:#}"),
                        window,
                        cx,
                    ),
                };
                cx.notify();
            });
        }));
        cx.notify();
    }
    fn review(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending {
            return;
        }
        let Some(plan) = self.plan.clone() else {
            self.prepare(window, cx);
            return;
        };
        let _ = self.owner.update(cx, |owner, cx| {
            if owner.path.as_deref() != Some(this_path(&self.repo)) || owner.operation_busy.is_some() { crate::tags::set_error_line(&mut self.error, "The repository changed or another operation started. Reopen this download after it finishes.", window, cx); cx.notify(); return; }
            window.close_dialog(cx);
            owner.confirm_git_write("Download selected LFS object".into(), format!("File: {}\nObject: sha256:{}\nSize: {}\nRemote: {}\n{}\n\nThis explicit transfer fetches only this object's pointer. Git LFS uses your configured credentials and SSH host verification. The download can be cancelled from the operation bar.\n\nGitTurtle verifies the object's SHA-256 and size before previewing it. Working files and the index remain unchanged; no checkout, smudge, prune, or push is performed.", plan.target.path.display(), plan.oid, crate::counted(plan.size, "byte", "bytes"), plan.remote, plan.source), "Download object", WriteCommand::DownloadLfs(Arc::new(plan)), window, cx);
        });
    }
}
fn this_path(repo: &GitRepository) -> &std::path::Path {
    repo.path()
}

impl Render for LfsDownloadForm {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let pointer = gitturtle_preview::detect_lfs_pointer(&self.target.pointer);
        // The remotes and Read configured source, ghost Buttons at the
        // dialog's text edge, stand the label or icon of each line's first
        // on that edge, their frames their pull before it (DESIGN.md, "One
        // left edge"); the form keeps the pull with the ring's room on its
        // left, and the ring's room on its right, given back through its
        // margins, so its clip cuts neither them nor the field's ring. It
        // keeps the ring's room above and below its controls too, given
        // back through its margin, and a wrapper keeps the room below them
        // inside the dialog's clip.
        let room = appearance::button_ring_room(cx);
        let lead = crate::ghost_pull(window) + room;
        // A control Tab moves onto scrolls into view with its ring's room
        // (`focus_reveal`).
        self.body.reveal(window, cx);
        let field = self.remote.read(cx).focus_handle(cx);
        let read = self.body.focus("read-lfs-source", cx);
        let remotes: Vec<_> = (0..self.remotes.len().min(12))
            .map(|index| self.body.focus(("lfs-source-remote", index), cx))
            .collect();
        let cue = crate::tags::ScrollCue::revealing(&self.body, room);
        // At most 450 px, and never more than the whole dialog's cap, its
        // title and footer included: a loose bound only. What holds the
        // body to the room the cap leaves it is the `min_h_0` chain from
        // the dialog's content down to this list, which shrinks it to that
        // room and scrolls the rest.
        let cap = px(450.).min(appearance::dialog_max_height(window));
        div().flex().flex_col().min_h_0().pb(room).child(cue.list(div().on_children_prepainted(cue.observe(self.body.observe())).id("lfs-download-review-content").flex().flex_col().gap_3().max_h(cap + room * 2.).min_h_0().overflow_y_scroll().track_scroll(self.body.scroll()).p(room).m(-room).pl(lead).ml(-lead)
            .child(label("lfs-download-file", format!("File: {}", self.target.path.display())).text_size(appearance::ui_text(13.)))
            .when_some(pointer.as_ref(), |element, pointer| element.child(label("lfs-download-object", format!("sha256:{}\n{}", pointer.oid, crate::counted(pointer.size, "byte", "bytes"))).text_size(appearance::ui_text(12.))))
            .child(label("lfs-download-source-label", "Choose the configured source").text_size(appearance::ui_text(12.)))
            // Reported from a wrapper of its own: a list that tracks its
            // scroll hands its own children's bounds to the scroll handle,
            // not to its `on_children_prepainted`.
            .child(div().flex().flex_col().on_children_prepainted(self.body.control(field, room)).child(Input::new(&self.remote).aria_label("Git LFS download remote")))
            .child(crate::edge_row(window).on_children_prepainted(self.body.controls(remotes.iter().cloned().map(Some).collect(), room)).children(self.remotes.iter().take(12).zip(&remotes).enumerate().map(|(index, (name, focus))| { let name = name.clone(); crate::edge_button(("lfs-source-remote", index), name.clone()).track_focus(focus).debug_selector(move || format!("lfs-source-remote-{index}")).disabled(self.pending).on_click(cx.listener(move |this, _, window, cx| { this.remote.update(cx, |input, cx| input.set_value(name.clone(), window, cx)); this.prepare(window, cx); })) })))
            .child(crate::edge_row(window).on_children_prepainted(self.body.controls(vec![Some(read.clone())], room)).child(button("read-lfs-source", "Read configured source", "refresh", false).track_focus(&read).debug_selector(|| "read-lfs-source".into()).flex_shrink(1.).disabled(self.pending).on_click(cx.listener(|this, _, window, cx| this.prepare(window, cx)))))
            .when(self.pending, |element| element.child(label("lfs-source-loading", "Resolving the selected pointer, Git LFS tooling, and configured source…").text_size(appearance::ui_text(12.))))
            .when_some(self.plan.as_ref(), |element, plan| element.child(label("lfs-source-resolved", plan.source.clone()).text_size(appearance::ui_text(12.))))
            .children(self.error.as_ref().map(|error| crate::tags::error_line("lfs-download-error", error, Some(appearance::ui_text(12.)), window, cx).text_color(rgb(p.warning))))
            .child(label("lfs-download-limit", "Normal browsing stays passive. This action is limited to one object up to 32 MiB; decoder and memory limits still apply. No credentials are stored in this form.").text_size(appearance::ui_text(12.)).text_color(rgb(p.muted))), "lfs-download-scrollbar").min_h_0())
    }
}
