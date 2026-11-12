//! Work areas: named sets of open sessions with their screen arrangement.

use gpui_kit::*;
use wai_settings::WorkArea;

use crate::workspace::Workspace;

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

impl Workspace {
    /// The area holding session `id`.
    #[allow(dead_code)] // used by the next tasks
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
}

#[cfg(test)]
mod tests {
    use super::rearranged;
    use crate::workspace::tests::{launch, open_at};
    use wai_settings::WorkArea;
    use wai_settings::{AppState, Settings};

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
}
