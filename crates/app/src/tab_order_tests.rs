//! The Tab-order audit: each page walked with Tab and again with Shift+Tab
//! at 13 pt in a 1,480 × 973 window, from the window's root until focus
//! comes round, its stops recorded by name. Every list that holds a
//! keyboard selection, and the commit message's text, is a stop met once
//! each way, in the page's visual order (DESIGN.md, one responsive order),
//! and the sweep's checks run at every stop, so each shows its ring whole
//! (`focus_ring::sweep`). Shift+Tab meets the stops Tab meets, in the other
//! order. GPUI ignores a div's `tab_stop` where the div tracks a focus
//! handle of its own, and the handle's own flag decides; the lists' handles
//! were made without it, so Tab passed over History's commit list, the
//! changed files, Working Changes' files, File history's revisions, the
//! navigator, the project list and Blame's lines, and over the message.
//!
//! The dialogs' selection lists, whose rows are each a Button and so each a
//! stop, are walked the same way at the end, each named by the list that
//! holds the stop.

use crate::focus_ring::{PaintedOutline, sweep};
use crate::views::tests::{history_window, history_window_from, settle};
use crate::*;
use core::prelude::v1::test;

/// The view whose Working list's row controls name their stops, and each
/// control's selector with its name.
type WorkingControls = (Entity<GitTurtle>, Vec<(String, &'static str)>);

/// What names a stop: a focus handle the app keeps, or a drawn region,
/// found by its debug selector, that holds the stop's ring.
#[derive(Default)]
struct Names {
    handles: Vec<(FocusHandle, &'static str)>,
    regions: Vec<(&'static str, &'static str)>,
    /// The Working list's row controls by their selector, named from the
    /// handle holding focus: a region would also claim whatever ring lies
    /// where a scrolled body has carried the control.
    working: Option<WorkingControls>,
}

impl Names {
    fn handle(mut self, handle: Option<FocusHandle>, name: &'static str) -> Self {
        if let Some(handle) = handle {
            self.handles.push((handle, name));
        }
        self
    }

    fn region(mut self, selector: &'static str, name: &'static str) -> Self {
        self.regions.push((selector, name));
        self
    }

    /// Names the Working list's row controls, `controls` by their selector
    /// (`GitTurtle::focused_row_control`).
    fn working_controls(
        mut self,
        app: Entity<GitTurtle>,
        controls: Vec<(String, &'static str)>,
    ) -> Self {
        self.working = Some((app, controls));
        self
    }

    /// The name of the stop holding focus, whose ring is `ring`: its
    /// handle's, or the row control's holding focus, else the first region
    /// holding the ring's centre, else the key contexts around it and where
    /// its ring lies.
    fn of(&self, cx: &mut VisualTestContext, ring: &[PaintedOutline]) -> String {
        let focused = cx.update(|window, cx| window.focused(cx));
        if let Some((_, name)) = self
            .handles
            .iter()
            .find(|(handle, _)| Some(handle) == focused.as_ref())
        {
            return (*name).to_owned();
        }
        if let Some((app, controls)) = &self.working {
            let held = cx.update(|window, cx| app.read(cx).focused_row_control(window, cx));
            if let Some((_, name)) =
                held.and_then(|held| controls.iter().find(|(selector, _)| *selector == held))
            {
                return (*name).to_owned();
            }
        }
        let centre = ring.first().map(|outline| outline.bounds.center());
        if let Some(centre) = centre {
            for (selector, name) in &self.regions {
                if cx
                    .debug_bounds(selector)
                    .is_some_and(|region| region.contains(&centre))
                {
                    return (*name).to_owned();
                }
            }
        }
        let contexts = cx.update(|window, _| {
            window
                .context_stack()
                .iter()
                .rev()
                .take(2)
                .map(|context| format!("{context:?}"))
                .collect::<Vec<_>>()
                .join(" < ")
        });
        match centre {
            Some(centre) => format!(
                "({contexts}) at {:.0}, {:.0}",
                f32::from(centre.x),
                f32::from(centre.y)
            ),
            None => format!("({contexts})"),
        }
    }
}

/// Opens the audit's window: 1,480 × 973 at 13 pt, Midnight, the app's key
/// bindings and motion reduced.
fn prepare(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) {
    prepare_at(app, cx, size(px(1480.), px(973.)), 13);
}

/// [`prepare`] with a window of `window` at `interface` pt, as a 461 × 490
/// half tile at 18 pt.
fn prepare_at(
    app: &Entity<GitTurtle>,
    cx: &mut VisualTestContext,
    window: Size<Pixels>,
    interface: u8,
) {
    sweep::prepare(app, cx);
    cx.simulate_resize(window);
    sweep::interface_size(cx, interface);
}

/// A stop a walk met: the handle holding focus there and its name.
type Stop = (Option<FocusHandle>, String);

/// How [`audit`] walks a surface.
#[derive(Clone, Copy)]
struct Walk {
    /// The interface size the window is drawn at, in points, which names
    /// each stop.
    interface: u8,
    /// Whether each list is met once each way, else in one run of stops, as
    /// a list of Buttons is, one stop a row.
    once: bool,
    /// Whether the walks start at the stop holding focus, as a dialog's do
    /// at the stop it opened on, rather than at the window's root.
    from_focus: bool,
}

impl Walk {
    /// A page, walked from the window's root at 13 pt.
    const PAGE: Walk = Walk {
        interface: 13,
        once: true,
        from_focus: false,
    };

    /// A dialog, walked from the stop it opened on at 13 pt, each of its
    /// lists met `once`, else in one run of stops.
    fn dialog(once: bool) -> Walk {
        Walk {
            once,
            from_focus: true,
            ..Walk::PAGE
        }
    }

    /// This walk with the window drawn at `interface` pt.
    fn at(self, interface: u8) -> Walk {
        Walk { interface, ..self }
    }
}

/// The stops `key` meets on the shown page from `start`, or from the
/// window's root, until focus comes round, with every failure of the
/// sweep's checks at a stop added to `failures`, named by `surface`,
/// `walk`'s interface size, the stop and `key` ([`sweep::place`]), and, at
/// a stop named in `lists`, what is painted over its ring after it.
#[allow(clippy::too_many_arguments)]
fn stops(
    app: &Entity<GitTurtle>,
    cx: &mut VisualTestContext,
    key: &str,
    surface: &str,
    walk: Walk,
    names: &Names,
    lists: &[&str],
    start: Option<FocusHandle>,
    failures: &mut Vec<String>,
) -> Vec<Stop> {
    let start = start.unwrap_or_else(|| app.read_with(cx, |app, _| app.app_focus.clone()));
    cx.update(|window, cx| window.focus(&start, cx));
    views::tests::draw(cx);
    let mut met = Vec::new();
    let mut covered = Vec::new();
    sweep::walk_each(
        app,
        cx,
        key,
        surface,
        walk.interface,
        failures,
        &mut |cx, at, ring| {
            let focused = cx.update(|window, cx| window.focused(cx));
            let name = names.of(cx, ring);
            // A list's ring is whole inside its clips, and nothing is painted
            // over it after: the kit's divider beside the project pane covered
            // the navigator's left side.
            if lists.contains(&name.as_str()) {
                for outline in ring {
                    let over = focus_ring::painted_over(cx, outline);
                    if !over.is_empty() {
                        covered.push(format!(
                            "{at}: quads painted after its ring at {:?} cover it: {over:?}",
                            outline.bounds
                        ));
                    }
                }
            }
            met.push((focused, name));
        },
    );
    failures.extend(covered);
    met
}

/// Walks the shown surface both ways, from the window's root or, as `walk`
/// says, from the stop holding focus, prints what each walk met for the
/// audit's record, and checks that every name in `lists` is met once each
/// way (or, as `walk` says, in one run of stops), in that order with Tab
/// and the other with Shift+Tab, and that Shift+Tab meets the stops Tab
/// does in the other order. Returns the names Tab and Shift+Tab met, Tab's
/// from the walk's start.
fn audit(
    app: &Entity<GitTurtle>,
    cx: &mut VisualTestContext,
    surface: &str,
    names: &Names,
    lists: &[&str],
    walk: Walk,
    failures: &mut Vec<String>,
) -> (Vec<String>, Vec<String>) {
    let start = walk
        .from_focus
        .then(|| cx.update(|window, cx| window.focused(cx)))
        .flatten();
    let mut forward = stops(
        app,
        cx,
        "tab",
        surface,
        walk,
        names,
        lists,
        start.clone(),
        failures,
    );
    let backward = stops(
        app,
        cx,
        "shift-tab",
        surface,
        walk,
        names,
        lists,
        start.clone(),
        failures,
    );
    // From a stop, s1, Tab meets s2 … sN and comes back to s1, and Shift+Tab
    // meets sN … s2, s1: turned to begin at s1, Tab's walk is Shift+Tab's
    // reversed. From the window's root, or a dialog's shell where it opens
    // with focus there, no stop, the walks need no turn.
    if let Some(start) = &start
        && let Some(at) = forward
            .iter()
            .position(|(handle, _)| handle.as_ref() == Some(start))
    {
        forward.rotate_left(at);
    }
    let named =
        |walk: &[Stop]| -> Vec<String> { walk.iter().map(|(_, name)| name.clone()).collect() };
    println!(
        "{surface}, Tab: {:#?}\n{surface}, Shift+Tab: {:#?}",
        named(&forward),
        named(&backward)
    );
    for (key, met) in [("Tab", &forward), ("Shift+Tab", &backward)] {
        for list in lists {
            let at: Vec<usize> = met
                .iter()
                .enumerate()
                .filter_map(|(index, (_, name))| (name == list).then_some(index))
                .collect();
            let run = at
                .first()
                .zip(at.last())
                .is_some_and(|(first, last)| last - first + 1 == at.len());
            if (walk.once && at.len() != 1) || at.is_empty() || !run {
                failures.push(format!("{surface}: {key} meets {list} at {at:?}"));
            }
        }
    }
    // Each list at its first stop, in the surface's visual order from the
    // walk's start.
    let order = |met: &[Stop]| -> Vec<String> {
        let mut order: Vec<String> = Vec::new();
        for (_, name) in met {
            if lists.contains(&name.as_str()) && order.last() != Some(name) {
                order.push(name.clone());
            }
        }
        order
    };
    let expected: Vec<String> = lists.iter().map(|list| (*list).to_owned()).collect();
    if order(&forward) != expected {
        failures.push(format!(
            "{surface}: Tab meets the lists as {:?}, not {expected:?}",
            order(&forward)
        ));
    }
    let mut back = order(&backward);
    back.reverse();
    if back != expected {
        failures.push(format!(
            "{surface}: Shift+Tab meets the lists as {back:?} reversed, not {expected:?}"
        ));
    }
    // The same stops the other way round, compared by the handle holding
    // focus at each.
    let handles = |met: &[Stop]| -> Vec<Option<FocusHandle>> {
        met.iter().map(|(handle, _)| handle.clone()).collect()
    };
    let mut reversed = handles(&backward);
    reversed.reverse();
    if reversed != handles(&forward) {
        failures.push(format!(
            "{surface}: Shift+Tab meets {:?} reversed, Tab {:?}",
            named(&backward),
            named(&forward)
        ));
    }
    (named(&forward), named(&backward))
}

/// The names of History's and Compare's stops that the app keeps handles
/// for.
fn repository_names(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) -> Names {
    cx.update(|_, cx| {
        let app = app.read(cx);
        Names::default()
            .handle(Some(app.nav_focus.clone()), "the navigator")
            .handle(Some(app.search.read(cx).focus_handle(cx)), "the search")
            .handle(Some(app.focus.clone()), "the commit list")
            .handle(app.inspector_message.focus(), "the commit message")
            .handle(Some(app.file_focus.clone()), "the file list")
    })
}

/// [`repository_names`], with every other stop named by its own debug
/// selector where it has one (the tab strip's controls, the Git actions and
/// Compare's review options), else by the part of the window that holds
/// it: the tab strip, the repository's header, the sidebar, History's scope
/// bar, the message's toolbar, the files' header, Compare's toolbar, text
/// and footer, and the status bar. A stop named by where its ring lay moved
/// with the controls before it, and the strip's Main Menu is Linux's alone:
/// macOS keeps it in its menu bar.
fn page_names(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) -> Names {
    repository_names(app, cx)
        .region("main-menu", "the Main Menu")
        .region("repository-tab-0", "the repository's tab")
        .region("repository-tab-close-0", "the tab's Close")
        .region("new-repository-tab", "Open repository tab")
        .region("repository-workspaces", "Workspaces")
        .region("current-branch", "the branch picker")
        .region("edge-add-remote", "Add remote")
        .region("git-actions-toggle", "Targets")
        .region("previous-text-change", "Previous change")
        .region("next-text-change", "Next change")
        .region("hide-whitespace-changes", "Hide whitespace")
        .region("expand-text-context", "More context")
        .region("repository-tab-strip", "a tab strip control")
        .region("git-actions", "a Git actions control")
        .region("text-review-options", "a review option")
        .region("repository-rail", "a rail control")
        .region("repository-header", "a header control")
        .region("history-sidebar", "a sidebar control")
        .region("history-scope-toolbar", "a scope control")
        .region("commit-message-toolbar", "a message control")
        .region("changed-files-header", "a files header control")
        .region("commit-inspector-files", "a files control")
        .region("compare-toolbar", "a comparison control")
        .region("compare-footer", "a comparison footer control")
        .region("preview-content", "the comparison's text")
        .region("status-bar", "a status bar control")
}

/// Fails unless Tab met `expected`, stop for stop: the whole order, so a
/// stop that moves, leaves or comes in is seen, not only the lists'.
fn pin(surface: &str, met: &[String], expected: &[&str], failures: &mut Vec<String>) {
    if met != expected {
        failures.push(format!("{surface}: Tab meets {met:#?}\nnot {expected:#?}"));
    }
}

/// Known failures for [`sweep::assert_whole_except`]: `finding` at each of
/// `tab`'s stops by Tab and `shift_tab`'s by Shift+Tab, on `surface` at
/// `interface` pt, each named by its key ([`sweep::place`]).
fn known_at(
    surface: &str,
    interface: u8,
    tab: &[usize],
    shift_tab: &[usize],
    finding: &'static str,
) -> Vec<(String, &'static str)> {
    let at = |key, stops: &[usize]| -> Vec<(String, &'static str)> {
        stops
            .iter()
            .map(|stop| (sweep::place(surface, interface, *stop, key), finding))
            .collect()
    };
    [at("tab", tab), at("shift-tab", shift_tab)].concat()
}

#[gpui::test]
async fn history_meets_each_list_and_the_message_once_in_order(cx: &mut TestAppContext) {
    let (_fixture, app, cx) = history_window(cx).await;
    prepare(&app, cx);
    let names = page_names(&app, cx);
    let mut failures = Vec::new();
    let (met, _) = audit(
        &app,
        cx,
        "History",
        &names,
        &[
            "the navigator",
            "the search",
            "the commit list",
            "the commit message",
            "the file list",
        ],
        Walk::PAGE,
        &mut failures,
    );
    pin("History", &met, &history_stops(), &mut failures);
    sweep::assert_whole(failures);
}

/// The stops Linux's tab strip draws before the repository's tab: the Main
/// Menu, which macOS keeps in its menu bar. A page's Tab stops from the
/// window's root come one later for it on Linux; its Shift+Tab stops, which
/// meet the Main Menu last, do not.
const MAIN_MENU: &[&str] = if cfg!(target_os = "linux") {
    &["the Main Menu"]
} else {
    &[]
};

/// Tab's stops `stops`, counted from the window's root as macOS meets them,
/// where the platform meets them: on Linux one later, past the Main Menu.
fn past_the_menu<const N: usize>(stops: [usize; N]) -> [usize; N] {
    stops.map(|stop| stop + MAIN_MENU.len())
}

/// The tab strip's stops, in Tab's order from the window's root: on Linux
/// the Main Menu, then the repository's tab, its Close, Open repository tab
/// and Workspaces.
fn tab_strip_stops() -> Vec<&'static str> {
    let mut stops = MAIN_MENU.to_vec();
    stops.extend([
        "the repository's tab",
        "the tab's Close",
        "Open repository tab",
        "Workspaces",
    ]);
    stops
}

/// The Git actions' stops below the repository's header, a fixture with no
/// remote offering Add remote among them.
const GIT_ACTION_STOPS: &[&str] = &["the branch picker", "Add remote", "Targets"];

/// Every stop Tab meets in History from the window's root, in order.
fn history_stops() -> Vec<&'static str> {
    let mut stops = tab_strip_stops();
    stops.extend(["a header control"; 6]);
    stops.extend(GIT_ACTION_STOPS);
    stops.extend(["a sidebar control"; 4]);
    stops.extend([
        "the navigator",
        "a scope control",
        "a scope control",
        "the search",
        "the commit list",
    ]);
    stops.extend(["a message control"; 3]);
    stops.extend(["the commit message", "a files control", "the file list"]);
    stops
}

/// Every stop Tab meets in Compare from the window's root, in order.
fn compare_stops() -> Vec<&'static str> {
    let mut stops = tab_strip_stops();
    stops.extend(["a header control"; 7]);
    stops.extend(GIT_ACTION_STOPS);
    stops.push("a rail control");
    stops.extend(["a comparison control"; 7]);
    stops.extend([
        "Previous change",
        "Next change",
        "Hide whitespace",
        "More context",
        "the comparison's text",
    ]);
    stops.extend(["a message control"; 3]);
    stops.extend(["the commit message", "a files control", "the file list"]);
    stops
}

#[gpui::test]
async fn compare_and_blame_meet_their_lists_once_in_order(cx: &mut TestAppContext) {
    let (_fixture, app, cx) = history_window(cx).await;
    prepare(&app, cx);
    cx.update(|window, cx| app.update(cx, |app, cx| app.select_file(0, window, cx)));
    settle(&app, cx).await;
    let names = page_names(&app, cx);
    let mut failures = Vec::new();
    let (met, _) = audit(
        &app,
        cx,
        "Compare",
        &names,
        &["the commit message", "the file list"],
        Walk::PAGE,
        &mut failures,
    );
    pin("Compare", &met, &compare_stops(), &mut failures);
    cx.update(|window, cx| app.update(cx, |app, cx| app.open_blame(0, window, cx)));
    settle(&app, cx).await;
    let blame = app.read_with(cx, |app, _| app.blame.focus());
    let names = repository_names(&app, cx).handle(blame, "the attribution lines");
    audit(
        &app,
        cx,
        "Blame",
        &names,
        &[
            "the attribution lines",
            "the commit message",
            "the file list",
        ],
        Walk::PAGE,
        &mut failures,
    );
    sweep::assert_whole(failures);
}

/// The colour of the 2 px leading marker the inspector's selected file row
/// paints in the last frame, none where it paints it transparent.
fn selected_row_marker(cx: &mut VisualTestContext) -> Option<Hsla> {
    let row = cx
        .debug_bounds("selected-changed-file")
        .expect("the selected file's row is drawn");
    cx.update(|window, _| {
        let scale = window.scale_factor();
        let near = |drawn: f32, laid: Pixels| (drawn / scale - f32::from(laid)).abs() <= 0.5;
        window
            .painted_quads()
            .into_iter()
            .filter(|quad| {
                near(quad.bounds.left().as_f32(), row.left())
                    && near(quad.bounds.top().as_f32(), row.top())
                    && near(quad.bounds.right().as_f32(), row.right())
                    && near(quad.bounds.bottom().as_f32(), row.bottom())
                    && quad.border_widths.left.as_f32() > 0.
                    && !quad.border_color.is_transparent()
            })
            .map(|quad| quad.border_color)
            .next_back()
    })
}

/// While Blame's lines show their keyboard ring, the selected file's row in
/// the inspector beside them leaves its 2 px marker to the ring, keeping
/// its selected fill, as it does beside Compare's text: at 13 pt in a 461
/// px window the two read as one doubled stroke (DESIGN.md, selected row).
/// The marker returns once the file list takes the ring.
#[gpui::test]
async fn blame_s_ring_takes_the_selected_rows_marker(cx: &mut TestAppContext) {
    let (_fixture, app, cx) = history_window(cx).await;
    sweep::prepare(&app, cx);
    cx.simulate_resize(size(px(461.), px(490.)));
    cx.update(|window, cx| app.update(cx, |app, cx| app.select_file(0, window, cx)));
    settle(&app, cx).await;
    cx.update(|window, cx| app.update(cx, |app, cx| app.open_blame(0, window, cx)));
    blame::lands(&app, cx);
    let accent: Hsla = cx.read(|cx| rgb(palette(cx).accent).into());
    let (lines, list) = app.read_with(cx, |app, _| {
        (
            app.blame.focus().expect("Blame's lines"),
            app.file_focus.clone(),
        )
    });
    assert_eq!(selected_row_marker(cx), Some(accent), "at rest");
    cx.update(|window, cx| window.focus(&lines, cx));
    cx.simulate_keystrokes("down");
    views::tests::draw(cx);
    assert!(
        app.read_with(cx, |app, _| app.focus_ring.shows(&lines)),
        "Blame's lines show their ring"
    );
    assert_eq!(selected_row_marker(cx), None, "beside Blame's ring");
    cx.update(|window, cx| window.focus(&list, cx));
    cx.simulate_keystrokes("down");
    views::tests::draw(cx);
    assert_eq!(
        selected_row_marker(cx),
        Some(accent),
        "the file list holds the ring"
    );
}

/// Blame's lines and the rendered Markdown at 18 pt in a 461 × 490 half
/// tile, where their panes are narrowest: each a stop met once both ways,
/// its ring whole on its own edge, as at 13 pt in the full window. Every
/// stop both ways counts: no sweep walks the page in the half tile, so the
/// rings still awaiting their fix are pinned here, each by its key, its
/// stop and what fails.
#[gpui::test]
async fn blame_and_the_rendered_markdown_ring_whole_in_a_half_tile_at_18_pt(
    cx: &mut TestAppContext,
) {
    let (_fixture, app, cx) = history_window_from(cx, MARKDOWN_SCRIPT).await;
    prepare_at(&app, cx, size(px(461.), px(490.)), 18);
    let rendered = show_rendered_markdown(&app, cx).await;
    let names = repository_names(&app, cx).handle(Some(rendered), "the rendered Markdown");
    let mut failures = Vec::new();
    let walk = Walk::PAGE.at(18);
    audit(
        &app,
        cx,
        "Compare in Rendered",
        &names,
        &["the rendered Markdown"],
        walk,
        &mut failures,
    );
    cx.update(|window, cx| app.update(cx, |app, cx| app.open_blame(0, window, cx)));
    blame::lands(&app, cx);
    let blame = app.read_with(cx, |app, _| app.blame.focus());
    let names = repository_names(&app, cx).handle(blame, "the attribution lines");
    audit(
        &app,
        cx,
        "Blame",
        &names,
        &["the attribution lines"],
        walk,
        &mut failures,
    );
    // Awaiting their follow-ups (FU30-C2's report): the rendered Markdown's
    // side headings wrap past the 188.5 px pane, whose clip cuts both
    // sides' heading Buttons' rings on every side, and Blame's footer, a row
    // that does not wrap, lies past its short pane's foot, where its four
    // Buttons' rings are clipped away.
    let known = [
        known_at(
            "Compare in Rendered",
            18,
            &past_the_menu([21, 25]),
            &[6, 10],
            "a clip cuts the top, right, bottom, left of its ring",
        ),
        known_at(
            "Blame",
            18,
            &past_the_menu([18, 19, 20, 21]),
            &[6, 7, 8, 9],
            "the focused element paints no ring",
        ),
        // That footer leaves Blame's lines 2 px, and a border painted after
        // their ring lies over its top.
        known_at(
            "Blame",
            18,
            &past_the_menu([17]),
            &[10],
            "painted after its ring",
        ),
    ];
    sweep::assert_whole_except(failures, &known.concat());
}

/// The captured local document a rendered Markdown's link opens, in its
/// dialog: its rendered text, the dialog's one side, is a stop met both
/// ways from where the dialog puts focus, its ring whole on the view's own
/// edge, at 13 pt in the full window and at 18 pt in a half tile. Every
/// stop both ways counts: no sweep walks this dialog, so the rings still
/// awaiting their fix are pinned here, each by its key, its stop and what
/// fails.
#[gpui::test]
async fn the_local_document_s_markdown_is_met_both_ways(cx: &mut TestAppContext) {
    let (_fixture, app, cx) = history_window(cx).await;
    let mut failures = Vec::new();
    for (window, interface) in [
        (size(px(1480.), px(973.)), 13),
        (size(px(461.), px(490.)), 18),
    ] {
        prepare_at(&app, cx, window, interface);
        settle(&app, cx).await;
        let ring = app.read_with(cx, |app, _| app.focus_ring.clone());
        let rendered = cx.update(|window, cx| markdown_view::open_local_markdown(ring, window, cx));
        settle(&app, cx).await;
        views::tests::draw(cx);
        assert!(
            cx.update(|window, _| rendered.is_focused(window)),
            "the document opens with focus on its Markdown"
        );
        let names = Names::default().handle(Some(rendered), "the document's Markdown");
        audit(
            &app,
            cx,
            "The local document",
            &names,
            &["the document's Markdown"],
            Walk::dialog(true).at(interface),
            &mut failures,
        );
        sweep::close_dialogs(cx, "The local document", interface, &mut failures);
    }
    // Awaiting their follow-ups (FU30-C2's report): the row of Exact source
    // and Rendered keeps no ring room inside the dialog body's clip, which
    // cuts both toggles' rings at the top and sides at both sizes, Tab's
    // fifth and sixth stops and Shift+Tab's first two; and in the half tile
    // the document keeps 65 % of the window's height, taller than the
    // dialog's capped body, whose clip cuts the view and the ring on its
    // edge, the last stop both ways.
    let toggles = "a clip cuts the top, right, left of its ring";
    let known = [
        known_at("The local document", 13, &[4, 5], &[0, 1], toggles),
        known_at("The local document", 18, &[4, 5], &[0, 1], toggles),
        known_at(
            "The local document",
            18,
            &[6],
            &[6],
            "a clip cuts the right, bottom, left of its ring",
        ),
    ];
    sweep::assert_whole_except(failures, &known.concat());
}

/// A commit adding a Markdown file, `README.md`.
const MARKDOWN_SCRIPT: &str = "commit refs/heads/main\ncommitter Fixture <fixture@example.invalid> 1700000000 +0000\ndata 13\nfixture notes\nM 100644 inline README.md\ndata 19\n# Notes\n\nSome text\n\ndone\n";

/// Shows Compare's rendered Markdown of [`MARKDOWN_SCRIPT`]'s file, in
/// Split, and hands back the rendered text's focus handle.
async fn show_rendered_markdown(
    app: &Entity<GitTurtle>,
    cx: &mut VisualTestContext,
) -> FocusHandle {
    cx.update(|window, cx| app.update(cx, |app, cx| app.select_file(0, window, cx)));
    settle(app, cx).await;
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.text_mode = TextMode::Markdown;
            app.prepare_editor(false, window, cx);
            cx.notify();
        })
    });
    settle(app, cx).await;
    cx.update(|_, cx| {
        app.read(cx)
            .markdown_view
            .as_ref()
            .map(|view| view.read(cx).focus_handle())
    })
    .expect("the Markdown is rendered")
}

