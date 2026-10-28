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
const GAP: f32 = 8.;
/// The widest a pane's line runs before it wraps, at the default interface
/// size.
const LINE_MAX: f32 = 480.;

#[cfg(test)]
thread_local! {
    /// The line each state drew last, by its id, for a test to read back.
    static SHOWN: std::cell::RefCell<HashMap<&'static str, SharedString>> =
        Default::default();
}

/// The line the state `id` drew last; a test pairs it with the line's
/// bounds in the last frame.
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
        }
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
        SHOWN.with(|shown| shown.borrow_mut().insert(id, line.clone()));
        let name = self.name();
        let tooltip = self.tooltip.clone().map(|explanation| -> SharedString {
            if name == line {
                explanation
            } else {
                format!("{name} · {explanation}").into()
            }
        });
        div()
            .id(SharedString::from(format!("{id}-line")))
            .debug_selector(move || format!("{id}-line"))
            .when_some(self.tooltip.clone(), |line, explanation| {
                line.aria_description(explanation)
            })
            .text_size(appearance::ui_text(LINE_TEXT))
            .text_color(rgb(palette(cx).muted))
            .when(self.truncate, |line| line.min_w_0().truncate())
            .when_some(tooltip, |line, tooltip| {
                line.tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
            })
            .child(line)
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

/// An empty or edge state's action: the shared 28 px control with its icon
/// and label, resting on `subtle` so that on an empty surface it reads as a
/// control. Its label is its accessible name.
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
}

impl EdgeForm {
    /// The forms the bar tries, richest first; the last always fits as well
    /// as any.
    pub(crate) const ORDER: [Self; 3] = [
        Self {
            caption: true,
            labelled: true,
        },
        Self {
            caption: true,
            labelled: false,
        },
        Self {
            caption: false,
            labelled: false,
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
    symbol: "remote",
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
        check(
            cx,
            "branch-edge",
            "No remote",
            Some("edge-add-remote"),
            "git-actions",
        );
        click(cx, "edge-add-remote");
        assert!(
            cx.update(|window, cx| window.has_active_dialog(cx)),
            "Add remote… opens the remote form"
        );
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
            &["No upstream · Push sets one", "No upstream"],
            Some("push"),
            "git-actions",
        );
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
        check(
            cx,
            "branch-edge",
            &line,
            Some("edge-create-branch"),
            "git-actions",
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
        check(
            cx,
            "branch-edge",
            "No commits to push yet",
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
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.search.update(cx, |input, cx| {
                    input.set_value("no such commit", window, cx)
                });
                app.history_query_changed(window, cx);
            })
        });
        wait(cx, "the search to finish", |cx| {
            app.read_with(cx, |app, _| app.history_search_no_match().is_some())
        });
        let line = app.read_with(cx, |app, _| app.history_search_no_match().unwrap());
        assert!(
            line.starts_with("No commits match · searched 1 commit in "),
            "{line}"
        );
        // In the short half tile at 13 pt the search's own controls and the
        // full chrome leave History's list no height, a limit of the search
        // row this state does not own; the tall one shows it.
        check_in(
            cx,
            "history-no-match",
            &[&line],
            Some("history-clear-search"),
            "history-empty-state",
            &[(461., 980.)],
        );
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
        cx.update(|window, cx| app.update(cx, |app, cx| app.open_tags(window, cx)));
        wait(cx, "the Tags list", |cx| {
            cx.debug_bounds("tag-list-empty-line").is_some()
        });
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
        check(
            cx,
            "stash-list-empty",
            "No saved stashes · Save one from the Actions menu in Changes",
            None,
            "stash-list-empty",
        );
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
}
