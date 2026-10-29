//! Empty and edge states (DESIGN.md, "Empty and edge states"). A list or
//! pane with nothing to show, or a repository at an edge (no remote, no
//! upstream, detached HEAD, no commits yet), says so in one deliberate line
//! where the user looks, with the next action beside it as a real control
//! where the product has one. One component draws every such state, so they
//! share their type, inset and spacing, and the line wraps inside its room
//! rather than being cut. Loading and failures keep `empty`'s title and
//! explanation; a state's action only opens an existing form or moves to an
//! existing control, never starting a write or a network action itself.

use crate::*;
use gpui_kit::prelude::FluentBuilder;

/// The line's text size at the default interface size: a list's secondary
/// text, as the Tags and stash lists' notes were.
const LINE_TEXT: f32 = 12.;
/// The room a pane's state keeps from the pane's edges, at the default
/// interface size: a list row's text inset, so in the 156 px inspector of an
/// 18 pt half tile a short action still fits beside both.
const PANE_INSET: f32 = 12.;
/// The space between the line and its action, beside it or below it.
pub(crate) const GAP: f32 = 8.;
/// The widest a pane's line runs before it wraps, at the default interface
/// size.
const LINE_MAX: f32 = 480.;
/// A glyph drawn in a line's place, at the default interface size: the
/// line's own height, as the bar's counts' icons sit in theirs.
pub(crate) const GLYPH: f32 = 14.;

#[cfg(test)]
thread_local! {
    /// What each state drew last, by its id, for a test to read back: its
    /// line, or `glyph:` and the icon drawn in its place.
    static SHOWN: std::cell::RefCell<HashMap<&'static str, SharedString>> =
        Default::default();
}

/// The line the state `id` drew last, or `glyph:` and the icon it drew in
/// the line's place; a test pairs it with the line's bounds in the last
/// frame.
#[cfg(test)]
pub(crate) fn shown(id: &str) -> Option<SharedString> {
    SHOWN.with(|shown| shown.borrow().get(id).cloned())
}

/// One empty or edge state: its line and, where there is one, its action.
pub(crate) struct EmptyState {
    id: &'static str,
    line: SharedString,
    name: Option<SharedString>,
    tooltip: Option<SharedString>,
    action: Option<AnyElement>,
    truncate: bool,
    glyph: Option<&'static str>,
}

impl EmptyState {
    /// The state `id`, which names its element and, with `-line`, its line,
    /// saying `line`.
    pub(crate) fn new(id: &'static str, line: impl Into<SharedString>) -> Self {
        Self {
            id,
            line: line.into(),
            name: None,
            tooltip: None,
            action: None,
            truncate: false,
            glyph: None,
        }
    }

    /// In a row with no room for even the shortest whole line, as the
    /// compact action bar's in a half tile at 18 pt, the `icons/` asset
    /// `glyph` in the line's place, muted as the line is: the whole line
    /// stays its accessible name and leads its tooltip.
    pub(crate) fn glyph(mut self, glyph: Option<&'static str>) -> Self {
        self.glyph = glyph;
        self
    }