/// Compare showing a Markdown file rendered: the rendered text, focusable
/// read-only text, is a stop with its ring on its own edge, before the
/// inspector's message and files.
#[gpui::test]
async fn compare_meets_the_rendered_markdown_once_in_order(cx: &mut TestAppContext) {
    let (_fixture, app, cx) = history_window_from(cx, MARKDOWN_SCRIPT).await;
    prepare(&app, cx);
    let rendered = show_rendered_markdown(&app, cx).await;
    let names = repository_names(&app, cx).handle(Some(rendered), "the rendered Markdown");
    let mut failures = Vec::new();
    audit(
        &app,
        cx,
        "Compare in Rendered",
        &names,
        &[
            "the rendered Markdown",
            "the commit message",
            "the file list",
        ],
        Walk::PAGE,
        &mut failures,
    );
    sweep::assert_whole(failures);
}

/// Asks for Quick source of the tracked file that `query` finds first. Its
/// read has yet to land.
fn show_quick_source(app: &Entity<GitTurtle>, cx: &mut VisualTestContext, query: &str) {
    use gitturtle_core::{HistoryCancellation, PathScope};
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            let repo = app.repository.clone().expect("a repository");
            let paths = repo
                .search_tracked_paths(&PathScope::Worktree, query, &HistoryCancellation::default())
                .expect("tracked paths");
            let entry = paths.entries.first().expect("the file").clone();
            app.show_tracked_file(paths.scope.clone(), entry, window, cx);
        })
    });
}

