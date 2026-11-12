//! Work areas: named sets of open sessions with their screen arrangement.

use gpui_kit::component::WindowExt as _;
use gpui_kit::component::notification::Notification;
use gpui_kit::*;
use wai_settings::WorkArea;

use crate::chat::ChatSession;
use crate::workspace::{MAX_PANES, Pane, Workspace};

/// `area` as the panes now show it: its sessions on screen (`shown`, the panes' session ids
/// left to right) take, in pane order, the slots they held; the others keep theirs. The focus
/// moves to `focused` (the focused pane's session id) when it is one of them.
pub(crate) fn rearranged(area: &WorkArea, shown: &[&str], focused: Option<&str>) -> WorkArea {
    let slots = (0..area.panes.len()).filter(|&i| shown.contains(&area.panes[i].as_str()));
    let order = shown
        .iter()
        .filter(|id| area.panes.iter().any(|p| p == *id));
    let mut panes = area.panes.clone();
    for (slot, id) in slots.zip(order) {
        panes[slot] = id.to_string();
    }
    let focused = focused
        .and_then(|f| panes.iter().position(|p| p == f))
        .unwrap_or(area.focused);
    WorkArea {
        panes,
        focused,
        ..area.clone()
    }
}

pub(crate) const AREA_FULL: &str = "A work area holds up to 4 sessions";

impl Workspace {
    /// A session dropped on area `ix`'s row joins it (D2, D3): leaving its group or other area, as a
    /// new pane at the right while `ix` is shown and has room. A full area refuses with a warning
    /// notification `AREA_FULL` and returns false.
    pub(crate) fn add_to_area(
        &mut self,
        ix: usize,
        session: &Entity<ChatSession>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let id = session.read(cx).id.clone();
        if self.area_of(&id) == Some(ix) {
            return true;
        }
        if self.work_areas[ix].panes.len() >= MAX_PANES {
            window.push_notification(Notification::warning(AREA_FULL), cx);
            return false;
        }
        if self.shown_area == Some(ix) {
            self.work_areas[ix] = self.area_now(ix, cx);
        }
        self.join(ix, &id);
        let in_pane = self.panes.iter().any(|p| p.session() == Some(session));
        if self.shown_area == Some(ix) && !in_pane && self.panes.len() < MAX_PANES {
            self.panes.push(Pane::Session(session.clone()));
        }
        self.save_state(cx);
        cx.notify();
        true
    }

    /// Session `id` leaves its group and area (`ungroup`), then joins area `ix` at the end.
    fn join(&mut self, ix: usize, id: &str) {
        self.ungroup(id);
        self.work_areas[ix].panes.push(id.to_string());
    }

    /// D14: while an area is shown, `session`, in no area, which the user just put in a pane,
    /// joins it when it holds fewer than `MAX_PANES`.
    pub(crate) fn adopt(&mut self, session: &Entity<ChatSession>, cx: &App) {
        let id = session.read(cx).id.clone();
        if let Some(ix) = self.shown_area
            && self.area_of(&id).is_none()
            && self.work_areas[ix].panes.len() < MAX_PANES
        {
            self.work_areas[ix] = self.area_now(ix, cx);
            self.join(ix, &id);
        }
    }

    /// The area holding session `id`.
    pub(crate) fn area_of(&self, id: &str) -> Option<usize> {
        self.work_areas
            .iter()
            .position(|a| a.panes.iter().any(|p| p == id))
    }

    /// Area `ix` as it stands: `rearranged` from the panes while it is shown, else as stored.
    pub(crate) fn area_now(&self, ix: usize, cx: &App) -> WorkArea {
        let area = &self.work_areas[ix];
        if self.shown_area != Some(ix) {
            return area.clone();
        }
        let ids: Vec<String> = self
            .panes
            .iter()
            .filter_map(|p| p.session())
            .map(|s| s.read(cx).id.clone())
            .collect();
        let shown: Vec<&str> = ids.iter().map(String::as_str).collect();
        let focused = self
            .panes
            .get(self.focused_pane)
            .and_then(|p| p.session())
            .map(|s| s.read(cx).id.as_str());
        rearranged(area, &shown, focused)
    }
    /// Shows area `ix` (`lay_out_area`), focuses that pane and follows its session.
    pub(crate) fn show_area(
        &mut self,
        ix: usize,
        focus: Option<&Entity<ChatSession>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pane = self.lay_out_area(ix, focus, cx);
        self.focus_pane(pane, window, cx);
        self.follow_session(window, cx);
        self.save_state(cx);
    }

