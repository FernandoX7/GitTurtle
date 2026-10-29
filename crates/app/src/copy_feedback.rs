//! Confirmation of explicit copy actions, and of the other actions that
//! confirm on the control they came from ([`confirm`]): opening captured
//! bytes in another application (`system_preview`), and the Git writes a
//! control asks for, which confirm once Git reports success (`workspace`'s
//! `write_from`: a file row's Stage or Unstage, Stage all and Unstage all,
//! Fetch, Pull, Push, Create and Switch, and the branch picker's switch and
//! tracking-branch creation).
//!
//! Its announcement node also says what no control confirms ([`announce`]):
//! the GitHub panel's notice, as the panel opens with it and as it changes,
//! from wherever its body has scrolled it. That leaves the control reading
//! its confirmation, and the time it reads it, alone.
//!
//! Every copy a person asks for goes through [`copy`]: it writes the
//! clipboard, marks the copy button it came from, if any, and posts one polite
//! "Copied ‹what›" announcement. For [`RESTORE_AFTER`] the marked button,
//! drawn by [`copy_button`], reads "Copied" beside a check icon instead of its
//! content; an icon-only button shows the check and the tooltip "Copied". It
//! then reverts. A repeated copy restarts the time and is announced again.
//! The button keeps its bounds and its accessible name throughout, and
//! nothing animates, so reduced motion needs nothing more. Another action's
//! button, drawn by [`feedback_button`] in the shared frame or by
//! [`confirming`] in a frame of its own (the action bar's 34 px Fetch, Pull
//! and Push), reads its own word ("Opened", "Fetched", "Staged") the same
//! way, or, where a layout depends on its width, the word alone within its
//! content's room ([`Fit::Within`]). A control that names something it
//! changes, as the branch picker names the branch, keeps its content and
//! trades only its icon for the check ([`confirmed`]).
//!
//! The mark is one application global: the latest confirmation, its window
//! and its generation. A newer one replaces it, so at most one button reads
//! Copied or Opened, and dropping the older restore timer cancels it; the
//! generation check keeps a timer from clearing a newer mark. It holds no
//! repository or inspection state, so tab switches and restored inspections
//! carry nothing.
//!
//! A [`Mark`] names both the button and what it copies, such as the commit a
//! Hash button copies, so Copied stays where the copy happened: Hash drawn
//! for the next selected commit, with the same id in the same place, is at
//! rest, while the copied commit's Hash, or any button that leaves the screen
//! and returns before the time is up, still reads Copied.
//!
//! A text selection copied with the edit command (the editors, Split's
//! sides, the commit message's selectable text) stays silent, as native text
//! fields are: it is the standard Copy, not a copy action.
use crate::native_accessibility::LiveRegion as _;
use crate::*;
use gpui_kit::prelude::FluentBuilder;
use std::{
    hash::{DefaultHasher, Hash, Hasher},
    time::Duration,
};

/// How long a copy button reads "Copied".
pub(crate) const RESTORE_AFTER: Duration = Duration::from_millis(1500);
/// What a copied button reads, after its check icon.
pub(crate) const COPIED: &str = "Copied";

/// An action whose button confirms it in place ([`feedback_button`]): the
/// icon the button leads with at rest, the word it reads after the check
/// once [`confirm`] marks it, and the name tests find that state by
/// (`<id>/rest`, `<id>/<state>`).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Action {
    pub(crate) icon: &'static str,
    pub(crate) done: &'static str,
    pub(crate) state: &'static str,
}

/// A copy: the copy icon, then Copied.
pub(crate) const COPY: Action = Action {
    icon: "copy",
    done: COPIED,
    state: "copied",
};

#[derive(Default)]
struct Feedback {
    /// Counts confirmations; each one is a fresh announcement node.
    generation: usize,
    latest: Option<Confirmed>,
    /// Restores `latest`; replacing it cancels the previous one's timer.
    restore: Option<Task<()>>,
    /// The latest announcement without a confirmation ([`announce`]), which
    /// leaves `latest`'s mark and its timer alone.
    spoken: Option<Confirmed>,
    /// Ends `spoken`; replacing it cancels the previous one's timer.
    quiet: Option<Task<()>>,
}
impl Global for Feedback {}

struct Confirmed {
    generation: usize,
    window: AnyWindowHandle,
    mark: Option<Mark>,
    announcement: SharedString,
}

/// A copy button and what it copies, which its Copied mark belongs to. The
/// button draws with it ([`copy_button`]) and its click copies with it
/// ([`copy`]), so the two cannot disagree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Mark {
    control: ElementId,
    /// A hash of the content's identity, such as a commit id, or a byte path
    /// and its side, so the mark holds no copy of the content.
    subject: u64,
}