/// The History window with the fixture's file checked out for Quick Open to
/// find, its history being imported.
async fn quick_source_window(
    cx: &mut TestAppContext,
) -> (tempfile::TempDir, Entity<GitTurtle>, &mut VisualTestContext) {
    let (fixture, app, cx) = history_window(cx).await;
    let path = app.read_with(cx, |app, _| app.path.clone().unwrap());
    tags::tests::git(&path, &["reset", "--quiet", "--hard"]);
    settle(&app, cx).await;
    prepare(&app, cx);
    (fixture, app, cx)
}

/// Quick source's text and whether it holds focus with its ring drawn.
fn quick_source_text(app: &Entity<GitTurtle>, cx: &mut VisualTestContext) -> (FocusHandle, bool) {
    let text = app
        .read_with(cx, |app, cx| {
            app.after_editor
                .as_ref()
                .map(|editor| editor.read(cx).focus_handle(cx))
        })
        .expect("the source text is shown");
    let ringed =
        cx.update(|window, cx| text.is_focused(window) && app.read(cx).text_ring_shows(cx));
    (text, ringed)
}

/// Quick source, a tracked file opened from Quick Open: its focus lands on
/// the source text with its ring drawn. Its inspector, "Tracked source
/// information", stands where the changed files would and holds the focus
/// that lands on them while the file is read (`GitTurtle::files_focus`); it
/// holds no selection, and its controls are their own stops, so it is no
/// stop. It once tracked the changed files' handle, a stop that drew no ring
/// there, and then kept focus itself with no ring.
#[gpui::test]
async fn quick_source_meets_its_stops_both_ways_and_not_its_information(cx: &mut TestAppContext) {
    let (_fixture, app, cx) = quick_source_window(cx).await;
    show_quick_source(&app, cx, "notes");
    settle(&app, cx).await;
    views::tests::draw(cx);
    assert!(
        app.read_with(cx, |app, _| app.is_quick_source()),
        "Quick source is shown"
    );
    assert!(
        quick_source_text(&app, cx).1,
        "opening Quick source puts focus on its text, its ring drawn"
    );
    let information = app.read_with(cx, |app, _| app.source_focus.clone());
    let names = repository_names(&app, cx).handle(Some(information), "the information");
    let mut failures = Vec::new();
    let (forward, backward) = audit(
        &app,
        cx,
        "Quick source",
        &names,
        &[],
        Walk::PAGE,
        &mut failures,
    );
    for (key, walk) in [("Tab", forward), ("Shift+Tab", backward)] {
        if walk.iter().any(|name| name == "the information") {
            failures.push(format!(
                "Quick source: {key} meets its information: {walk:?}"
            ));
        }
    }
    sweep::assert_whole(failures);
}