    /// Shows `session`, in no area, alone in one pane (beside the editor pane, see `replace_panes`);
    /// the shown area, if any, hides as its panes left it. Focuses it and follows it.
    pub(crate) fn show_alone(
        &mut self,
        session: Entity<ChatSession>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.hide_area(cx);
        self.replace_panes(vec![Pane::Session(session)], cx);
        self.focus_pane(0, window, cx);
        self.follow_session(window, cx);
        self.save_state(cx);
    }

    /// Hides the shown area, storing it as `area_now` gives it.
    fn hide_area(&mut self, cx: &App) {
        if let Some(ix) = self.shown_area.take() {
            self.work_areas[ix] = self.area_now(ix, cx);
        }
    }

    /// `hide_area`, then area `ix`'s open sessions (`take(MAX_PANES)`; one empty pane if none) go
    /// on screen through `replace_panes`, and it is the shown area. Returns the pane to focus:
    /// `focus`'s, else the one showing the area's `panes[focused]`, else 0.
    pub(crate) fn lay_out_area(
        &mut self,
        ix: usize,
        focus: Option<&Entity<ChatSession>>,
        cx: &mut Context<Self>,
    ) -> usize {
        self.hide_area(cx);
        let area = self.work_areas[ix].clone();
        let open: Vec<Entity<ChatSession>> = area
            .panes
            .iter()
            .filter_map(|id| self.sessions.iter().find(|s| s.read(cx).id == *id))
            .take(MAX_PANES)
            .cloned()
            .collect();
        let main = area
            .panes
            .get(area.focused)
            .and_then(|id| open.iter().find(|s| s.read(cx).id == *id))
            .cloned();
        let mut panes: Vec<Pane> = open.into_iter().map(Pane::Session).collect();
        if panes.is_empty() {
            panes.push(Pane::Empty(None));
        }
        self.replace_panes(panes, cx);
        self.shown_area = Some(ix);
        let at = |s: &Entity<ChatSession>| self.panes.iter().position(|p| p.session() == Some(s));
        focus
            .and_then(at)
            .or(main.as_ref().and_then(at))
            .unwrap_or(0)
    }