impl Mark {
    /// The mark of the copy button `control` while it copies the content
    /// `subject` identifies: a commit id for Hash and Copy message, a path
    /// for a path, the path and side for a comparison side's source.
    pub(crate) fn new(control: impl Into<ElementId>, subject: impl Hash) -> Self {
        let mut hasher = DefaultHasher::new();
        subject.hash(&mut hasher);
        Self {
            control: control.into(),
            subject: hasher.finish(),
        }
    }

    /// The mark of one of several `name` controls in a list, one per
    /// `subject`, such as a working file's Stage: the control's id carries
    /// the subject's hash, so each row's control has its own id however the
    /// rows are ordered, and a confirmation made for a subject finds its row
    /// after the list is read again.
    pub(crate) fn per_subject(name: &'static str, subject: impl Hash) -> Self {
        let mut hasher = DefaultHasher::new();
        subject.hash(&mut hasher);
        let subject = hasher.finish();
        Self {
            control: ElementId::NamedInteger(name.into(), subject),
            subject,
        }
    }

    /// The id of the button this mark belongs to.
    pub(crate) fn control(&self) -> &ElementId {
        &self.control
    }
}

/// Puts `text` on the clipboard and confirms it: `mark`, the copy button that
/// asked and what it copied (`None` for a keyboard copy without a button),
/// reads Copied, and assistive technology hears "Copied `what`", where `what`
/// names the content, such as "commit hash" or "file path".
pub(crate) fn copy(
    window: &mut Window,
    cx: &mut App,
    mark: Option<Mark>,
    what: &str,
    text: String,
) {
    cx.write_to_clipboard(ClipboardItem::new_string(text));
    confirm(window, cx, mark, format!("{COPIED} {what}"));
}

/// Confirms an action where it happened: `mark`, the button that asked and
/// what it acted on, reads its [`Action`]'s word for [`RESTORE_AFTER`], and
/// assistive technology hears `announcement` once, politely.
pub(crate) fn confirm(
    window: &mut Window,
    cx: &mut App,
    mark: Option<Mark>,
    announcement: impl Into<SharedString>,
) {
    let feedback = cx.default_global::<Feedback>();
    feedback.generation = feedback.generation.wrapping_add(1);
    let generation = feedback.generation;
    feedback.latest = Some(Confirmed {
        generation,
        window: window.window_handle(),
        mark,
        announcement: announcement.into(),
    });
    // The whole window redraws, so a button inside a dialog or a child view
    // reads its confirmation without its owner being notified.
    window.refresh();
    let restore = cx.spawn(async move |cx| {
        cx.background_executor().timer(RESTORE_AFTER).await;
        cx.update(|cx| restore(generation, cx));
    });
    cx.global_mut::<Feedback>().restore = Some(restore);
}

/// Ends confirmation `generation`'s mark and announcement unless a newer one
/// replaced it, redrawing the window it was made in.
fn restore(generation: usize, cx: &mut App) {
    let feedback = cx.global_mut::<Feedback>();
    if feedback
        .latest
        .as_ref()
        .is_none_or(|latest| latest.generation != generation)
    {
        return;
    }
    if let Some(confirmed) = feedback.latest.take() {
        // The window may have closed since.
        let _ = confirmed.window.update(cx, |_, window, _| window.refresh());
    }
}

/// Announces `announcement` once, politely, from the window's announcement
/// node, as [`confirm`] does, without confirming on any control: a control
/// that reads its confirmation keeps it for the rest of its time, where a
/// confirmation without a mark would end it. The node goes after
/// [`RESTORE_AFTER`], or when a newer announcement takes its place.
pub(crate) fn announce(window: &mut Window, cx: &mut App, announcement: impl Into<SharedString>) {
    let feedback = cx.default_global::<Feedback>();
    feedback.generation = feedback.generation.wrapping_add(1);
    let generation = feedback.generation;
    feedback.spoken = Some(Confirmed {
        generation,
        window: window.window_handle(),
        mark: None,
        announcement: announcement.into(),
    });
    window.refresh();
    let quiet = cx.spawn(async move |cx| {
        cx.background_executor().timer(RESTORE_AFTER).await;
        cx.update(|cx| quiet(generation, cx));
    });
    cx.global_mut::<Feedback>().quiet = Some(quiet);
}

/// Ends announcement `generation` unless a newer one replaced it, redrawing
/// the window it was made in.
fn quiet(generation: usize, cx: &mut App) {
    let feedback = cx.global_mut::<Feedback>();
    if feedback
        .spoken
        .as_ref()
        .is_none_or(|spoken| spoken.generation != generation)
    {
        return;
    }
    if let Some(spoken) = feedback.spoken.take() {
        // The window may have closed since.
        let _ = spoken.window.update(cx, |_, window, _| window.refresh());
    }
}

/// Whether `mark`'s control reads its confirmation now.
pub(crate) fn confirmed(mark: &Mark, cx: &App) -> bool {
    cx.try_global::<Feedback>()
        .and_then(|feedback| feedback.latest.as_ref())
        .is_some_and(|latest| latest.mark.as_ref() == Some(mark))
}