/// Quick source opened by a pointer draws its ring on the text as well, the
/// press having ended the keyboard's ring, and the window's shortcuts still
/// work from the text: the command palette opens over it.
#[gpui::test]
async fn quick_source_rings_its_text_and_keeps_the_windows_keys(cx: &mut TestAppContext) {
    let (_fixture, app, cx) = quick_source_window(cx).await;
    // As a click on one of Quick Open's rows leaves it.
    cx.simulate_mouse_down(point(px(2.), px(2.)), MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(point(px(2.), px(2.)), MouseButton::Left, Modifiers::none());
    views::tests::draw(cx);
    show_quick_source(&app, cx, "notes");
    settle(&app, cx).await;
    views::tests::draw(cx);
    assert!(
        quick_source_text(&app, cx).1,
        "the text holds focus with its ring"
    );
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-shift-p"
    } else {
        "ctrl-shift-p"
    });
    assert!(
        cx.update(|window, cx| window.has_active_dialog(cx)),
        "the window's shortcuts work from the text"
    );
}

/// A tracked file with no text to show leaves focus on Quick source's
/// information, which then draws the ring, so focus is never left unseen.
#[gpui::test]
async fn quick_source_of_a_binary_rings_its_information(cx: &mut TestAppContext) {
    let (_fixture, app, cx) = quick_source_window(cx).await;
    let path = app.read_with(cx, |app, _| app.path.clone().unwrap());
    std::fs::write(path.join("data.bin"), [0_u8, 1, 2, 0, 255, 0]).unwrap();
    tags::tests::git(&path, &["add", "data.bin"]);
    tags::tests::git(&path, &["commit", "--quiet", "-m", "A binary"]);
    settle(&app, cx).await;
    show_quick_source(&app, cx, "data.bin");
    settle(&app, cx).await;
    views::tests::draw(cx);
    let information = app.read_with(cx, |app, _| app.source_focus.clone());
    assert!(
        cx.update(|window, _| information.is_focused(window)),
        "focus rests on the information"
    );
    assert!(
        cx.debug_bounds("quick-source-information-ring").is_some(),
        "the information draws the ring"
    );
}