    /// Puts `panes` on screen with the editor pane, if there is one, kept after them (at
    /// `MAX_PANES` it takes the last pane). A session leaving the screen that is in no area closes
    /// its subagent drawer (`close_hidden_drawers`); an area's sessions keep theirs.
    fn replace_panes(&mut self, mut panes: Vec<Pane>, cx: &mut Context<Self>) {
        let editor = self.panes.iter().position(|p| matches!(p, Pane::Editor(_)));
        if let Some(editor) = editor {
            panes.truncate(MAX_PANES - 1);
            panes.push(self.panes.swap_remove(editor));
        }
        self.panes = panes;
        self.focused_pane = 0;
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::rearranged;
    use crate::chat::{ChatSession, TurnState};
    use crate::workspace::tests::{fake_settings, launch, open, open_at, start_now, wait_until};
    use crate::workspace::{Pane, Workspace};
    use gpui_kit::component::WindowExt as _;
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{ElementId, Entity, TestAppContext, VisualTestContext};
    use wai_settings::SessionGroup;
    use wai_settings::WorkArea;
    use wai_settings::{AppState, OpenSession, Settings};

    fn area(ids: &[&str], focused: usize) -> WorkArea {
        WorkArea {
            name: "A".into(),
            panes: ids.iter().map(|s| s.to_string()).collect(),
            focused,
            ..Default::default()
        }
    }

    #[::core::prelude::v1::test]
    fn rearranged_follows_the_panes() {
        let a = area(&["a", "b", "c", "d"], 0);
        let r = rearranged(&a, &["c", "x", "a"], Some("a"));
        assert_eq!(r.panes, ["c", "b", "a", "d"]);
        assert_eq!(r.focused, 2);
        let r = rearranged(&a, &["d", "a"], Some("x"));
        assert_eq!(r.panes, ["d", "b", "c", "a"]);
        assert_eq!(r.focused, 0);
        assert_eq!(rearranged(&a, &[], None), a);
    }

    #[gpui_kit::test]
    fn work_areas_are_saved_and_read_back(cx: &mut gpui_kit::TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        let want = vec![area(&["s1", "s2"], 1)];
        let (w, cx) = open_at(cx, Settings::default(), Some(path.clone()));
        cx.update(|_, cx| {
            w.update(cx, |w, cx| {
                w.work_areas = want.clone();
                cx.notify();
                w.save_state(cx);
            })
        });
        assert_eq!(AppState::load(&path).work_areas, want);
        let (w, cx) = open_at(cx, Settings::default(), Some(path));
        launch(&w, cx);
        assert_eq!(w.read_with(cx, |w, _| w.work_areas.clone()), want);
    }

    fn set_areas(
        workspace: &Entity<Workspace>,
        sessions: &[Entity<ChatSession>],
        members: &[&[usize]],
        cx: &mut VisualTestContext,
    ) {
        let areas = members
            .iter()
            .enumerate()
            .map(|(i, m)| WorkArea {
                name: format!("Area {i}"),
                panes: m
                    .iter()
                    .map(|&s| sessions[s].read_with(cx, |s, _| s.id.clone()))
                    .collect(),
                ..Default::default()
            })
            .collect();
        workspace.update(cx, |w, _| w.work_areas = areas);
    }

    fn with_areas<'a>(
        n: usize,
        members: &[&[usize]],
        cx: &'a mut TestAppContext,
    ) -> (
        Entity<Workspace>,
        &'a mut VisualTestContext,
        Vec<Entity<ChatSession>>,
    ) {
        let (workspace, cx) = crate::sidebar::tests::with_sessions(n, cx);
        let sessions = workspace.read_with(cx, |w, _| w.sessions.clone());
        set_areas(&workspace, &sessions, members, cx);
        (workspace, cx, sessions)
    }

    fn on_screen(
        workspace: &Entity<Workspace>,
        cx: &mut VisualTestContext,
    ) -> Vec<Option<Entity<ChatSession>>> {
        workspace.read_with(cx, |w, _| {
            w.panes.iter().map(|p| p.session().cloned()).collect()
        })
    }

    fn some(sessions: &[&Entity<ChatSession>]) -> Vec<Option<Entity<ChatSession>>> {
        sessions.iter().map(|s| Some((*s).clone())).collect()
    }

    fn show_area(workspace: &Entity<Workspace>, ix: usize, cx: &mut VisualTestContext) {
        cx.update(|window, cx| workspace.update(cx, |w, cx| w.show_area(ix, None, window, cx)));
        cx.run_until_parked();
    }

    fn show(
        workspace: &Entity<Workspace>,
        s: &Entity<ChatSession>,
        side: bool,
        cx: &mut VisualTestContext,
    ) {
        let s = s.clone();
        cx.update(|window, cx| workspace.update(cx, |w, cx| w.show(s, side, window, cx)));
        cx.run_until_parked();
    }

    fn add(
        workspace: &Entity<Workspace>,
        ix: usize,
        s: &Entity<ChatSession>,
        cx: &mut VisualTestContext,
    ) -> bool {
        let s = s.clone();
        let added =
            cx.update(|window, cx| workspace.update(cx, |w, cx| w.add_to_area(ix, &s, window, cx)));
        cx.run_until_parked();
        added
    }

    fn ids(s: &[&Entity<ChatSession>], cx: &mut VisualTestContext) -> Vec<String> {
        s.iter()
            .map(|s| s.read_with(cx, |s, _| s.id.clone()))
            .collect()
    }

    fn area_panes(w: &Entity<Workspace>, ix: usize, cx: &mut VisualTestContext) -> Vec<String> {
        w.read_with(cx, |w, cx| w.area_now(ix, cx).panes)
    }