    /// The whole line, where the drawn one says less, as the Git action
    /// bar's does once its caption goes: the line's accessible name, and
    /// what its tooltip leads with.
    pub(crate) fn named(mut self, name: impl Into<SharedString>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// What an assistive technology reads the line as: its whole line.
    fn name(&self) -> SharedString {
        self.name.clone().unwrap_or_else(|| self.line.clone())
    }

    /// The next action, usually [`action`].
    pub(crate) fn action(mut self, action: impl IntoElement) -> Self {
        self.action = Some(action.into_any_element());
        self
    }

    /// A longer explanation on the line's tooltip, after the whole line
    /// where the drawn one says less, and the line's accessible description.
    pub(crate) fn tooltip(mut self, tooltip: impl Into<SharedString>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    /// In a row of one line that gives up its width before anything wraps,
    /// as the compact action bar's, the line ends in an ellipsis rather than
    /// wrapping; its tooltip and accessible name keep it whole, and the
    /// action keeps its own width.
    pub(crate) fn truncating(mut self, truncate: bool) -> Self {
        self.truncate = truncate;
        self
    }

    /// The line as drawn, for its host to place; [`Self::read`] makes it
    /// the static text an assistive technology reads.
    fn line(&self, cx: &App) -> Stateful<Div> {
        let id = self.id;
        let line = self.line.clone();
        #[cfg(test)]
        SHOWN.with(|shown| {
            let drawn = self
                .glyph
                .map_or_else(|| line.clone(), |glyph| format!("glyph:{glyph}").into());
            shown.borrow_mut().insert(id, drawn)
        });
        let name = self.name();
        // A glyph says nothing itself, so its tooltip always leads with the
        // whole line.
        let tooltip = self.tooltip.clone().map(|explanation| -> SharedString {
            if name == line && self.glyph.is_none() {
                explanation
            } else {
                format!("{name} · {explanation}").into()
            }
        });
        let muted = palette(cx).muted;
        div()
            .id(SharedString::from(format!("{id}-line")))
            .debug_selector(move || format!("{id}-line"))
            .when_some(self.tooltip.clone(), |line, explanation| {
                line.aria_description(explanation)
            })
            .text_size(appearance::ui_text(LINE_TEXT))
            .text_color(rgb(muted))
            .when_some(tooltip, |line, tooltip| {
                line.tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
            })
            .map(|drawn| match self.glyph {
                Some(glyph) => drawn.flex_shrink_0().child(
                    svg()
                        .path(format!("icons/{glyph}.svg"))
                        .size(appearance::ui_size(GLYPH))
                        .text_color(rgb(muted)),
                ),
                None => drawn
                    .when(self.truncate, |line| line.min_w_0().truncate())
                    .child(line),
            })
    }

    /// `line`, placed, as the static text an assistive technology reads by
    /// the whole line, where it is drawn among the state's controls.
    fn read(&self, line: Stateful<Div>) -> impl IntoElement + use<> {
        native_accessibility::static_text(line, self.name())
    }

    /// Centered in the pane it fills, the line wrapping inside the pane's
    /// inset and the action below it.
    pub(crate) fn pane(self, cx: &App) -> AnyElement {
        let id = self.id;
        let line = self.read(
            self.line(cx)
                .w_full()
                .max_w(appearance::ui_size(LINE_MAX))
                .text_center(),
        );
        div()
            .id(id)
            .debug_selector(move || id.into())
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            // Beside the line its pane's text inset; above and below, only
            // half the gap, as a short pane, such as Changes' list at 18 pt
            // in a 461 × 490 window, has no height to spare.
            .px(appearance::ui_size(PANE_INSET))
            .py(appearance::ui_size(GAP / 2.))
            .gap(appearance::ui_size(GAP))
            .child(line)
            .children(self.action)
            .into_any_element()
    }

    /// In a row of controls, as the Git action bar's counts are, the action
    /// after the line; the row's prediction gives it the room it takes.
    pub(crate) fn inline(self, cx: &App) -> AnyElement {
        let id = self.id;
        let line = self.read(self.line(cx).min_w_0());
        div()
            .id(id)
            .debug_selector(move || id.into())
            .min_w_0()
            .flex()
            .items_center()
            .gap(appearance::ui_size(GAP))
            .child(line)
            .children(
                self.action
                    .map(|action| div().flex_shrink_0().child(action)),
            )
            .into_any_element()
    }

    /// At the leading edge of the list that holds it, the line as wide as
    /// the list and wrapping inside it, and the action below it; the host
    /// gives the inset its rows take.
    pub(crate) fn block(self, cx: &App) -> AnyElement {
        let id = self.id;
        let line = self.read(self.line(cx).w_full());
        div()
            .id(id)
            .debug_selector(move || id.into())
            .w_full()
            .flex()
            .flex_col()
            .items_start()
            .gap(appearance::ui_size(GAP))
            .child(line)
            .children(self.action)
            .into_any_element()
    }
}

/// The size [`EmptyState::block`] draws its line at, for a list that
/// measures the line's wrapped lines.
pub(crate) fn line_text() -> Pixels {
    appearance::ui_text(LINE_TEXT)
}

/// The height [`EmptyState::block`] takes with its line wrapped to `lines`
/// lines and, where `action`, its action below it: what a list that keeps
/// the state whole keeps as its least height, its own padding aside.
pub(crate) fn block_height(lines: usize, action: bool, window: &Window) -> Pixels {
    let line = window
        .text_style()
        .line_height
        .to_pixels(line_text().into(), window.rem_size());
    line * lines.max(1) as f32
        + if action {
            appearance::ui_size(GAP) + appearance::ui_size(28.)
        } else {
            Pixels::ZERO
        }
}

/// An empty or edge state's action: the shared 28 px control with its icon
/// and label, resting on `subtle` so that on an empty surface it reads as a
/// control. Its label is its accessible name. An empty `symbol` leaves out
/// the icon where no glyph names the action, as Name repository, which moves
/// to a name field, would otherwise take a search's magnifier.
pub(crate) fn action(id: &'static str, label: &'static str, symbol: &str) -> Button {
    button(id, label, symbol, false)
        .secondary()
        .debug_selector(move || id.into())
}

/// Working Changes' line with nothing to list: a clean working tree, or, on
/// a branch with no commits yet, no files yet. The list, as short as two
/// lines at 18 pt in a half tile, says only that; the preview beside it
/// adds what comes next.
pub(crate) fn working_clean_line(
    status: Option<&gitturtle_core::RepositoryStatus>,
    caption: bool,
) -> &'static str {
    match (status.is_some_and(|status| status.head.is_none()), caption) {
        (true, false) => "No files yet",
        (true, true) => "No files yet · Files you add appear here for the first commit",
        (false, false) => "Working tree clean",
        (false, true) => "Working tree clean · Edits you make appear here",
    }
}

/// The repository's edge, which the Git action bar's sync state shows in
/// place of the ahead and behind counts an upstream gives it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum RepositoryEdge {
    /// HEAD names commit `oid` rather than a branch.
    Detached { oid: String },
    /// The repository has no remote to fetch from or push to.
    NoRemote,
    /// The current branch has no commits yet, so there is nothing to push.
    Unborn,
    /// The current branch has no upstream; Push sets one.
    NoUpstream,
}

/// How the full action bar draws the repository edge, from the richest form
/// to the barest, as "One responsive order" gives way: the action becomes
/// its icon first, then the line's caption goes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct EdgeForm {
    /// The whole line, caption included, rather than `short_line`.
    pub(crate) caption: bool,
    /// The action with its label rather than as its icon.
    pub(crate) labelled: bool,
    /// In the compact bar, the line's glyph in its place, where even its
    /// shortest whole form (`compact_line`) would be cut beside the branch
    /// picker's whole label: the line stays its name and tooltip.
    pub(crate) glyph: bool,
}

impl EdgeForm {
    /// The forms the full bar tries, richest first; the last always fits as
    /// well as any.
    pub(crate) const ORDER: [Self; 3] = [
        Self {
            caption: true,
            labelled: true,
            glyph: false,
        },
        Self {
            caption: true,
            labelled: false,
            glyph: false,
        },
        Self {
            caption: false,
            labelled: false,
            glyph: false,
        },
    ];
}

/// The action an edge offers: its control's id, label and icon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EdgeAction {
    pub(crate) id: &'static str,
    pub(crate) label: &'static str,
    pub(crate) symbol: &'static str,
}

/// Detached HEAD's action: Targets' branch field, from which Create makes a
/// branch at HEAD and switches to it.
pub(crate) const CREATE_BRANCH_HERE: EdgeAction = EdgeAction {
    id: "edge-create-branch",
    label: "Create branch…",
    symbol: "branch-add",
};

/// No remote's action: the remote form the branch menu's Manage remotes
/// opens.
pub(crate) const ADD_REMOTE: EdgeAction = EdgeAction {
    id: "edge-add-remote",
    label: "Add remote…",
    symbol: "remote-add",
};

impl RepositoryEdge {
    /// The edge `status` shows with `remotes` configured, if any. An
    /// upstream gives the counts instead, and a detached HEAD outranks a
    /// missing remote, which outranks an unborn branch.
    pub(crate) fn of(
        status: &gitturtle_core::RepositoryStatus,
        remotes: &[gitturtle_core::Remote],
    ) -> Option<Self> {
        match (&status.branch, &status.head) {
            (None, Some(oid)) => Some(Self::Detached { oid: oid.clone() }),
            _ if status.upstream.is_some() => None,
            _ if remotes.is_empty() => Some(Self::NoRemote),
            (_, None) => Some(Self::Unborn),
            _ => Some(Self::NoUpstream),
        }
    }

    /// The edge's one line.
    pub(crate) fn line(&self) -> String {
        match self {
            Self::NoUpstream => "No upstream · Push sets one".into(),
            _ => self.short_line(),
        }
    }

    /// The line without its caption, where the bar's row has no room for
    /// it: the caption's words stay in the tooltip.
    pub(crate) fn short_line(&self) -> String {
        match self {
            Self::Detached { oid } => format!("Detached at {}", short_oid(oid)),
            Self::NoRemote => "No remote".into(),
            Self::Unborn => "No commits to push yet".into(),
            Self::NoUpstream => "No upstream".into(),
        }
    }