/// Refresh in Quick source, and a write's landing with no key pressed, read
/// the file again, which drops the text that held focus. Focus waits on the
/// information while the read runs, so the window's keys still work, and goes
/// back to the text as it lands, its ring drawn. It once rested on the
/// dropped editor, on no control at all.
#[gpui::test]
async fn quick_source_keeps_focus_on_its_text_through_a_re_read(cx: &mut TestAppContext) {
    let (_fixture, app, cx) = quick_source_window(cx).await;
    cx.update(|_, cx| shortcuts::bind_keys(cx));
    show_quick_source(&app, cx, "notes");
    settle(&app, cx).await;
    views::tests::draw(cx);
    let source = app.read_with(cx, |app, _| app.source_focus.clone());
    assert!(quick_source_text(&app, cx).1, "opened on its text");

    // The key, which parks until the read has landed.
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-r"
    } else {
        "ctrl-r"
    });
    settle(&app, cx).await;
    views::tests::draw(cx);
    assert!(
        quick_source_text(&app, cx).1,
        "Refresh's key ends with focus on the text, ringed"
    );

    // The same action, seen while the read still runs.
    cx.update(|window, cx| window.dispatch_action(Box::new(Refresh), cx));
    assert!(
        cx.update(|window, _| source.is_focused(window)),
        "Refresh leaves focus on the information while the file is read"
    );
    settle(&app, cx).await;
    views::tests::draw(cx);
    assert!(
        quick_source_text(&app, cx).1,
        "and the landing gives it back to the text, ringed"
    );

    // A write's landing: an LFS download of the shown file reads it again
    // with no key pressed (`GitTurtle::read_after_write`).
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            let path = app.path.clone().expect("a repository");
            let file = app.files[0].clone();
            let view = (file, app.mode, app.generation);
            app.read_after_write(&path, false, Some((true, Some(view))), window, cx);
        })
    });
    assert!(
        cx.update(|window, _| source.is_focused(window)),
        "a landing write holds it on the information too"
    );
    settle(&app, cx).await;
    views::tests::draw(cx);
    assert!(
        quick_source_text(&app, cx).1,
        "and it returns to the text as the read lands"
    );
}