    #[gpui_kit::test]
    fn a_dropped_session_joins_at_the_right(cx: &mut TestAppContext) {
        let (w, cx, s) = with_areas(3, &[&[0], &[]], cx);
        show_area(&w, 0, cx);
        assert!(add(&w, 0, &s[1], cx));
        assert_eq!(on_screen(&w, cx), some(&[&s[0], &s[1]]));
        assert_eq!(area_panes(&w, 0, cx), ids(&[&s[0], &s[1]], cx));
        assert!(add(&w, 1, &s[2], cx));
        assert_eq!(area_panes(&w, 1, cx), ids(&[&s[2]], cx));
        assert_eq!(on_screen(&w, cx), some(&[&s[0], &s[1]]));
    }

    #[gpui_kit::test]
    fn a_full_area_refuses(cx: &mut TestAppContext) {
        let (w, cx, s) = with_areas(5, &[&[0, 1, 2, 3]], cx);
        let before = area_panes(&w, 0, cx);
        assert!(!add(&w, 0, &s[4], cx));
        assert_eq!(area_panes(&w, 0, cx), before);
        assert!(cx.update(|window, cx| !window.notifications(cx).is_empty()));
    }

    #[gpui_kit::test]
    fn a_session_is_in_one_place_at_most(cx: &mut TestAppContext) {
        let (w, cx, s) = with_areas(2, &[&[0], &[]], cx);
        let id1 = ids(&[&s[1]], cx);
        w.update(cx, |w, _| {
            w.groups = vec![SessionGroup {
                name: "G".into(),
                collapsed: false,
                sessions: id1.clone(),
                color: 0,
            }]
        });
        assert!(add(&w, 1, &s[0], cx));
        assert!(area_panes(&w, 0, cx).is_empty());
        assert_eq!(area_panes(&w, 1, cx), ids(&[&s[0]], cx));
        assert!(add(&w, 0, &s[1], cx));
        assert_eq!(area_panes(&w, 0, cx), id1);
        assert!(w.read_with(cx, |w, _| w.groups[0].sessions.is_empty()));
    }

    #[gpui_kit::test]
    fn closing_a_pane_takes_it_out_of_its_area(cx: &mut TestAppContext) {
        let (w, cx, s) = with_areas(3, &[&[0, 1], &[2]], cx);
        show_area(&w, 0, cx);
        w.update(cx, |w, _| w.focused_pane = 1);
        cx.simulate_keystrokes("secondary-w");
        cx.run_until_parked();
        assert_eq!(on_screen(&w, cx), some(&[&s[0]]));
        assert_eq!(area_panes(&w, 0, cx), ids(&[&s[0]], cx));
        assert!(w.read_with(cx, |w, _| w.sessions.contains(&s[1])));
        let s2 = s[2].clone();
        cx.update(|window, cx| w.update(cx, |w, cx| w.end_session(&s2, window, cx)));
        cx.run_until_parked();
        assert!(area_panes(&w, 1, cx).is_empty());
        assert_eq!(area_panes(&w, 0, cx), ids(&[&s[0]], cx));
    }

    #[gpui_kit::test]
    fn a_draft_does_not_push_a_session_out(cx: &mut TestAppContext) {
        let (w, cx, s) = with_areas(3, &[&[0, 1], &[2]], cx);
        show_area(&w, 0, cx);
        w.update(cx, |w, _| w.focused_pane = 1);
        cx.simulate_keystrokes("secondary-n");
        cx.run_until_parked();
        assert!(w.read_with(cx, |w, _| w.panes[1].draft().is_some()));
        assert_eq!(area_panes(&w, 0, cx), ids(&[&s[0], &s[1]], cx));
        show_area(&w, 1, cx);
        show_area(&w, 0, cx);
        assert_eq!(on_screen(&w, cx), some(&[&s[0], &s[1]]));
    }

    #[gpui_kit::test]
    fn an_area_keeps_its_panes_as_moved(cx: &mut TestAppContext) {
        let (w, cx, s) = with_areas(3, &[&[0, 1], &[2]], cx);
        show_area(&w, 0, cx);
        let s1 = s[1].clone();
        let from = cx.update(|window, cx| {
            window.render_frame(cx);
            let header = window.within(ElementId::View(s1.entity_id()));
            header.find("session-title").bounds().center()
        });
        crate::workspace::tests::drag_to_pane(from, 0, 0.5, cx);
        assert_eq!(on_screen(&w, cx), some(&[&s[1], &s[0]]));
        show_area(&w, 1, cx);
        show_area(&w, 0, cx);
        assert_eq!(on_screen(&w, cx), some(&[&s[1], &s[0]]));
    }