/// A copy button with the shared `button` frame and `mark`'s control id,
/// which reads Copied while [`copy`] marks `mark`, the same button copying
/// the same content; the caller's click handler calls [`copy`] with
/// `Some(mark)`.
///
/// A labelled button draws its content, the copy icon and `label`, and the
/// copied content, the check icon and "Copied", in one grid cell, showing one
/// and hiding the other, so it is as wide as the wider of the two in both
/// states. Both start at its leading inset, so the check replaces the copy
/// icon in place, the label and "Copied" start at one x, and nothing moves.
/// Every copy button carries the copy icon: a label alone gained the check
/// where no icon was and shifted 22 px when copied. It is named `label`, and
/// `tooltip` (if not empty) stays.
/// An icon-only button (an empty `label`) keeps its 28 px square, swaps its
/// icon for the check and its tooltip for "Copied", and is named as `button`
/// names one until the caller names it.
pub(crate) fn copy_button(
    mark: Mark,
    label: impl Into<SharedString>,
    tooltip: impl Into<SharedString>,
    cx: &App,
) -> Button {
    feedback_button(mark, COPY, label, tooltip, cx)
}

/// A button that confirms `action` in place as [`copy_button`] confirms a
/// copy, with the same frame, cells, naming and tooltip: at rest it draws the
/// action's icon and `label`, and while [`confirm`] marks `mark` the check and
/// the action's word in the same bounds.
pub(crate) fn feedback_button(
    mark: Mark,
    action: Action,
    label: impl Into<SharedString>,
    tooltip: impl Into<SharedString>,
    cx: &App,
) -> Button {
    let label = label.into();
    let frame = button_frame(mark.control.clone(), &label, action.icon, false);
    confirming_with(
        frame,
        appearance::ui_size(6.),
        Fit::Reserve,
        mark,
        action,
        label,
        Some(tooltip.into()),
        cx,
    )
}

/// [`feedback_button`] with no tooltip at all, not even its confirmation's
/// word: a control a list moved under a pointer that has not moved since
/// shows none until the pointer moves (`motion::StillPointer`). An icon-only
/// control given an empty tooltip still says its confirmation in one, so
/// gamma's Stage, confirmed in Staged as its flight began under the still
/// pointer, said "Staged" there.
pub(crate) fn still_feedback_button(
    mark: Mark,
    action: Action,
    label: impl Into<SharedString>,
    cx: &App,
) -> Button {
    let label = label.into();
    let frame = button_frame(mark.control.clone(), &label, action.icon, false);
    confirming_with(
        frame,
        appearance::ui_size(6.),
        Fit::Reserve,
        mark,
        action,
        label,
        None,
        cx,
    )
}

/// How a labelled control draws its confirmation in its bounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Fit {
    /// The check in the icon's place, then the word where the label starts,
    /// as Copied: the control is as wide as the wider of its content and
    /// that, at rest too.
    Reserve,
    /// The word alone, from where the icon starts, within the content's own
    /// width, for a control whose width a layout depends on: the action
    /// bar's Fetch, Pull and Push, whose rest widths decide where the bar
    /// wraps and where the compact chrome begins. "Fetched" is narrower than
    /// the icon, its gap and "Fetch" together, so nothing at rest grows.
    Within,
}

/// [`feedback_button`]'s content, naming and tooltip in `frame`, a Button
/// with `mark`'s control id and its own size, padding and variant, such as
/// the action bar's 34 px Fetch, Pull and Push; `gap` is the space it keeps
/// between its icon and its label, so the content sits where the frame's own
/// icon and label would, and `fit` how a label's confirmation fits.
#[allow(clippy::too_many_arguments)]
pub(crate) fn confirming(
    frame: Button,
    gap: impl Into<DefiniteLength>,
    fit: Fit,
    mark: Mark,
    action: Action,
    label: impl Into<SharedString>,
    tooltip: impl Into<SharedString>,
    cx: &App,
) -> Button {
    confirming_with(
        frame,
        gap,
        fit,
        mark,
        action,
        label,
        Some(tooltip.into()),
        cx,
    )
}