/// A re-read landing never moves focus the person placed: with the file open
/// and focus clicked onto its information, a read of the same file that lands
/// later leaves focus there. Only the open and a re-read that held the text's
/// own focus return it to the text.
#[gpui::test]
async fn quick_source_re_read_leaves_focus_the_person_placed(cx: &mut TestAppContext) {
    let (_fixture, app, cx) = quick_source_window(cx).await;
    show_quick_source(&app, cx, "notes");
    settle(&app, cx).await;
    views::tests::draw(cx);
    let (text, ringed) = quick_source_text(&app, cx);
    assert!(ringed, "opened on its text");
    let source = app.read_with(cx, |app, _| app.source_focus.clone());
    // As a click on the information region leaves it.
    cx.update(|window, cx| window.focus(&source, cx));
    views::tests::draw(cx);

    cx.update(|window, cx| app.update(cx, |app, cx| app.read_selected_preview(window, cx)));
    settle(&app, cx).await;
    views::tests::draw(cx);

    assert!(
        cx.update(|window, _| source.is_focused(window)),
        "the read's landing leaves focus where the person put it"
    );
    assert!(!cx.update(|window, _| text.is_focused(window)));
}

/// A re-read that fails settles what it owed the text: focus left on the
/// information stays there when a later read of the file succeeds.
#[gpui::test]
async fn quick_source_failed_re_read_owes_the_text_nothing(cx: &mut TestAppContext) {
    let (_fixture, app, cx) = quick_source_window(cx).await;
    show_quick_source(&app, cx, "notes");
    settle(&app, cx).await;
    views::tests::draw(cx);
    assert!(quick_source_text(&app, cx).1, "opened on its text");
    let source = app.read_with(cx, |app, _| app.source_focus.clone());
    let file = app.read_with(cx, |app, _| app.path.clone().unwrap().join("notes.txt"));
    let saved = std::fs::read(&file).unwrap();

    // The file goes, and Refresh's read of it fails.
    std::fs::remove_file(&file).unwrap();
    cx.update(|window, cx| window.dispatch_action(Box::new(Refresh), cx));
    settle(&app, cx).await;
    views::tests::draw(cx);
    assert!(
        app.read_with(cx, |app, _| app.error.is_some() && app.content.is_none()),
        "the read failed"
    );

    // Back again, with focus placed on the information, a read succeeds.
    std::fs::write(&file, saved).unwrap();
    cx.update(|window, cx| window.focus(&source, cx));
    views::tests::draw(cx);
    cx.update(|window, cx| app.update(cx, |app, cx| app.read_selected_preview(window, cx)));
    settle(&app, cx).await;
    views::tests::draw(cx);
    assert!(
        app.read_with(cx, |app, _| app.content.is_some()),
        "the read succeeded"
    );
    assert!(
        cx.update(|window, _| source.is_focused(window)),
        "focus stays where it was placed"
    );
}

/// Focus the person moved while the file was read stays where they put it.
#[gpui::test]
async fn quick_source_leaves_focus_moved_during_its_read(cx: &mut TestAppContext) {
    let (_fixture, app, cx) = quick_source_window(cx).await;
    show_quick_source(&app, cx, "notes");
    let moved = app.read_with(cx, |app, _| app.pull_requests_focus.clone());
    cx.update(|window, cx| window.focus(&moved, cx));
    settle(&app, cx).await;
    views::tests::draw(cx);
    let (text, _) = quick_source_text(&app, cx);
    assert!(cx.update(|window, _| moved.is_focused(window)));
    assert!(!cx.update(|window, _| text.is_focused(window)));
}

/// The rendered Markdown in Split: its keys move only the active side, so
/// its ring lies on that side's blocks below their heading, on their own
/// edge, and moves with the side; it once surrounded both sides. The
/// inspector's selected row leaves its marker to the ring only while the
/// pane shows the selected file's rendered Markdown: over an error, which
/// draws no Markdown, the row keeps it though the view keeps focus.
#[gpui::test]
async fn the_rendered_markdown_rings_its_active_side(cx: &mut TestAppContext) {
    let (_fixture, app, cx) = history_window_from(cx, MARKDOWN_SCRIPT).await;
    prepare(&app, cx);
    let rendered = show_rendered_markdown(&app, cx).await;
    let accent: Hsla = cx.read(|cx| rgb(palette(cx).accent).into());
    let rings = |cx: &mut VisualTestContext| -> Vec<Bounds<Pixels>> {
        focus_ring::painted_outlines(cx, accent)
            .into_iter()
            .filter(|ring| ring.whole && ring.width == px(1.))
            .map(|ring| ring.bounds)
            .collect()
    };
    let blocks = ["markdown-side-0-blocks", "markdown-side-1-blocks"];
    cx.update(|window, cx| window.focus(&rendered, cx));
    cx.simulate_keystrokes("down");
    views::tests::draw(cx);
    for (active, other) in [(1, 0), (0, 1)] {
        let at = format!("side {active} active");
        let side = cx
            .debug_bounds(blocks[active])
            .expect("the active side's blocks");
        let beside = cx
            .debug_bounds(blocks[other])
            .expect("the other side's blocks");
        let drawn = rings(cx);
        assert!(
            sweep::rings_edge(&drawn, side),
            "{at}: ringed on its blocks' edge, {side:?}: {drawn:?}"
        );
        assert!(
            !drawn.iter().any(|ring| ring.contains(&beside.center())),
            "{at}: the ring leaves the other side: {drawn:?}"
        );
        assert_eq!(
            selected_row_marker(cx),
            None,
            "{at}: the row's marker gives way"
        );
        // A press on the other side's blocks makes it the active side; the
        // next key shows the ring there.
        cx.simulate_click(beside.center(), Modifiers::none());
        cx.simulate_keystrokes("down");
        views::tests::draw(cx);
    }
    cx.update(|_, cx| {
        app.update(cx, |app, cx| {
            app.error = Some("The file is not available.".into());
            cx.notify();
        })
    });
    views::tests::draw(cx);
    assert!(
        cx.update(|window, _| rendered.is_focused(window)),
        "the view keeps focus"
    );
    assert_eq!(
        selected_row_marker(cx),
        Some(accent),
        "over an error, the row keeps its marker"
    );
}