    #[gpui_kit::test]
    fn open_to_the_side_joins_the_shown_area(cx: &mut TestAppContext) {
        let (w, cx, s) = with_areas(3, &[&[0]], cx);
        show_area(&w, 0, cx);
        show(&w, &s[1], true, cx);
        assert_eq!(on_screen(&w, cx), some(&[&s[0], &s[1]]));
        assert_eq!(area_panes(&w, 0, cx), ids(&[&s[0], &s[1]], cx));
        show(&w, &s[2], false, cx);
        assert_eq!(on_screen(&w, cx), some(&[&s[2]]));
        assert_eq!(
            w.read_with(cx, |w, _| w.work_areas[0].panes.clone()),
            ids(&[&s[0], &s[1]], cx)
        );
    }

    #[gpui_kit::test]
    fn showing_an_area_puts_back_its_panes(cx: &mut TestAppContext) {
        let (w, cx, s) = with_areas(4, &[&[0, 1, 2], &[3]], cx);
        show_area(&w, 0, cx);
        assert_eq!(on_screen(&w, cx), some(&[&s[0], &s[1], &s[2]]));
        assert_eq!(w.read_with(cx, |w, _| w.shown_area), Some(0));
        w.update(cx, |w, _| w.focused_pane = 2);
        show_area(&w, 1, cx);
        assert_eq!(on_screen(&w, cx), some(&[&s[3]]));
        show_area(&w, 0, cx);
        assert_eq!(on_screen(&w, cx), some(&[&s[0], &s[1], &s[2]]));
        w.read_with(cx, |w, cx| {
            assert_eq!(w.focused_pane, 2);
            assert_eq!(w.sessions.len(), 4);
            for s in &w.sessions {
                assert!(!matches!(s.read(cx).model.state, TurnState::Error(_)));
            }
        });
    }

    #[gpui_kit::test]
    fn an_outside_session_shows_alone(cx: &mut TestAppContext) {
        let (w, cx, s) = with_areas(4, &[&[0, 1]], cx);
        let ids: Vec<String> = [0, 1]
            .iter()
            .map(|&i| s[i].read_with(cx, |s, _| s.id.clone()))
            .collect();
        show_area(&w, 0, cx);
        show(&w, &s[2], false, cx);
        assert_eq!(on_screen(&w, cx), some(&[&s[2]]));
        w.read_with(cx, |w, _| {
            assert_eq!(w.shown_area, None);
            assert_eq!(w.work_areas[0].panes, ids);
        });
        show(&w, &s[3], true, cx);
        assert_eq!(on_screen(&w, cx), some(&[&s[2], &s[3]]));
        w.read_with(cx, |w, _| assert_eq!(w.work_areas[0].panes, ids));
        show(&w, &s[2], false, cx);
        assert_eq!(on_screen(&w, cx), some(&[&s[2], &s[3]]));
        assert_eq!(w.read_with(cx, |w, _| w.focused_pane), 0);
        show_area(&w, 0, cx);
        show(&w, &s[2], false, cx);
        assert_eq!(on_screen(&w, cx), some(&[&s[2]]));
    }

    #[gpui_kit::test]
    fn clicking_a_session_of_another_area_shows_that_area(cx: &mut TestAppContext) {
        let (w, cx, s) = with_areas(4, &[&[0, 1], &[2, 3]], cx);
        show_area(&w, 0, cx);
        show(&w, &s[3], false, cx);
        assert_eq!(on_screen(&w, cx), some(&[&s[2], &s[3]]));
        w.read_with(cx, |w, _| {
            assert_eq!(w.focused_pane, 1);
            assert_eq!(w.shown_area, Some(1));
        });
    }