    /// The line in the compact bar, where a detached HEAD's branch picker
    /// names the commit (`compact_branch`), so the line need not.
    pub(crate) fn compact_line(&self) -> String {
        match self {
            Self::Detached { .. } => "Detached".into(),
            _ => self.short_line(),
        }
    }

    /// The compact bar's branch picker label at this edge, if it is not the
    /// branch's name: a detached HEAD's short commit ID, which says what
    /// HEAD is in fewer words than "Detached HEAD".
    pub(crate) fn compact_branch(&self) -> Option<String> {
        match self {
            Self::Detached { oid } => Some(short_oid(oid)),
            _ => None,
        }
    }

    /// The `icons/` asset the compact bar draws in the line's place where
    /// even [`Self::compact_line`] would be cut.
    pub(crate) fn glyph(&self) -> &'static str {
        match self {
            Self::Detached { .. } | Self::Unborn => "commit",
            Self::NoRemote | Self::NoUpstream => "remote",
        }
    }

    /// What the line's tooltip adds.
    pub(crate) fn explanation(&self) -> String {
        match self {
            Self::Detached { oid } => format!(
                "HEAD is at commit {} with no branch. Create a branch here to keep new commits on it.",
                short_oid(oid)
            ),
            Self::NoRemote => {
                "This repository has no remote. Add one to fetch, pull and push.".into()
            }
            Self::Unborn => "This branch has no commits yet. Commit in Changes first.".into(),
            Self::NoUpstream => {
                "No upstream is configured. Push publishes this branch to the target in Targets and sets it as the upstream."
                    .into()
            }
        }
    }

    /// The action beside the line. No upstream's is Push itself, in the same
    /// bar.
    pub(crate) fn action(&self) -> Option<EdgeAction> {
        match self {
            Self::Detached { .. } => Some(CREATE_BRANCH_HERE),
            Self::NoRemote => Some(ADD_REMOTE),
            Self::Unborn | Self::NoUpstream => None,
        }
    }
}

impl GitTurtle {
    /// The repository's edge for the Git action bar, once its status is read.
    pub(crate) fn repository_edge(&self) -> Option<RepositoryEdge> {
        self.work_status
            .as_ref()
            .and_then(|status| RepositoryEdge::of(status, &self.remotes))
    }

    /// Runs `action`, an edge's: Create branch… opens Targets on its branch
    /// field, Add remote… opens the remote form. Neither writes anything.
    pub(crate) fn run_edge_action(
        &mut self,
        action: EdgeAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.page != AppPage::Repository || self.operation_busy.is_some() {
            return;
        }
        if action == CREATE_BRANCH_HERE {
            self.git_actions_open = true;
            window.focus(&self.branch_name.read(cx).focus_handle(cx), cx);
            cx.notify();
        } else if action == ADD_REMOTE {
            self.open_remote_form(None, window, cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::focus_ring::painted_outlines_where;
    use crate::views::tests::{draw, fixture_git, history_script, settle, styled_history_window};
    use core::prelude::v1::test;
    use gpui_kit::ScaledPixels;

    /// Behind every text run, so a test finds the runs a clip cuts, as the
    /// width walk marks them: a hue nothing in the app paints, almost clear.
    fn text_mark() -> Hsla {
        hsla(0.234, 0.987, 0.456, 0.004)
    }

    /// The default and the largest interface sizes.
    const SIZES: [u8; 2] = [13, 18];

    /// The narrowest window, a half tile: short, where 18 pt takes the
    /// compact chrome, and tall.
    const WINDOWS: [(f32, f32); 2] = [(461., 490.), (461., 980.)];

    /// A window on the repository `input` imports, every text run marked,
    /// with the app's keys bound and Midnight applied, and its root.
    async fn window<'a>(
        cx: &'a mut TestAppContext,
        input: &str,
    ) -> (
        tempfile::TempDir,
        Entity<GitTurtle>,
        &'a mut VisualTestContext,
        PathBuf,
    ) {
        let (fixture, app, cx) =
            styled_history_window(cx, input.as_bytes(), |root| root.text_bg(text_mark())).await;
        crate::focus_ring::sweep::prepare(&app, cx);
        let root = app.read_with(cx, |app, _| app.path.clone().unwrap());
        (fixture, app, cx, root)
    }

    /// Reads the working tree, its branch and remotes again, as Refresh does.
    async fn refresh(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
        cx.update(|window, cx| app.update(cx, |app, cx| app.refresh_worktree(window, cx)));
        settle(app, cx).await;
    }

    /// Opens Tags and draws it once its list has landed. The list is read on
    /// the app's operation executor, a thread of its own, and the dialog opens
    /// with the reply, so the test waits for that read
    /// (`tags::tests::read_lands`) rather than for wall-clock time, which a
    /// slow read outlasted.
    async fn open_tags(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_tags(window, cx)));
        crate::tags::tests::read_lands(app, cx).await;
        draw(cx);
    }

    /// Draws, letting the reader threads answer, until `done` holds.
    fn wait(
        cx: &mut VisualTestContext,
        what: &str,
        mut done: impl FnMut(&mut VisualTestContext) -> bool,
    ) {
        for _ in 0..400 {
            cx.executor()
                .advance_clock(std::time::Duration::from_millis(50));
            draw(cx);
            if done(cx) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("{what} never happened");
    }

    fn selector(name: String) -> &'static str {
        Box::leak(name.into_boxed_str())
    }

    /// The marked text runs whose middle lies in `within` and that cross a
    /// side of their clip, other than those an open dialog's overlay, the
    /// last fill that spans the window, covers.
    fn cut_runs(cx: &mut VisualTestContext, within: Bounds<Pixels>) -> Vec<String> {
        let mark = gpui_kit::Background::from(text_mark());
        let (scale, viewport, quads) = cx.update(|window, _| {
            (
                window.scale_factor(),
                window.viewport_size(),
                window.painted_quads(),
            )
        });
        let logical = |value: ScaledPixels| px(value.0 / scale);
        let slack = ScaledPixels(0.5);
        let overlay = cx
            .update(|window, cx| window.has_active_dialog(cx))
            .then(|| {
                quads
                    .iter()
                    .filter(|quad| {
                        logical(quad.bounds.size.width) >= viewport.width - px(1.)
                            && logical(quad.bounds.size.height) >= viewport.height - px(1.)
                    })
                    .map(|quad| quad.order)
                    .max()
            })
            .flatten();
        quads
            .iter()
            .filter(|quad| quad.background == mark)
            .filter(|quad| overlay.is_none_or(|overlay| quad.order > overlay))
            .filter_map(|quad| {
                let (drawn, clip) = (quad.bounds, quad.content_mask.bounds);
                let middle = point(
                    logical(drawn.left() + drawn.size.width / 2.),
                    logical(drawn.top() + drawn.size.height / 2.),
                );
                let cut = drawn.left() < clip.left() - slack
                    || drawn.right() > clip.right() + slack
                    || drawn.top() < clip.top() - slack
                    || drawn.bottom() > clip.bottom() + slack;
                (within.contains(&middle) && cut).then(|| format!("{drawn:?} in {clip:?}"))
            })
            .collect()
    }