/// Working Changes meets its filter, its list and the list's stops once
/// each, in order, at both sizes: the selected row's Stage where the row
/// draws one, then the group headings' Unstage all and Stage all.
#[gpui::test]
async fn working_changes_meet_their_list_once_in_order(cx: &mut TestAppContext) {
    let (fixture, app, cx) = history_window(cx).await;
    let root = fixture.path().join("narrow");
    std::fs::write(root.join("notes.txt"), "edited\n").unwrap();
    std::fs::write(root.join("untracked.txt"), "new\n").unwrap();
    std::fs::write(root.join("staged.txt"), "staged\n").unwrap();
    tags::tests::git(&root, &["add", "staged.txt"]);
    for (window, interface, row_control) in [
        (size(px(1480.), px(973.)), 13, true),
        (size(px(461.), px(490.)), 18, false),
    ] {
        prepare_at(&app, cx, window, interface);
        cx.update(|window, cx| app.update(cx, |app, cx| app.show_working(window, cx)));
        settle(&app, cx).await;
        crate::path_filter::filters_land(&app, cx);
        let (index, position) = app.read_with(cx, |app, _| {
            let status = app.shown_working_status().expect("a status");
            app.working_rows
                .iter()
                .enumerate()
                .find_map(|(position, row)| match row {
                    crate::workspace::WorkingRow::File(index, area)
                        if *area == gitturtle_core::ChangeArea::Unstaged
                            && status.entries[*index].path == std::path::Path::new("notes.txt") =>
                    {
                        Some((*index, position))
                    }
                    _ => None,
                })
                .expect("notes.txt's Unstaged row")
        });
        cx.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.choose_working_selection(
                    index,
                    gitturtle_core::ChangeArea::Unstaged,
                    false,
                    false,
                    window,
                    cx,
                )
            })
        });
        settle(&app, cx).await;
        let names = cx
            .update(|_, cx| {
                let view = app.read(cx);
                Names::default()
                    .handle(
                        Some(view.working_filter.read(cx).focus_handle(cx)),
                        "the filter",
                    )
                    .handle(Some(view.file_focus.clone()), "the working files")
            })
            .working_controls(
                app.clone(),
                vec![
                    (
                        format!("working-stage-{position}"),
                        "the selected row's Stage",
                    ),
                    ("unstage-all".to_string(), "Unstage all"),
                    ("stage-all".to_string(), "Stage all"),
                ],
            );
        let mut lists = vec!["the filter", "the working files"];
        if row_control {
            lists.push("the selected row's Stage");
        }
        lists.extend(["Unstage all", "Stage all"]);
        let mut failures = Vec::new();
        audit(
            &app,
            cx,
            "Working Changes",
            &names,
            &lists,
            Walk::PAGE.at(interface),
            &mut failures,
        );
        sweep::assert_whole(failures);
    }
}

/// A list that draws its designed empty state stays a stop and rings it:
/// the Tab order stays the same whatever the list holds, and a screen
/// reader hears the empty state where the list is (DESIGN.md, keyboard
/// focus; decided 2026-11-07). Working Changes, with nothing changed.
#[gpui::test]
async fn clean_working_changes_keep_their_empty_list_a_stop(cx: &mut TestAppContext) {
    let (fixture, app, cx) = history_window(cx).await;
    // The fixture's history is imported: its file is checked out, and
    // nothing is changed.
    tags::tests::git(
        &fixture.path().join("narrow"),
        &["reset", "--quiet", "--hard"],
    );
    prepare(&app, cx);
    cx.update(|window, cx| app.update(cx, |app, cx| app.show_working(window, cx)));
    settle(&app, cx).await;
    assert!(
        cx.debug_bounds("working-clean").is_some(),
        "the list draws its clean state"
    );
    let names = cx.update(|_, cx| {
        let app = app.read(cx);
        Names::default().handle(Some(app.file_focus.clone()), "the working files")
    });
    let mut failures = Vec::new();
    audit(
        &app,
        cx,
        "Clean Working Changes",
        &names,
        &["the working files"],
        Walk::PAGE,
        &mut failures,
    );
    sweep::assert_whole(failures);
}

#[gpui::test]
async fn file_history_meets_its_revisions_and_message_once_in_order(cx: &mut TestAppContext) {
    let (_fixture, app, cx) = history_window_from(cx, views::tests::history_script(4, 2)).await;
    prepare(&app, cx);
    views::tests::hold_quiet_reads_for_test(&app, cx);
    cx.update(|window, cx| app.update(cx, |app, cx| app.select_commit(1, window, cx)));
    settle(&app, cx).await;
    file_history::tests::open_landed(&app, cx).await;
    // File history's revisions take the file list's handle.
    let names = cx.update(|_, cx| {
        let app = app.read(cx);
        Names::default()
            .handle(app.inspector_message.focus(), "the commit message")
            .handle(Some(app.file_focus.clone()), "the revisions")
    });
    let mut failures = Vec::new();
    audit(
        &app,
        cx,
        "File history",
        &names,
        &["the commit message", "the revisions"],
        Walk::PAGE,
        &mut failures,
    );
    sweep::assert_whole(failures);
}

#[gpui::test]
async fn history_beside_the_project_pane_meets_the_projects_once(cx: &mut TestAppContext) {
    let (_fixture, app, cx) = history_window(cx).await;
    prepare(&app, cx);
    cx.update(|window, cx| {
        app.update(cx, |app, cx| {
            if !app.settings.project_pane {
                app.toggle_project_pane(window, cx);
            }
        })
    });
    settle(&app, cx).await;
    let projects = app.read_with(cx, |app, _| app.project_pane.focus.clone());
    let names = repository_names(&app, cx).handle(Some(projects), "the projects");
    let mut failures = Vec::new();
    audit(
        &app,
        cx,
        "History beside the project pane",
        &names,
        &[
            "the projects",
            "the navigator",
            "the search",
            "the commit list",
            "the commit message",
            "the file list",
        ],
        Walk::PAGE,
        &mut failures,
    );
    sweep::assert_whole(failures);
}

/// The stops `key` meets from the window's root until focus comes round,
/// read without the sweep's checks, which draw three frames a stop: the
/// other way's walk, for a surface whose rings are checked one way a test.
fn handles_met(
    app: &Entity<GitTurtle>,
    cx: &mut VisualTestContext,
    key: &str,
) -> Vec<Option<FocusHandle>> {
    let root = app.read_with(cx, |app, _| app.app_focus.clone());
    cx.update(|window, cx| window.focus(&root, cx));
    views::tests::draw(cx);
    let mut met = Vec::new();
    for _ in 0..300 {
        cx.simulate_keystrokes(key);
        views::tests::draw(cx);
        let focused = cx.update(|window, cx| window.focused(cx));
        if met.contains(&focused) {
            break;
        }
        met.push(focused);
    }
    met
}