/// [`confirming`], with no tooltip at all where `tooltip` is `None`.
#[allow(clippy::too_many_arguments)]
fn confirming_with(
    frame: Button,
    gap: impl Into<DefiniteLength>,
    fit: Fit,
    mark: Mark,
    action: Action,
    label: impl Into<SharedString>,
    tooltip: Option<SharedString>,
    cx: &App,
) -> Button {
    let copied = confirmed(&mark, cx);
    let id = mark.control.clone();
    let label = label.into();
    let labelled = !label.is_empty();
    let gap = gap.into();
    let selector = id.clone();
    let mut button = frame.debug_selector(move || selector.to_string());
    let cell = |icon: Option<Icon>, text: SharedString, shown: bool, state: &'static str| {
        let (probe, text_probe) = (id.clone(), id.clone());
        // Both contents start at the leading edge, so the check takes the
        // icon's place and the narrower one's spare width trails it.
        div()
            .row_start(1)
            .col_start(1)
            .flex()
            .justify_start()
            .when(!shown, |cell| cell.invisible())
            .child(
                // Drawn only while its cell is shown, so tests find
                // `<id>/rest` or `<id>/copied` (the action's state), never
                // both.
                div()
                    .debug_selector(move || format!("{probe}/{state}"))
                    .flex()
                    .items_center()
                    .gap(gap)
                    .children(icon)
                    .when(!text.is_empty(), |content| {
                        let key = format!("{text_probe}/{state}/text");
                        let probe = key.clone();
                        content.child(
                            div()
                                .debug_selector(move || key)
                                .min_w_0()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .line_height(relative(1.))
                                .child(text)
                                // In tests, the colour the text is painted in.
                                .children(appearance::text_probe(move || probe)),
                        )
                    }),
            )
    };
    button = button.child(
        div()
            .grid()
            .grid_cols(1)
            .items_center()
            .child(cell(
                Some(control_icon(action.icon)),
                label.clone(),
                !copied,
                "rest",
            ))
            .child(cell(
                (!labelled || fit == Fit::Reserve).then(|| control_icon("check")),
                if labelled {
                    action.done.into()
                } else {
                    "".into()
                },
                copied,
                action.state,
            )),
    );
    if labelled {
        // The content is drawn text, which names nothing.
        button = button.accessibility_label(label);
        return match tooltip {
            Some(tooltip) if !tooltip.is_empty() => button.tooltip(tooltip),
            _ => button,
        };
    }
    let Some(tooltip) = tooltip else {
        return button;
    };
    if tooltip.is_empty() {
        return if copied {
            button.tooltip(action.done)
        } else {
            button
        };
    }
    // An icon-only button says Copied in its tooltip, built as the tooltip
    // renders, so one already shown changes with the mark; and the press that
    // copies leaves it shown, or still showing after its delay, while the
    // pointer stays on the button.
    button
        .tooltip_through_press()
        .tooltip_view(move |window, cx| {
            let (text, state) = if self::confirmed(&mark, cx) {
                (SharedString::from(action.done), action.state)
            } else {
                (tooltip.clone(), "rest")
            };
            let probe = id.clone();
            Tooltip::element(move |_, _| {
                let probe = probe.clone();
                div()
                    .debug_selector(move || format!("{probe}/tooltip/{state}"))
                    .child(text.clone())
            })
            .build(window, cx)
        })
}