    #[gpui_kit::test]
    fn switching_keeps_the_editor_pane(cx: &mut TestAppContext) {
        let (w, cx, s) = with_areas(6, &[&[0], &[1, 2, 3, 4]], cx);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        std::fs::write(&path, "a").unwrap();
        show_area(&w, 0, cx);
        cx.update(|window, cx| w.update(cx, |w, cx| w.open_file(path, None, window, cx)));
        cx.run_until_parked();
        let editor = w.read_with(cx, |w, _| match &w.panes[..] {
            [Pane::Session(_), Pane::Editor(e)] => e.clone(),
            _ => panic!("a session and the editor"),
        });
        let tab = editor.read_with(cx, |e, _| e.tabs[0].clone());
        tab.update(cx, |t, _| t.dirty = true);
        let kept = |cx: &mut VisualTestContext| {
            w.read_with(cx, |w, _| {
                assert!(matches!(w.panes.last(), Some(Pane::Editor(e)) if *e == editor));
            });
            assert!(tab.read_with(cx, |t, _| t.dirty));
        };
        show_area(&w, 1, cx);
        let mut want = some(&[&s[1], &s[2], &s[3]]);
        want.push(None);
        assert_eq!(on_screen(&w, cx), want);
        kept(cx);
        w.read_with(cx, |w, _| assert_eq!(w.work_areas[1].panes.len(), 4));
        show(&w, &s[5], false, cx);
        let mut want = some(&[&s[5]]);
        want.push(None);
        assert_eq!(on_screen(&w, cx), want);
        kept(cx);
        show_area(&w, 0, cx);
        let mut want = some(&[&s[0]]);
        want.push(None);
        assert_eq!(on_screen(&w, cx), want);
        kept(cx);
    }

    #[gpui_kit::test]
    fn hidden_area_keeps_a_waiting_permission(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (w, cx) = open(cx, fake_settings("--ask-permission"));
        let s0 = start_now(&w, cx);
        let s1 = start_now(&w, cx);
        let s = vec![s0.clone(), s1];
        set_areas(&w, &s, &[&[0], &[1]], cx);
        show_area(&w, 0, cx);
        cx.simulate_input("edit");
        cx.simulate_keystrokes("enter");
        show_area(&w, 1, cx);
        wait_until(cx, "the permission card", |cx| {
            s0.read_with(cx, |s, _| s.model.state == TurnState::NeedsYou)
        });
        assert!(s0.read_with(cx, |s, _| !s.model.waiting.is_empty()));
        assert!(!on_screen(&w, cx).contains(&Some(s0.clone())));
        show_area(&w, 0, cx);
        cx.update(|window, cx| {
            window.render_frame(cx);
            window.within("waiting").click("perm-allow_once", cx);
        });
        wait_until(cx, "the turn's end", |cx| {
            s0.read_with(cx, |s, _| s.model.state == TurnState::Idle && s.model.done)
        });
    }

    #[gpui_kit::test]
    fn a_shown_area_comes_back_on_relaunch(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        let open_session = |id: &str, pane| OpenSession {
            agent: "fake".into(),
            session_id: id.into(),
            cwd: std::env::temp_dir(),
            pane,
            name: None,
        };
        let panes: Vec<String> = ["a", "gone", "b"].map(String::from).into();
        let state = AppState {
            open_sessions: vec![open_session("a", 0), open_session("b", 1)],
            work_areas: vec![WorkArea {
                name: "A".into(),
                panes: panes.clone(),
                focused: 2,
                ..Default::default()
            }],
            shown_area: Some(0),
            ..AppState::default()
        };
        state.save(&path).unwrap();
        let (w, cx) = open_at(cx, fake_settings(""), Some(path));
        let project = w.read_with(cx, |w, _| w.project.clone());
        launch(&w, cx);
        w.read_with(cx, |w, cx| {
            let ids: Vec<_> = w
                .panes
                .iter()
                .map(|p| p.session().map(|s| s.read(cx).id.clone()))
                .collect();
            assert_eq!(ids, [Some("a".to_string()), Some("b".to_string())]);
            assert_eq!(w.focused_pane, 1);
            assert_eq!(w.shown_area, Some(0));
            assert_eq!(w.work_areas[0].panes, panes);
            assert_eq!(w.project, project);
        });
    }
}