/// Settings walked with `key` from the window's root, the sweep's checks
/// run at every stop, and walked the other way without them: some stops
/// met, and the same ones in the other order. Settings holds no selection
/// list: its theme cards are Buttons, and the Your themes rows are no stops
/// by design (`theme_editor::State::row_focus`). Walked both ways with the
/// checks in one test, it took 29 s on a loaded host, so each way is a test
/// of its own.
async fn settings_walked_with(key: &str, cx: &mut TestAppContext) {
    let (_fixture, app, cx) = history_window(cx).await;
    cx.update(|_, cx| theme_editor::init(cx));
    prepare(&app, cx);
    cx.update(|window, cx| app.update(cx, |app, cx| app.show_settings(window, cx)));
    settle(&app, cx).await;
    let mut failures = Vec::new();
    let names = Names::default();
    let checked: Vec<_> = stops(
        &app,
        cx,
        key,
        "Settings",
        Walk::PAGE,
        &names,
        &[],
        None,
        &mut failures,
    )
    .into_iter()
    .map(|(handle, _)| handle)
    .collect();
    let other = if key == "tab" { "shift-tab" } else { "tab" };
    let mut back = handles_met(&app, cx, other);
    back.reverse();
    if checked.is_empty() {
        failures.push(format!("Settings: {key} meets no stop"));
    } else if back != checked {
        failures.push(format!(
            "Settings: {other} meets {} stops, not the {} {key} meets, in the other order",
            back.len(),
            checked.len()
        ));
    }
    sweep::assert_whole(failures);
}

#[gpui::test]
async fn settings_meet_their_stops_with_tab_as_shift_tab_does(cx: &mut TestAppContext) {
    settings_walked_with("tab", cx).await;
}

#[gpui::test]
async fn settings_meet_their_stops_with_shift_tab_as_tab_does(cx: &mut TestAppContext) {
    settings_walked_with("shift-tab", cx).await;
}

/// The dialogs' selection lists, each dialog walked both ways from where it
/// puts focus as it opens: every list met by Tab and by Shift+Tab, its stops
/// in one run, the same stops both ways. The branch chooser, the remote
/// manager, Tags and the worktree manager list a Button for each row, each
/// a stop; the Stashes' two lists and the Reflog's entries are one stop
/// each, moved through with the arrow keys. Saved recovery drafts lists no
/// selection, and the series review's files and Edit local commits'
/// sequence belong to `rewrite_review` and `interactive_rebase`.
#[gpui::test]
async fn dialog_selection_lists_are_met_both_ways(cx: &mut TestAppContext) {
    let (fixture, app, cx) = history_window(cx).await;
    let root = fixture.path().join("narrow");
    let run = |args: &[&str]| tags::tests::git(&root, args);
    run(&["branch", "feature"]);
    // Checked out nowhere, so the worktree manager's Create offers it.
    run(&["branch", "spare"]);
    run(&["tag", "v1"]);
    run(&[
        "remote",
        "add",
        "origin",
        "https://example.invalid/fixture.git",
    ]);
    let linked = fixture.path().join("linked");
    run(&["worktree", "add", linked.to_str().unwrap(), "feature"]);
    std::fs::write(root.join("notes.txt"), "stashed\n").unwrap();
    run(&["stash", "push", "-m", "saved"]);
    prepare(&app, cx);
    settle(&app, cx).await;
    views::tests::hold_quiet_reads_for_test(&app, cx);
    let mut failures = Vec::new();
    for (surface, remotes, list, selector) in [
        (
            "The branch manager",
            false,
            "the branch choices",
            "branch-chooser-list",
        ),
        (
            "The remote manager",
            true,
            "the remotes",
            "remote-manager-list",
        ),
    ] {
        let list_focus = app.read_with(cx, |app, _| app.focus.clone());
        cx.update(|window, cx| window.focus(&list_focus, cx));
        let read =
            cx.update(|window, cx| app.update(cx, |app, cx| app.open_manager(remotes, window, cx)));
        read.expect("the list is read").await;
        settle(&app, cx).await;
        let names = Names::default().region(selector, list);
        audit(
            &app,
            cx,
            surface,
            &names,
            &[list],
            Walk::dialog(false),
            &mut failures,
        );
        sweep::close_dialogs(cx, surface, 13, &mut failures);
    }
    sweep::open_dialog(&app, cx, "Tags", |app, window, cx| {
        app.open_tags(window, cx)
    })
    .await;
    let names = Names::default().region("tags-list", "the tags");
    audit(
        &app,
        cx,
        "Tags",
        &names,
        &["the tags"],
        Walk::dialog(false),
        &mut failures,
    );
    sweep::close_dialogs(cx, "Tags", 13, &mut failures);
    sweep::open_dialog(&app, cx, "Worktrees", |app, window, cx| {
        app.open_worktree_manager_entity(window, cx)
    })
    .await;
    let names = Names::default().region("managed-worktree-list", "the worktrees");
    audit(
        &app,
        cx,
        "Worktrees",
        &names,
        &["the worktrees"],
        Walk::dialog(false),
        &mut failures,
    );
    // Create, for an existing branch: its branch choices, a Button each.
    for selector in ["worktree-create-tab", "worktree-existing-branch"] {
        let button = cx.debug_bounds(selector).expect(selector);
        cx.simulate_click(button.center(), Modifiers::none());
        views::tests::draw(cx);
    }
    let names = Names::default().region("worktree-branch-choices", "the branch choices");
    audit(
        &app,
        cx,
        "Worktrees' Create",
        &names,
        &["the branch choices"],
        Walk::dialog(false),
        &mut failures,
    );
    sweep::close_dialogs(cx, "Worktrees", 13, &mut failures);
    sweep::open_dialog(&app, cx, "Stashes", |app, window, cx| {
        app.open_stash_browser(window, cx)
    })
    .await;
    // The saved files are drawn once a stash is selected, from the keyboard
    // as the Stashes sweep selects it, and its inspection has landed.
    let (browser, _) = app
        .read_with(cx, recovery::open_stash_list)
        .expect("Stashes is open");
    let row = cx
        .debug_bounds("saved-stash-row-0")
        .expect("the saved stash is drawn");
    cx.simulate_click(row.center(), Modifiers::none());
    cx.simulate_keystrokes("home");
    recovery::StashBrowser::lands(&browser, cx);
    let names = Names::default()
        .region("stash-list", "the stashes")
        .region("stash-files", "the saved files");
    audit(
        &app,
        cx,
        "Stashes",
        &names,
        &["the stashes", "the saved files"],
        Walk::dialog(true),
        &mut failures,
    );
    sweep::close_dialogs(cx, "Stashes", 13, &mut failures);
    sweep::open_dialog(&app, cx, "The Reflog", |app, window, cx| {
        app.open_reflog_browser(window, cx)
    })
    .await;
    let names = Names::default().region("reflog-list", "the reflog entries");
    audit(
        &app,
        cx,
        "The Reflog",
        &names,
        &["the reflog entries"],
        Walk::dialog(true),
        &mut failures,
    );
    sweep::close_dialogs(cx, "The Reflog", 13, &mut failures);
    sweep::assert_whole(failures);
}