    /// Tabs on until a whole accent ring surrounds the control `selector`,
    /// as a focused Button draws its own, where focus has scrolled it, and
    /// says whether one did.
    fn tab_reaches(cx: &mut VisualTestContext, selector: &'static str) -> bool {
        let accent: Hsla = cx.read(|cx| rgb(palette(cx).accent).into());
        for _ in 0..80 {
            cx.simulate_keystrokes("tab");
            draw(cx);
            let Some(control) = cx.debug_bounds(selector) else {
                continue;
            };
            let ringed = painted_outlines_where(cx, |color| {
                color.a > 0.
                    && Hsla {
                        a: accent.a,
                        ..color
                    } == accent
            })
            .iter()
            .any(|ring| {
                ring.whole
                    && ring.bounds.contains(&control.center())
                    && ring.bounds.size.width >= control.size.width
                    && ring.bounds.size.width <= control.size.width + px(8.)
                    && ring.bounds.size.height >= control.size.height
            });
            if ringed {
                return true;
            }
        }
        false
    }

    /// At 13 and 18 pt in a short and a tall half tile, the state `id` says
    /// `line`, drawn whole inside `region` and the window with every text run
    /// inside its clip, and its action, where it names one, lies inside them
    /// too, where Tab reaches it and rings it whole.
    fn check(
        cx: &mut VisualTestContext,
        id: &'static str,
        line: &str,
        action: Option<&'static str>,
        region: &'static str,
    ) {
        check_lines(cx, id, &[line], action, region);
    }

    /// [`check`], the state saying one of `lines`.
    fn check_lines(
        cx: &mut VisualTestContext,
        id: &'static str,
        lines: &[&str],
        action: Option<&'static str>,
        region: &'static str,
    ) {
        check_in(cx, id, lines, action, region, &WINDOWS);
    }

    /// [`check_lines`] in `windows`.
    fn check_in(
        cx: &mut VisualTestContext,
        id: &'static str,
        lines: &[&str],
        action: Option<&'static str>,
        region: &'static str,
        windows: &[(f32, f32)],
    ) {
        for interface in SIZES {
            for &(width, height) in windows {
                cx.update(|window, cx| appearance::apply_text_sizes(interface, 12, window, cx));
                cx.simulate_resize(size(px(width), px(height)));
                draw(cx);
                let at = format!("{id} at {interface} pt in {width} × {height}");
                let within = cx
                    .debug_bounds(region)
                    .unwrap_or_else(|| panic!("{at}: no {region}"));
                let viewport = cx.update(|window, _| window.viewport_size());
                // The GitHub panel's body scrolls: there, as the width walk
                // does, only the sides count.
                let scrolls = region == "github-panel";
                let inside = |bounds: Bounds<Pixels>| {
                    let slack = px(0.5);
                    bounds.left() >= within.left().max(px(0.)) - slack
                        && bounds.right() <= within.right().min(viewport.width) + slack
                        && (scrolls
                            || bounds.top() >= within.top().max(px(0.)) - slack
                                && bounds.bottom() <= within.bottom().min(viewport.height) + slack)
                };
                let drawn = cx
                    .debug_bounds(selector(format!("{id}-line")))
                    .unwrap_or_else(|| panic!("{at}: no line"));
                let said = shown(id);
                assert!(
                    said.as_deref().is_some_and(|said| lines.contains(&said)),
                    "{at}: {said:?}"
                );
                assert!(
                    inside(drawn),
                    "{at}: the line {drawn:?} leaves {region} {within:?}"
                );
                let cut = cut_runs(cx, drawn);
                assert!(
                    cut.is_empty(),
                    "{at}: a clip cuts the line {drawn:?} in {region} {within:?}: {cut:?}"
                );
                if let Some(action) = action {
                    let control = cx
                        .debug_bounds(action)
                        .unwrap_or_else(|| panic!("{at}: no {action}"));
                    assert!(
                        inside(control),
                        "{at}: {action} {control:?} leaves {region} {within:?}"
                    );
                    assert!(tab_reaches(cx, action), "{at}: Tab never rings {action}");
                }
            }
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
        cx.simulate_resize(size(px(1280.), px(800.)));
        draw(cx);
    }

    /// At 18 pt in a 461 × 490 half tile, where the Git action bar is
    /// compact: what its edge drew, and the branch picker's icon.
    fn compact_edge(cx: &mut VisualTestContext) -> (Option<SharedString>, Option<SharedString>) {
        cx.update(|window, cx| appearance::apply_text_sizes(18, 12, window, cx));
        cx.simulate_resize(size(px(461.), px(490.)));
        crate::views::CONTROL_ICONS.with(|icons| icons.borrow_mut().clear());
        draw(cx);
        let drawn = (
            shown("branch-edge"),
            crate::views::CONTROL_ICONS.with(|icons| icons.borrow().get("current-branch").cloned()),
        );
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
        cx.simulate_resize(size(px(1280.), px(800.)));
        draw(cx);
        drawn
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        let control = cx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("no {selector}"));
        cx.simulate_click(control.center(), Modifiers::default());
        draw(cx);
    }

    /// A detached HEAD outranks a missing remote, which outranks an unborn
    /// branch; an upstream gives the counts instead.
    #[test]
    fn the_bar_names_the_outermost_edge() {
        let remote = gitturtle_core::Remote {
            name: "origin".into(),
            url: "local".into(),
            push_url: "local".into(),
        };
        let status = |branch: Option<&str>, head: Option<&str>, upstream: Option<&str>| {
            gitturtle_core::RepositoryStatus {
                branch: branch.map(Into::into),
                head: head.map(Into::into),
                upstream: upstream.map(Into::into),
                ..Default::default()
            }
        };
        let detached = status(None, Some("0123456789abcdef"), None);
        assert_eq!(
            RepositoryEdge::of(&detached, &[]),
            Some(RepositoryEdge::Detached {
                oid: "0123456789abcdef".into()
            })
        );
        assert_eq!(
            RepositoryEdge::of(&detached, &[]).unwrap().line(),
            "Detached at 0123456"
        );
        let unborn = status(Some("main"), None, None);
        assert_eq!(
            RepositoryEdge::of(&unborn, &[]),
            Some(RepositoryEdge::NoRemote)
        );
        assert_eq!(
            RepositoryEdge::of(&unborn, std::slice::from_ref(&remote)),
            Some(RepositoryEdge::Unborn)
        );
        let local = status(Some("main"), Some("0123456"), None);
        assert_eq!(
            RepositoryEdge::of(&local, std::slice::from_ref(&remote)),
            Some(RepositoryEdge::NoUpstream)
        );
        let tracking = status(Some("main"), Some("0123456"), Some("origin/main"));
        assert_eq!(
            RepositoryEdge::of(&tracking, std::slice::from_ref(&remote)),
            None
        );
    }