/// The polite status node announcing the latest confirmation or
/// announcement in `window`, drawn once at the window root until it ends.
/// Each one is a new node, which screen readers announce when it appears; it
/// is out of flow and draws nothing. AT-SPI announces its name and
/// AccessKit's macOS adapter its value, so both carry the text.
pub(crate) fn announcement(window: &Window, cx: &App) -> Option<Stateful<Div>> {
    let feedback = cx.try_global::<Feedback>()?;
    let latest = [feedback.latest.as_ref(), feedback.spoken.as_ref()]
        .into_iter()
        .flatten()
        .filter(|made| made.window == window.window_handle())
        .max_by_key(|made| made.generation)?;
    Some(
        div()
            .id(("copy-announcement", latest.generation))
            .debug_selector(|| "copy-announcement".into())
            .role(Role::Status)
            .polite()
            .aria_label(latest.announcement.clone())
            .aria_value(latest.announcement.clone())
            .absolute()
            .size(px(1.))
            .overflow_hidden(),
    )
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::commit_message::{
        full_message,
        tests::{commit, copy_chord, draw, message_selection},
    };
    use ::core::prelude::v1::test;
    use gpui_kit::component::Root;
    use std::{cell::RefCell, rc::Rc};

    /// Which content a copy button draws.
    #[derive(Debug, PartialEq)]
    pub(crate) enum Shows {
        Rest,
        Copied,
    }

    /// What copy button `id` drew in the last frame: exactly one of its
    /// cells is painted.
    pub(crate) fn shows(cx: &mut VisualTestContext, id: &str) -> Shows {
        // The test context takes only static selectors.
        let mut drawn = |state| cx.debug_bounds(format!("{id}/{state}").leak());
        match (drawn("rest"), drawn("copied")) {
            (Some(_), None) => Shows::Rest,
            (None, Some(_)) => Shows::Copied,
            drawn => panic!("{id} drew {drawn:?}"),
        }
    }

    /// The copy announcement the last frame drew at the window root: its
    /// node's id and its name, which is also its value.
    pub(crate) fn announced(cx: &mut VisualTestContext) -> Option<(ElementId, String)> {
        cx.debug_bounds("copy-announcement")?;
        cx.update(|window, cx| {
            let node = announcement(window, cx).expect("the drawn copy is current");
            let mut a11y = gpui::accesskit::Node::new(node.a11y_role().unwrap());
            node.write_a11y_info(&mut a11y);
            assert_eq!(a11y.role(), Role::Status);
            assert_eq!(
                a11y.label(),
                a11y.value(),
                "AT-SPI reads the name, macOS the value"
            );
            Some((gpui::Element::id(&node).unwrap(), a11y.label()?.to_owned()))
        })
    }

    pub(crate) fn click(cx: &mut VisualTestContext, selector: &'static str) {
        let bounds = cx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), Modifiers::default());
    }

    pub(crate) fn clipboard(cx: &mut VisualTestContext) -> String {
        cx.read(|cx| cx.read_from_clipboard().unwrap().text().unwrap())
    }

    /// Lets `time` pass and runs what it wakes. Only a window that asked for
    /// a frame draws one.
    pub(crate) fn wait(cx: &mut VisualTestContext, time: Duration) {
        cx.executor().advance_clock(time);
        cx.run_until_parked();
    }

    /// A 1000 × 680 GitTurtle window, drawn by its own render with its dialog
    /// layer, in History on `commits` with the first selected.
    pub(crate) fn history(
        cx: &mut TestAppContext,
        commits: Vec<Commit>,
    ) -> (Entity<GitTurtle>, &mut VisualTestContext) {
        crate::views::tests::init_app(cx);
        cx.update(|cx| {
            shortcuts::bind_keys(cx);
        });
        let captured: Rc<RefCell<Option<Entity<GitTurtle>>>> = Default::default();
        let observed = captured.clone();
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let app = cx.new(|cx| {
                let mut app = GitTurtle::new(
                    None,
                    Preferences::default(),
                    repository_tabs::Session::default(),
                    activity::State::default(),
                    recovery_drafts::State::default(),
                    window,
                    cx,
                );
                // As in `commit_message::tests::fixture`: drain the startup
                // preferences read before GPUI polls its reply.
                futures::executor::block_on(app.preferences_writer.submit(|| Ok(())))
                    .expect("startup preferences worker replied")
                    .expect("startup preferences queue drained");
                app.page = AppPage::Repository;
                app.graph = crate::graph::layout(&commits, || Ok::<_, ()>(())).unwrap();
                app.visible = (0..commits.len()).collect();
                app.selected_commit = (!commits.is_empty()).then_some(0);
                app.commits = commits;
                app
            });
            *captured.borrow_mut() = Some(app.clone());
            Root::new(app, window, cx)
        });
        let app = observed.borrow_mut().take().unwrap();
        cx.simulate_resize(size(px(1000.), px(680.)));
        draw(cx);
        (app, cx)
    }

    /// Hash reads Copied with its check in the bounds it had at rest, and
    /// "Copied commit hash" is announced; 1.5 s later it reads Hash again and
    /// the announcement is gone, drawn by the frame the restore asks for.
    #[gpui::test]
    fn hash_reads_copied_in_its_own_bounds_then_restores(cx: &mut TestAppContext) {
        let (_app, cx) = history(cx, vec![commit("a", "Selected title", "Body.")]);
        let rest = cx.debug_bounds("copy-commit").unwrap();
        assert_eq!(shows(cx, "copy-commit"), Shows::Rest);
        assert_eq!(announced(cx), None);

        click(cx, "copy-commit");
        assert_eq!(clipboard(cx), "a".repeat(40));
        assert_eq!(shows(cx, "copy-commit"), Shows::Copied);
        assert_eq!(cx.debug_bounds("copy-commit"), Some(rest));
        assert_eq!(announced(cx).unwrap().1, "Copied commit hash");

        wait(cx, RESTORE_AFTER - Duration::from_millis(1));
        assert_eq!(shows(cx, "copy-commit"), Shows::Copied);
        wait(cx, Duration::from_millis(1));
        assert_eq!(shows(cx, "copy-commit"), Shows::Rest);
        assert_eq!(cx.debug_bounds("copy-commit"), Some(rest));
        assert_eq!(announced(cx), None);
    }

    /// Copied stays with what was copied: the commit selected next draws its
    /// Hash and Copy message with the same ids in the same place, at rest,
    /// while the copied commit's Hash reads Copied again on returning to it
    /// within the 1.5 s, and reverts when they are up.
    #[gpui::test]
    fn copied_stays_with_the_commit_it_copied(cx: &mut TestAppContext) {
        let (app, cx) = history(
            cx,
            vec![
                commit("a", "First title", "Body."),
                commit("b", "Second title", "Body."),
            ],
        );
        let focus_history = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                let focus = app.read(cx).focus.clone();
                window.focus(&focus, cx);
            });
            draw(cx);
        };
        let selected = |cx: &mut VisualTestContext| cx.read(|cx| app.read(cx).selected_commit);
        let hash = cx.debug_bounds("copy-commit").unwrap();
        click(cx, "copy-commit");
        assert_eq!(clipboard(cx), "a".repeat(40));
        assert_eq!(shows(cx, "copy-commit"), Shows::Copied);

        wait(cx, Duration::from_millis(200));
        focus_history(cx);
        cx.simulate_keystrokes("down");
        draw(cx);
        assert_eq!(selected(cx), Some(1));
        assert_eq!(cx.debug_bounds("copy-commit"), Some(hash), "the same place");
        assert_eq!(
            shows(cx, "copy-commit"),
            Shows::Rest,
            "another commit's Hash"
        );
        assert_eq!(shows(cx, "copy-message"), Shows::Rest);

        wait(cx, Duration::from_millis(300));
        cx.simulate_keystrokes("up");
        draw(cx);
        assert_eq!(selected(cx), Some(0));
        assert_eq!(
            shows(cx, "copy-commit"),
            Shows::Copied,
            "the copied commit's Hash"
        );
        wait(cx, RESTORE_AFTER - Duration::from_millis(500));
        assert_eq!(shows(cx, "copy-commit"), Shows::Rest);

        // Copy message too belongs to its commit.
        click(cx, "copy-message");
        assert_eq!(shows(cx, "copy-message"), Shows::Copied);
        focus_history(cx);
        cx.simulate_keystrokes("down");
        draw(cx);
        assert_eq!(selected(cx), Some(1));
        assert_eq!(
            shows(cx, "copy-message"),
            Shows::Rest,
            "another commit's message"
        );
    }

    /// Copy message and Hash start their content at the button's leading
    /// inset at rest, with no gap before the icon, and copied the check
    /// starts where the icon did; neither state moves either button. Hash
    /// carries Copy message's icon before its label.
    #[gpui::test]
    fn copied_takes_the_icons_place_and_moves_nothing(cx: &mut TestAppContext) {
        let (_app, cx) = history(cx, vec![commit("a", "Selected title", "Body.")]);
        let ids = ["copy-message", "copy-commit"];
        let buttons = |cx: &mut VisualTestContext| ids.map(|id| cx.debug_bounds(id).unwrap());
        let content = |cx: &mut VisualTestContext, id: &str, state: &str| {
            cx.debug_bounds(format!("{id}/{state}").leak()).unwrap()
        };
        let rest = buttons(cx);
        assert!(rest[0].right() <= rest[1].left(), "{rest:?}");
        for (button, id) in rest.iter().zip(ids) {
            let at_rest = content(cx, id, "rest");
            assert_eq!(
                at_rest.left(),
                button.left() + appearance::ui_size(10.),
                "{id} at rest starts at its inset"
            );
            click(cx, id);
            assert_eq!(shows(cx, id), Shows::Copied);
            assert_eq!(
                content(cx, id, "copied").left(),
                at_rest.left(),
                "{id}'s check takes its icon's place"
            );
            assert_eq!(buttons(cx), rest, "{id} copied moves neither button");
            wait(cx, RESTORE_AFTER);
            assert_eq!(shows(cx, id), Shows::Rest);
            assert_eq!(buttons(cx), rest);
        }
        let hash = content(cx, "copy-commit", "rest").size.width;
        let label = cx.update(|window, _| {
            crate::views::line_widths(&[("Hash", appearance::ui_text(12.))], window)
        });
        let icon = appearance::ui_size(16. + 6.);
        assert!(
            (hash - (icon + label)).abs() < px(0.5),
            "{hash:?} is the icon and {label:?}"
        );
    }

    /// The copy actions that once drew a label alone (Copy details, Copy saved
    /// text, Copy exact text, Copy commit ID, Copy object ID, the GitHub
    /// panel's Copy sign-in command, Copy complete draft and Copy reply) lead
    /// with the copy icon, as every copy button does, so the check takes the
    /// icon's place and "Copied" starts where the label did: copied, they
    /// gained a check where no icon was and the label moved 22 px. Neither
    /// state moves the button.
    #[gpui::test]
    fn every_copy_buttons_label_and_copied_start_at_one_x(cx: &mut TestAppContext) {
        const LABELS: [&str; 8] = [
            "Copy details",
            "Copy saved text",
            "Copy exact text",
            "Copy commit ID",
            "Copy object ID",
            "Copy sign-in command",
            "Copy complete draft",
            "Copy reply",
        ];
        struct Buttons;
        impl Render for Buttons {
            fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
                div().flex().flex_col().items_start().gap_2().children(
                    LABELS.map(|label| copy_button(Mark::new(label, "subject"), label, "", cx)),
                )
            }
        }
        cx.update(gpui_kit::init);
        let (_, cx) = cx.add_window_view(|_, _| Buttons);
        draw(cx);
        let bounds = |cx: &mut VisualTestContext, selector: String| {
            cx.debug_bounds(selector.leak())
                .unwrap_or_else(|| panic!("drawn"))
        };
        let icon = appearance::ui_size(16. + 6.);
        for label in LABELS {
            let button = bounds(cx, label.to_owned());
            let rest = bounds(cx, format!("{label}/rest"));
            let text = bounds(cx, format!("{label}/rest/text"));
            assert_eq!(
                rest.left(),
                button.left() + appearance::ui_size(10.),
                "{label}"
            );
            assert!(
                (text.left() - rest.left() - icon).abs() < px(0.5),
                "{label} leads with the copy icon: {rest:?} {text:?}"
            );
            cx.update(|window, cx| {
                copy(
                    window,
                    cx,
                    Some(Mark::new(label, "subject")),
                    "text",
                    "x".into(),
                )
            });
            draw(cx);
            assert_eq!(shows(cx, label), Shows::Copied);
            let copied = bounds(cx, format!("{label}/copied/text"));
            assert_eq!(
                copied.left(),
                text.left(),
                "{label}: Copied starts at its label"
            );
            assert_eq!(
                bounds(cx, label.to_owned()),
                button,
                "{label} keeps its bounds"
            );
            wait(cx, RESTORE_AFTER);
            assert_eq!(shows(cx, label), Shows::Rest);
        }
    }

    /// An icon-only copy button says Copied in its tooltip while the pointer
    /// stays on it. The kit hid a tooltip on mouse down until the pointer
    /// left and came back, so no tooltip followed a click on the compact
    /// inspector's Hash (18 pt in 461 × 490), even after a small move, and a
    /// shown tooltip kept the words it had when the pointer entered. A shown
    /// tooltip now reads Copied from the click and its own words again after
    /// the 1.5 s; a click before its delay is up shows Copied once it is.
    #[gpui::test]
    fn an_icon_only_copy_says_copied_in_its_tooltip_while_hovered(cx: &mut TestAppContext) {
        let (_app, cx) = history(cx, vec![commit("a", "Selected title", "Body.")]);
        cx.update(|window, cx| appearance::apply_text_sizes(18, 12, window, cx));
        cx.simulate_resize(size(px(461.), px(490.)));
        draw(cx);
        let hash = cx.debug_bounds("copy-commit").unwrap();
        assert_eq!(
            hash.size.width,
            appearance::ui_size(28.).ceil(),
            "Hash is its icon"
        );
        let tooltip = |cx: &mut VisualTestContext| match (
            cx.debug_bounds("copy-commit/tooltip/rest"),
            cx.debug_bounds("copy-commit/tooltip/copied"),
        ) {
            (None, None) => None,
            (Some(_), None) => Some(Shows::Rest),
            (None, Some(_)) => Some(Shows::Copied),
            both => panic!("two tooltips: {both:?}"),
        };
        cx.simulate_mouse_move(hash.center(), None, Modifiers::default());
        wait(cx, Duration::from_millis(600));
        assert_eq!(tooltip(cx), Some(Shows::Rest), "hovered");
        cx.simulate_click(hash.center(), Modifiers::default());
        draw(cx);
        assert_eq!(shows(cx, "copy-commit"), Shows::Copied);
        assert_eq!(tooltip(cx), Some(Shows::Copied), "clicked");
        wait(cx, Duration::from_millis(1000));
        assert_eq!(tooltip(cx), Some(Shows::Copied), "still on the button");
        wait(cx, RESTORE_AFTER - Duration::from_millis(1000));
        assert_eq!(shows(cx, "copy-commit"), Shows::Rest);
        assert_eq!(tooltip(cx), Some(Shows::Rest), "reverted");

        // Away, then back and clicked at once, within the tooltip's delay.
        cx.simulate_mouse_move(point(px(4.), px(4.)), None, Modifiers::default());
        wait(cx, Duration::from_millis(600));
        assert_eq!(tooltip(cx), None, "left");
        cx.simulate_mouse_move(hash.center(), None, Modifiers::default());
        cx.simulate_click(hash.center(), Modifiers::default());
        draw(cx);
        assert_eq!(shows(cx, "copy-commit"), Shows::Copied);
        assert_eq!(tooltip(cx), None, "within the delay");
        wait(cx, Duration::from_millis(600));
        assert_eq!(tooltip(cx), Some(Shows::Copied), "after the delay");
        wait(cx, RESTORE_AFTER);
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// A second click restarts the time, so Hash still reads Copied after the
    /// first click's 1.5 s, and is announced again with a fresh node.
    #[gpui::test]
    fn a_repeated_copy_restarts_the_time_and_is_announced_again(cx: &mut TestAppContext) {
        let (_app, cx) = history(cx, vec![commit("a", "Selected title", "Body.")]);
        click(cx, "copy-commit");
        let (first, _) = announced(cx).unwrap();
        wait(cx, Duration::from_millis(1000));
        click(cx, "copy-commit");
        let (second, name) = announced(cx).unwrap();
        assert_ne!(first, second, "a repeated copy is a new node");
        assert_eq!(name, "Copied commit hash");

        wait(cx, Duration::from_millis(600));
        assert_eq!(shows(cx, "copy-commit"), Shows::Copied);
        assert_eq!(announced(cx).map(|(id, _)| id), Some(second));
        wait(cx, RESTORE_AFTER - Duration::from_millis(601));
        assert_eq!(shows(cx, "copy-commit"), Shows::Copied);
        wait(cx, Duration::from_millis(1));
        assert_eq!(shows(cx, "copy-commit"), Shows::Rest);
        assert_eq!(announced(cx), None);
    }

    /// Copy message, labelled and as its compact icon, copies the complete
    /// message and reads Copied in its own bounds; it takes the mark from
    /// Hash, so one button reads Copied at a time.
    #[gpui::test]
    fn copy_message_labelled_and_compact_reads_copied(cx: &mut TestAppContext) {
        let selected = commit("a", "Selected title", "Body paragraph.");
        let message = full_message(&selected);
        let (_app, cx) = history(cx, vec![selected]);
        for (width, height, text) in [(1000., 680., 13), (461., 490., 18)] {
            cx.update(|window, cx| appearance::apply_text_sizes(text, 12, window, cx));
            cx.simulate_resize(size(px(width), px(height)));
            draw(cx);
            let rest = cx.debug_bounds("copy-message").unwrap();
            let compact = rest.size.width == appearance::ui_size(28.).ceil();
            assert_eq!(compact, text == 18, "{rest:?} at {text} pt");

            click(cx, "copy-commit");
            assert_eq!(shows(cx, "copy-commit"), Shows::Copied);
            click(cx, "copy-message");
            assert_eq!(clipboard(cx), message);
            assert_eq!(shows(cx, "copy-message"), Shows::Copied);
            assert_eq!(shows(cx, "copy-commit"), Shows::Rest);
            assert_eq!(cx.debug_bounds("copy-message"), Some(rest));
            assert_eq!(announced(cx).unwrap().1, "Copied commit message");

            wait(cx, RESTORE_AFTER);
            assert_eq!(shows(cx, "copy-message"), Shows::Rest);
            assert_eq!(cx.debug_bounds("copy-message"), Some(rest));
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// The focused History list's Copy is announced with no button to mark,
    /// while selected message text copies silently, as text fields do.
    #[gpui::test]
    fn list_copy_is_announced_and_a_text_selection_copies_silently(cx: &mut TestAppContext) {
        let selected = commit("a", "Selected title", "Body.");
        let message = full_message(&selected);
        let (app, cx) = history(cx, vec![selected]);
        cx.update(|window, cx| {
            let focus = app.read(cx).focus.clone();
            window.focus(&focus, cx);
        });
        draw(cx);
        assert_eq!(copy_chord(cx), "a".repeat(40));
        assert_eq!(announced(cx).unwrap().1, "Copied commit hash");
        assert_eq!(shows(cx, "copy-commit"), Shows::Rest);
        wait(cx, RESTORE_AFTER);
        assert_eq!(announced(cx), None);

        let selection = message_selection(&app, cx);
        cx.update(|window, cx| selection.select_all(window, cx));
        draw(cx);
        assert_eq!(copy_chord(cx), message);
        draw(cx);
        assert_eq!(announced(cx), None);
    }

    /// Copied changes what a button shows, never what it is called.
    #[gpui::test]
    fn the_accessible_name_stays_the_actions_name(cx: &mut TestAppContext) {
        let (_app, cx) = history(cx, Vec::new());
        let labelled_mark = Mark::new("copy-labelled", "a");
        let icon_mark = Mark::new("copy-icon", "a");
        let buttons = |cx: &mut VisualTestContext| {
            cx.update(|_, cx| {
                [
                    copy_button(labelled_mark.clone(), "Copy path", "Copy the path", cx),
                    copy_button(icon_mark.clone(), "", "Copy the path", cx),
                    copy_button(icon_mark.clone(), "", "Copy the path", cx)
                        .accessibility_label("Copy file path"),
                ]
            })
        };
        let names = |cx: &mut VisualTestContext, buttons: Vec<Button>| {
            crate::worktrees::ring_room_tests::row_nodes(cx, buttons)
                .into_iter()
                .map(|node| node.label().map(str::to_owned))
                .collect::<Vec<_>>()
        };
        let expected = vec![
            Some("Copy path".to_owned()),
            Some("copy file path".to_owned()),
            Some("Copy file path".to_owned()),
        ];
        let [labelled, icon, named] = buttons(cx);
        assert_eq!(names(cx, vec![labelled, icon, named]), expected);
        let mark = labelled_mark.clone();
        cx.update(|window, cx| copy(window, cx, Some(mark), "path", "a".into()));
        let [labelled, ..] = buttons(cx);
        let mark = icon_mark.clone();
        cx.update(|window, cx| copy(window, cx, Some(mark), "path", "a".into()));
        let [_, icon, named] = buttons(cx);
        cx.update(|_, cx| assert!(confirmed(&icon_mark, cx)));
        assert_eq!(names(cx, vec![labelled, icon, named]), expected);
    }
}