    /// No remote: the bar says so beside Add remote…, which opens the remote
    /// form and writes nothing.
    #[gpui::test]
    async fn no_remote_offers_add_remote_in_the_bar(cx: &mut TestAppContext) {
        let (_fixture, app, cx, _) = window(cx, &history_script(1, 1)).await;
        assert_eq!(
            app.read_with(cx, |app, _| app.repository_edge()),
            Some(RepositoryEdge::NoRemote)
        );
        // In the compact half tile at 18 pt "No remote" read "No r…" beside
        // the picker; there the bar draws the remote glyph instead.
        check_lines(
            cx,
            "branch-edge",
            &["No remote", "glyph:remote"],
            Some("edge-add-remote"),
            "git-actions",
        );
        assert_eq!(
            compact_edge(cx),
            (Some("glyph:remote".into()), Some("icons/branch.svg".into()))
        );
        // Beside Fetch's cloud, Add remote… draws a glyph of its own, not a
        // second cloud.
        let icon = crate::views::CONTROL_ICONS
            .with(|icons| icons.borrow().get("edge-add-remote").cloned())
            .expect("Add remote…'s icon");
        for cloud in ["icons/fetch.svg", "icons/remote.svg"] {
            assert_ne!(artwork(&icon), artwork(cloud), "Add remote… draws {cloud}");
        }
        click(cx, "edge-add-remote");
        assert!(
            cx.update(|window, cx| window.has_active_dialog(cx)),
            "Add remote… opens the remote form"
        );
    }

    /// The bytes of `path`, an app icon.
    fn artwork(path: &str) -> Vec<u8> {
        use gpui_kit::AssetSource as _;
        crate::Assets
            .load(path)
            .unwrap()
            .unwrap_or_else(|| panic!("{path} is an app asset"))
            .into_owned()
    }

    /// Whether a whole accent ring, a focused control's, surrounds `control`.
    fn ringed(cx: &mut VisualTestContext, control: Bounds<Pixels>) -> bool {
        let accent: Hsla = cx.read(|cx| rgb(palette(cx).accent).into());
        painted_outlines_where(cx, |color| {
            color.a > 0.
                && Hsla {
                    a: accent.a,
                    ..color
                } == accent
        })
        .iter()
        .any(|ring| {
            ring.whole
                && ring.bounds.contains(&control.center())
                && ring.bounds.size.width >= control.size.width
                && ring.bounds.size.width <= control.size.width + px(8.)
                && ring.bounds.size.height >= control.size.height
        })
    }

    /// No upstream: the bar says Push sets one, and Push is there to do it.
    #[gpui::test]
    async fn no_upstream_points_at_push(cx: &mut TestAppContext) {
        let (_fixture, app, cx, root) = window(cx, &history_script(1, 1)).await;
        fixture_git(&root, &["remote", "add", "origin", "../origin.git"]);
        refresh(&app, cx).await;
        // In a half tile the caption leaves before the bar takes a row.
        check_lines(
            cx,
            "branch-edge",
            &["No upstream · Push sets one", "No upstream", "glyph:remote"],
            Some("push"),
            "git-actions",
        );
        assert_eq!(compact_edge(cx).0.as_deref(), Some("glyph:remote"));
        assert_eq!(
            shown("branch-edge").as_deref(),
            Some("No upstream · Push sets one"),
            "the whole line where the bar has room"
        );
    }

    /// Detached HEAD: the bar names the commit beside Create branch…, which
    /// opens Targets on its branch field.
    #[gpui::test]
    async fn detached_head_offers_a_branch_here(cx: &mut TestAppContext) {
        let (_fixture, app, cx, root) = window(cx, &history_script(1, 1)).await;
        fixture_git(&root, &["checkout", "--detach"]);
        refresh(&app, cx).await;
        let head = app.read_with(cx, |app, _| {
            app.work_status
                .as_ref()
                .and_then(|status| status.head.clone())
                .unwrap()
        });
        let line = format!("Detached at {}", &head[..7]);
        // In the compact bar the picker names the commit, and the line says
        // only "Detached", or where even that would be cut, as at 18 pt in
        // a half tile, where the line was not drawn at all, its glyph.
        check_lines(
            cx,
            "branch-edge",
            &[&line, "Detached", "glyph:commit"],
            Some("edge-create-branch"),
            "git-actions",
        );
        assert_eq!(
            compact_edge(cx),
            (Some("glyph:commit".into()), Some("icons/commit.svg".into()))
        );
        click(cx, "edge-create-branch");
        let (open, field) = app.read_with(cx, |app, cx| {
            (
                app.git_actions_open,
                app.branch_name.read(cx).focus_handle(cx),
            )
        });
        assert!(open, "Create branch… opens Targets");
        assert!(
            cx.update(|window, _| field.is_focused(window)),
            "on its branch field"
        );
    }

    /// No commits yet: History points at Changes, where the composer makes
    /// the first one; Changes says no files yet, and with a remote the bar
    /// says there is nothing to push.
    #[gpui::test]
    async fn an_empty_repository_points_at_the_composer(cx: &mut TestAppContext) {
        let (_fixture, app, cx, root) = window(cx, "done\n").await;
        check(
            cx,
            "history-no-commits",
            "No commits yet · Your first commit starts in Changes",
            Some("history-open-changes"),
            "history-empty-state",
        );
        click(cx, "history-open-changes");
        settle(&app, cx).await;
        assert!(app.read_with(cx, |app, _| app.mode == WorkspaceMode::Working));
        check(cx, "working-clean", "No files yet", None, "working-pane");
        check(
            cx,
            "working-preview-clean",
            "No files yet · Files you add appear here for the first commit",
            None,
            "preview-content",
        );
        fixture_git(&root, &["remote", "add", "origin", "../origin.git"]);
        refresh(&app, cx).await;
        check_lines(
            cx,
            "branch-edge",
            &["No commits to push yet", "glyph:commit"],
            None,
            "git-actions",
        );
    }

    /// A clean working tree says so in the list and the preview beside it.
    #[gpui::test]
    async fn a_clean_working_tree_says_so(cx: &mut TestAppContext) {
        let (_fixture, app, cx, root) = window(cx, &history_script(1, 1)).await;
        fixture_git(&root, &["reset", "--hard"]);
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_working(window, cx)));
        settle(&app, cx).await;
        check(
            cx,
            "working-clean",
            "Working tree clean",
            None,
            "working-pane",
        );
        check(
            cx,
            "working-preview-clean",
            "Working tree clean · Edits you make appear here",
            None,
            "preview-content",
        );
    }

    /// A search with no match says how much it searched beside Clear search,
    /// which restores the list.
    #[gpui::test]
    async fn a_search_with_no_match_offers_clear_search(cx: &mut TestAppContext) {
        let (_fixture, app, cx, _) = window(cx, &history_script(1, 1)).await;
        search_for_nothing(&app, cx);
        wait(cx, "the search to finish", |cx| {
            app.read_with(cx, |app, _| app.history_search_no_match().is_some())
        });
        let line = app.read_with(cx, |app, _| app.history_search_no_match().unwrap());
        assert!(
            line.starts_with("No commits match · searched 1 commit in "),
            "{line}"
        );
        // In a short half tile the search's labelled controls once took
        // three rows and left History's list 2 px, so the line ran over the
        // header below them and Clear search ended below the window.
        check(
            cx,
            "history-no-match",
            &line,
            Some("history-clear-search"),
            "history-empty-state",
        );
        // There the line beside Clear search says how much the search read,
        // and the controls, Cancel and Continue disabled, leave it their
        // row's height.
        for interface in SIZES {
            cx.update(|window, cx| appearance::apply_text_sizes(interface, 12, window, cx));
            cx.simulate_resize(size(px(461.), px(490.)));
            draw(cx);
            assert!(
                cx.debug_bounds("history-search-controls").is_none(),
                "{interface} pt in 461 × 490: the controls stay over the state"
            );
        }
        wide_search_controls_unchanged(cx);
        click(cx, "history-clear-search");
        let (query, visible) = app.read_with(cx, |app, cx| {
            (app.search.read(cx).value().to_string(), app.visible.len())
        });
        assert_eq!(
            (query.as_str(), visible),
            ("", 1),
            "Clear search restores the list"
        );
    }

    /// Searches for what the fixture has no commit for, its debounce still
    /// to run: the test clock stands still until a test advances it.
    fn search_for_nothing(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.search.update(cx, |input, cx| {
                    input.set_value("no such commit", window, cx)
                });
                app.history_query_changed(window, cx);
            })
        });
    }

    /// The search's Cancel, Continue and Restart, and the accessible name
    /// each keeps.
    const SEARCH_CONTROLS: [(&str, &str); 3] = [
        ("history-search-cancel", "Cancel"),
        ("history-search-continue", "Continue"),
        ("history-search-restart", "Restart"),
    ];

    /// At 13 and 18 pt in a 461 × 490 half tile, the search's controls are
    /// one row of 28 px icons inside History's column, each named, and the
    /// summary has left them.
    fn narrow_search_controls(cx: &mut VisualTestContext) {
        for interface in SIZES {
            cx.update(|window, cx| appearance::apply_text_sizes(interface, 12, window, cx));
            cx.simulate_resize(size(px(461.), px(490.)));
            draw(cx);
            crate::views::COMPACT_NAMES.with(|names| names.borrow_mut().clear());
            draw(cx);
            let at = format!("{interface} pt in 461 × 490");
            let column = cx.debug_bounds("history-scope-toolbar").unwrap();
            let side = f32::from(cx.update(|_, _| appearance::ui_size(28.)));
            let mut row = None;
            for (id, name) in SEARCH_CONTROLS {
                let control = cx
                    .debug_bounds(id)
                    .unwrap_or_else(|| panic!("{at}: no {id}"));
                assert!(
                    (f32::from(control.size.width) - side).abs() < 0.5
                        && (f32::from(control.size.height) - side).abs() < 0.5,
                    "{at}: {id} {control:?} is not its {side} px icon"
                );
                assert!(
                    control.left() >= column.left() && control.right() <= column.right(),
                    "{at}: {id} {control:?} leaves History's column {column:?}"
                );
                assert_eq!(
                    *row.get_or_insert(control.top()),
                    control.top(),
                    "{at}: {id} wraps to a row of its own"
                );
                assert_eq!(
                    crate::views::COMPACT_NAMES.with(|names| names.borrow().get(id).copied()),
                    Some(name),
                    "{at}: {id}'s name"
                );
            }
            assert!(
                cx.debug_bounds("history-search-summary").is_none(),
                "{at}: the summary stays beside the icons"
            );
        }
    }

    /// At 13 pt in a 1,280 × 800 window the search's controls are as they
    /// were: the summary, the labelled Cancel, Continue and Restart beside it
    /// on one row, and the status below.
    fn wide_search_controls_unchanged(cx: &mut VisualTestContext) {
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
        cx.simulate_resize(size(px(1280.), px(800.)));
        draw(cx);
        crate::views::COMPACT_NAMES.with(|names| names.borrow_mut().clear());
        draw(cx);
        let summary = cx
            .debug_bounds("history-search-summary")
            .expect("the summary beside the controls");
        for (id, _) in SEARCH_CONTROLS {
            let control = cx.debug_bounds(id).unwrap();
            assert!(
                control.size.width > px(28.5) && control.top() >= summary.top() - px(8.),
                "{id} {control:?} keeps its label beside the summary {summary:?}"
            );
            assert!(
                crate::views::COMPACT_NAMES.with(|names| !names.borrow().contains_key(id)),
                "{id} is drawn as its icon"
            );
        }
        assert!(cx.debug_bounds("history-search-status").is_some());
    }

    /// A running search with nothing to list yet says so in one line in a
    /// half tile, its explanation the line's tooltip and description, where
    /// the title and its wrapped explanation once ran past the list; a wide
    /// window keeps both.
    #[gpui::test]
    async fn a_running_search_says_so_in_one_line_in_a_narrow_column(cx: &mut TestAppContext) {
        let (_fixture, app, cx, _) = window(cx, &history_script(1, 1)).await;
        search_for_nothing(&app, cx);
        draw(cx);
        assert!(app.read_with(cx, |app, _| app.history_search_pending()));
        check(
            cx,
            "history-search-state",
            "Searching repository history…",
            None,
            "history-empty-state",
        );
        narrow_search_controls(cx);
        let explanation = crate::native_accessibility::written::get("history-search-state-line")
            .and_then(|line| line.node.description().map(str::to_owned));
        assert!(
            explanation
                .as_deref()
                .is_some_and(|said| said.ends_with(" · literal message, author and hash search")),
            "the explanation is the line's description: {explanation:?}"
        );
        assert!(
            cx.debug_bounds("history-search-status").is_some(),
            "a running search keeps its status below the icons"
        );
        wide_search_controls_unchanged(cx);
        assert!(
            cx.debug_bounds("empty-state").is_some()
                && cx.debug_bounds("history-search-state-line").is_none(),
            "a wide window keeps the title and its explanation"
        );
        assert!(
            app.read_with(cx, |app, _| app.history_search_pending()),
            "the search still runs"
        );
    }

    /// A path filter with no match in the commit inspector says so beside
    /// Clear, which shows every file again.
    #[gpui::test]
    async fn a_path_filter_with_no_match_offers_clear(cx: &mut TestAppContext) {
        let (_fixture, app, cx, _) = window(cx, &history_script(1, 1)).await;
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.file_filter
                    .update(cx, |input, cx| input.set_value("no-such-path", window, cx));
                app.change_file_filter(cx);
            })
        });
        wait(cx, "the path filter", |cx| {
            app.read_with(cx, |app, _| !app.file_paths.pending)
        });
        check(
            cx,
            "files-no-match",
            "No matches",
            Some("files-clear-filter"),
            "commit-inspector-files",
        );
        click(cx, "files-clear-filter");
        wait(cx, "every file again", |cx| {
            cx.debug_bounds("files-no-match").is_none()
        });
        assert!(app.read_with(cx, |app, cx| app.file_filter.read(cx).value().is_empty()));
    }

    /// Tags and stashes with none saved say so in their lists.
    #[gpui::test]
    async fn empty_tags_and_stash_lists_say_so(cx: &mut TestAppContext) {
        let (_fixture, app, cx, _) = window(cx, &history_script(1, 1)).await;
        open_tags(&app, cx).await;
        check(
            cx,
            "tag-list-empty",
            "No local tags yet · Create tag… names a commit",
            None,
            "tags-list",
        );
        cx.update(|window, cx| window.close_all_dialogs(cx));
        draw(cx);
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_stashes(window, cx)));
        wait(cx, "the stash list", |cx| {
            cx.debug_bounds("stash-list-empty-line").is_some()
        });
        // Inside the list's visible part: at 18 pt in a 461 × 490 window the
        // line ran past it.
        check(
            cx,
            "stash-list-empty",
            "No saved stashes · Save one from the Actions menu in Changes",
            None,
            "stash-list",
        );
        // It starts where the rows' text does, their `px_3` in from the
        // list's side, rather than a rem in.
        for interface in SIZES {
            cx.update(|window, cx| appearance::apply_text_sizes(interface, 12, window, cx));
            cx.simulate_resize(size(px(461.), px(490.)));
            draw(cx);
            let (list, line) = (
                cx.debug_bounds("stash-list").unwrap(),
                cx.debug_bounds("stash-list-empty-line").unwrap(),
            );
            let inset = cx.update(|window, _| window.rem_size() * 0.75);
            assert!(
                (f32::from(line.left() - list.left()) - f32::from(inset)).abs() <= 0.5,
                "{interface} pt: the line {line:?} starts {:?} in from the list {list:?}",
                line.left() - list.left()
            );
        }
    }

    /// The GitHub panel of a repository with no GitHub remote names the next
    /// step, a destination, beside a control that focuses its field.
    #[gpui::test]
    async fn the_github_panel_without_a_remote_asks_for_a_destination(cx: &mut TestAppContext) {
        let (_fixture, app, cx, _) = window(cx, &history_script(1, 1)).await;
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_github(window, cx)));
        wait(cx, "the panel's local read", |cx| {
            cx.debug_bounds("github-choose-destination").is_some()
        });
        // Follow-up: in the short half tile at 18 pt the control lies below
        // the panel's fold, and Tab does not ring it whole there.
        check_in(
            cx,
            "github-list-empty",
            &["No GitHub remote · Name the repository to load its pull requests"],
            Some("github-choose-destination"),
            "github-panel",
            &[(461., 980.)],
        );
    }

    /// Tags' empty line starts at the dialog's one left edge, where the
    /// summary above it starts, at 13 and 18 pt: it stood 10 px right of it,
    /// a list padding's inset.
    #[gpui::test]
    async fn the_tags_line_starts_at_the_dialogs_edge(cx: &mut TestAppContext) {
        let (_fixture, app, cx, _) = window(cx, &history_script(1, 1)).await;
        open_tags(&app, cx).await;
        for interface in SIZES {
            for (width, height) in [(1280., 800.), WINDOWS[0]] {
                cx.update(|window, cx| appearance::apply_text_sizes(interface, 12, window, cx));
                cx.simulate_resize(size(px(width), px(height)));
                draw(cx);
                let line = cx.debug_bounds("tag-list-empty-line").expect("the line");
                let summary = cx.debug_bounds("tag-list-summary").expect("the summary");
                assert!(
                    (line.left() - summary.left()).abs() <= px(0.5),
                    "at {interface} pt in {width} × {height} the line starts at {:?}, the summary at {:?}",
                    line.left(),
                    summary.left()
                );
            }
        }
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// With no commit, a tag has nothing to name: Tags says so and points at
    /// the first commit, and Create tag… is unavailable rather than opening a
    /// form that cannot work.
    #[gpui::test]
    async fn tags_without_a_commit_point_at_the_first_commit(cx: &mut TestAppContext) {
        let (_fixture, app, cx, _) = window(cx, "done\n").await;
        open_tags(&app, cx).await;
        check(
            cx,
            "tag-list-empty",
            "No commits to tag yet · Your first commit starts in Changes",
            None,
            "tags-list",
        );
        click(cx, "create-tag");
        assert!(
            cx.debug_bounds("create-tag-dialog-title").is_none()
                && cx.debug_bounds("tag-list-empty-line").is_some(),
            "Create tag… opens nothing without a commit"
        );
    }

    /// Name repository moves focus to the destination field, a name field,
    /// so it draws no search magnifier; and where the panel's body has
    /// scrolled the field out of view, as at 18 pt in a half tile, the field
    /// comes into view with its ring, as it does for a Tab onto it.
    #[gpui::test]
    async fn name_repository_shows_the_field_it_focuses(cx: &mut TestAppContext) {
        let (_fixture, app, cx, _) = window(cx, &history_script(1, 1)).await;
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_github(window, cx)));
        wait(cx, "the panel's local read", |cx| {
            cx.debug_bounds("github-choose-destination").is_some()
        });
        crate::views::CONTROL_ICONS.with(|icons| icons.borrow_mut().clear());
        cx.update(|window, cx| appearance::apply_text_sizes(18, 12, window, cx));
        cx.simulate_resize(size(px(461.), px(490.)));
        draw(cx);
        let icon = crate::views::CONTROL_ICONS
            .with(|icons| icons.borrow().get("github-choose-destination").cloned());
        assert_eq!(icon, None, "Name repository draws no glyph");
        // Scroll the body to its end, then back until Name repository leads
        // the view, the field above it out of sight.
        let panel = cx.debug_bounds("github-panel").expect("the panel");
        let scroll = |cx: &mut VisualTestContext, by: Pixels| {
            cx.simulate_event(ScrollWheelEvent {
                position: panel.center(),
                delta: ScrollDelta::Pixels(point(px(0.), by)),
                modifiers: Modifiers::default(),
                touch_phase: TouchPhase::Moved,
            });
            draw(cx);
        };
        scroll(cx, px(-2000.));
        let action = cx
            .debug_bounds("github-choose-destination")
            .expect("the action");
        scroll(cx, panel.top() + px(8.) - action.top());
        let (field, action) = (
            cx.debug_bounds("github-destination").expect("the field"),
            cx.debug_bounds("github-choose-destination")
                .expect("the action"),
        );
        assert!(
            field.bottom() <= panel.top() && action.top() >= panel.top(),
            "the body shows Name repository {action:?} with the field {field:?} above {panel:?}"
        );
        click(cx, "github-choose-destination");
        draw(cx);
        let (field, panel) = (
            cx.debug_bounds("github-destination").expect("the field"),
            cx.debug_bounds("github-panel").expect("the panel"),
        );
        assert!(
            field.top() >= panel.top() && field.bottom() <= panel.bottom(),
            "the focused field {field:?} lies in the panel {panel:?}"
        );
        assert!(
            ringed(cx, field),
            "the focused field {field:?} draws its ring"
        );
        cx.update(|window, cx| appearance::apply_text_sizes(13, 12, window, cx));
    }

    /// A Changes path filter that matches nothing says so inside the list,
    /// where its rows start, with Clear filter below, in place of both
    /// groups' headings, and nothing above the list moves. The line once sat
    /// above the Changes header and pushed it 21 px at 13 pt, and at 18 pt in
    /// a half tile it was not drawn at all, with the Unstaged heading gone.
    /// Where the whole line would wrap it says "No matches", named by the
    /// whole line, and Clear filter is its icon where its label does not fit.
    /// Clear filter empties the field, lists every file again and moves to
    /// the list.
    #[gpui::test]
    async fn a_working_filter_with_no_match_says_so_in_the_list(cx: &mut TestAppContext) {
        use crate::workspace::{WORKING_NO_MATCH, WORKING_NO_MATCH_SHORT};
        let (_fixture, app, cx, root) = window(cx, &history_script(1, 1)).await;
        std::fs::write(root.join("notes.txt"), "changed\n").unwrap();
        std::fs::write(root.join("other.txt"), "other\n").unwrap();
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_working(window, cx)));
        settle(&app, cx).await;
        let filter = |cx: &mut VisualTestContext, query: &str| {
            cx.update(|window, cx| {
                app.update(cx, |app, cx| {
                    app.working_filter
                        .update(cx, |input, cx| input.set_value(query, window, cx));
                    app.filter_working_paths(window, cx);
                })
            });
            wait(cx, "the path filter", |cx| {
                app.read_with(cx, |app, _| !app.working_paths.pending)
            });
        };
        // What lies above the list, and the list's top.
        let above = |cx: &mut VisualTestContext| {
            [
                "working-selection",
                "working-header",
                "working-toolbar",
                "working-pane",
            ]
            .map(|name| cx.debug_bounds(name).map(|bounds| (name, bounds.top())))
        };
        for (interface, width, height, line, labelled) in [
            (13, 1280., 800., WORKING_NO_MATCH, true),
            (18, 461., 490., WORKING_NO_MATCH_SHORT, false),
        ] {
            let at = format!("{interface} pt in {width} × {height}");
            cx.update(|window, cx| appearance::apply_text_sizes(interface, 12, window, cx));
            cx.simulate_resize(size(px(width), px(height)));
            draw(cx);
            let unfiltered = above(cx);
            assert!(cx.debug_bounds("working-row-1").is_some(), "{at}: the rows");

            filter(cx, "zzz");
            assert_eq!(above(cx), unfiltered, "{at}: nothing above the list moves");
            assert_eq!(shown("working-no-match").as_deref(), Some(line), "{at}");
            let pane = cx.debug_bounds("working-pane").unwrap();
            let drawn = cx.debug_bounds("working-no-match-line").unwrap();
            let clear = cx.debug_bounds("working-clear-filter").unwrap();
            let inset = cx.update(|window, _| window.rem_size() * 0.75) + px(1.);
            assert!(
                (drawn.left() - (pane.left() + inset)).abs() <= px(0.5),
                "{at}: the line {drawn:?} starts at the rows' text inset in {pane:?}"
            );
            for (what, bounds) in [("the line", drawn), ("Clear filter", clear)] {
                assert!(
                    pane.contains(&bounds.origin)
                        && bounds.right() <= pane.right()
                        && bounds.bottom() <= pane.bottom(),
                    "{at}: {what} {bounds:?} lies in the list {pane:?}"
                );
            }
            assert!(clear.top() >= drawn.bottom(), "{at}: Clear filter is below");
            assert!(
                cx.debug_bounds("working-row-0").is_none(),
                "{at}: neither heading is drawn"
            );
            assert_eq!(
                // An icon is its pixel-snapped 28 px square.
                clear.size.width > appearance::ui_size(28.) + px(1.),
                labelled,
                "{at}: Clear filter {clear:?} labelled"
            );

            click(cx, "working-clear-filter");
            wait(cx, "every file again", |cx| {
                app.read_with(cx, |app, _| !app.working_paths.pending)
            });
            let (query, focused) = cx.update(|window, cx| {
                let app = app.read(cx);
                (
                    app.working_filter.read(cx).value().to_string(),
                    app.file_focus.is_focused(window),
                )
            });
            assert_eq!(query, "", "{at}: Clear filter empties the field");
            assert!(focused, "{at}: and moves to the list");
            assert!(cx.debug_bounds("working-no-match").is_none(), "{at}");
            assert!(
                cx.debug_bounds("working-row-1").is_some(),
                "{at}: the rows again"
            );
            assert_eq!(above(cx), unfiltered, "{at}");
        }
        filter(cx, "zzz");
        check_lines(
            cx,
            "working-no-match",
            &[WORKING_NO_MATCH, WORKING_NO_MATCH_SHORT],
            Some("working-clear-filter"),
            "working-pane",
        );
    }
}
