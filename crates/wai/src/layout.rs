//! R89–R92: where a dragged session lands among the panes. Pure, so the rules are tested apart
//! from the views; `Workspace::drop_session` applies the result.

use crate::workspace::MAX_PANES;

/// The part of a pane a drop lands on: its left or right quarter, or the rest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zone {
    Left,
    Center,
    Right,
}

/// A pane after the drop: one that was there, by its old index, or the dropped session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    Old(usize),
    Dropped,
}

/// Whether every zone of the target acts as its center: it is `empty` (R90), or the panes are
/// full and the session is in none of them, as a sidebar row's (R92). The drop highlight asks
/// this too, so it lights only the zones that act.
pub fn only_center(panes: usize, from: Option<usize>, empty: bool) -> bool {
    empty || (panes >= MAX_PANES && from.is_none())
}

/// The `panes` panes, left to right, once a session lands on pane `target`'s `zone`; `None`
/// when nothing changes. `from`: the pane already showing that session. `empty`: the target is
/// an empty pane. `locked`: the target is an editor with unsaved tabs, which is never replaced.
pub fn place(
    panes: usize,
    from: Option<usize>,
    target: usize,
    zone: Zone,
    empty: bool,
    locked: bool,
) -> Option<Vec<Slot>> {
    let before: Vec<Slot> = (0..panes).map(Slot::Old).collect();
    let mut after = before.clone();
    let full = panes >= MAX_PANES;
    let zone = if only_center(panes, from, empty) {
        Zone::Center
    } else {
        zone
    };
    match (zone, from) {
        // R91: it moves into the empty pane, and its own pane closes.
        (Zone::Center, Some(from)) if empty => {
            after[target] = Slot::Old(from);
            after.remove(from);
        }
        (Zone::Center, Some(from)) => after.swap(from, target),
        (Zone::Center, None) if !locked => after[target] = Slot::Dropped,
        // An unsaved editor: a right drop, which at four panes is its right neighbour's center.
        (Zone::Center, None) if !full => after.insert(target + 1, Slot::Dropped),
        (Zone::Center, None) if target + 1 < panes => after[target + 1] = Slot::Dropped,
        (Zone::Center, None) => after[target - 1] = Slot::Dropped,
        (side, from) => {
            let mut at = target + usize::from(side == Zone::Right);
            let moved = match from {
                Some(from) => {
                    after.remove(from);
                    at -= usize::from(from < at);
                    Slot::Old(from)
                }
                None => Slot::Dropped,
            };
            after.insert(at, moved);
        }
    }
    (after != before).then_some(after)
}

#[cfg(test)]
mod tests {
    use super::Slot::{Dropped, Old};
    use super::Zone::{Center, Left, Right};
    use super::{only_center, place};

    #[test]
    fn center_swaps_two_panes() {
        assert_eq!(
            place(2, Some(0), 1, Center, false, false),
            Some(vec![Old(1), Old(0)])
        );
        assert_eq!(
            place(3, Some(2), 0, Center, false, false),
            Some(vec![Old(2), Old(1), Old(0)])
        );
    }

    /// The session's own pane goes: the count stays.
    #[test]
    fn edge_moves_the_pane_beside_the_target() {
        assert_eq!(
            place(3, Some(0), 2, Right, false, false),
            Some(vec![Old(1), Old(2), Old(0)])
        );
        assert_eq!(
            place(3, Some(2), 1, Left, false, false),
            Some(vec![Old(0), Old(2), Old(1)])
        );
        // Already there.
        assert_eq!(place(2, Some(0), 1, Left, false, false), None);
    }

    #[test]
    fn edge_adds_a_pane_for_a_session_in_none() {
        assert_eq!(
            place(1, None, 0, Right, false, false),
            Some(vec![Old(0), Dropped])
        );
        assert_eq!(
            place(2, None, 1, Left, false, false),
            Some(vec![Old(0), Dropped, Old(1)])
        );
    }

    /// R92: at four panes a session in none (a sidebar row) takes the pane whatever the zone…
    #[test]
    fn four_panes_turn_sidebar_edges_into_center() {
        assert_eq!(
            place(4, None, 3, Right, false, false),
            Some(vec![Old(0), Old(1), Old(2), Dropped])
        );
        assert_eq!(
            place(4, None, 0, Left, false, false),
            Some(vec![Dropped, Old(1), Old(2), Old(3)])
        );
        assert!(only_center(4, None, false));
        assert!(!only_center(3, None, false));
    }

    /// …while a pane still moves beside the target: the count stays.
    #[test]
    fn four_panes_still_move_a_pane_to_an_edge() {
        assert_eq!(
            place(4, Some(0), 2, Left, false, false),
            Some(vec![Old(1), Old(0), Old(2), Old(3)])
        );
        assert_eq!(
            place(4, Some(3), 0, Left, false, false),
            Some(vec![Old(3), Old(0), Old(1), Old(2)])
        );
        assert_eq!(
            place(4, Some(1), 3, Right, false, false),
            Some(vec![Old(0), Old(2), Old(3), Old(1)])
        );
        assert!(!only_center(4, Some(0), false));
    }

    /// R90: an empty pane is filled wherever the drop lands.
    #[test]
    fn edge_on_an_empty_pane_fills_it() {
        for zone in [Left, Right] {
            assert_eq!(
                place(2, None, 1, zone, true, false),
                Some(vec![Old(0), Dropped]),
                "{zone:?}"
            );
        }
        assert_eq!(
            place(3, Some(0), 2, Left, true, false),
            Some(vec![Old(1), Old(0)])
        );
        assert!(only_center(2, Some(0), true));
    }

    /// R91: a pane dropped on an empty pane's center moves there; its own pane closes.
    #[test]
    fn pane_into_an_empty_pane_moves_and_closes_its_own() {
        assert_eq!(
            place(2, Some(0), 1, Center, true, false),
            Some(vec![Old(0)])
        );
        assert_eq!(
            place(3, Some(2), 0, Center, true, false),
            Some(vec![Old(2), Old(1)])
        );
        assert_eq!(
            place(4, Some(0), 3, Center, true, false),
            Some(vec![Old(1), Old(2), Old(0)])
        );
    }

    #[test]
    fn own_pane_is_a_no_op() {
        for zone in [Left, Center, Right] {
            assert_eq!(place(1, Some(0), 0, zone, false, false), None, "{zone:?}");
            assert_eq!(place(3, Some(1), 1, zone, false, false), None, "{zone:?}");
        }
        assert_eq!(place(4, Some(1), 1, Right, false, false), None);
    }

    /// A History row (or an open session in no pane) dropped on the center takes the pane; the
    /// session it showed is no slot now, and the workspace keeps it in the open list.
    #[test]
    fn center_replaces_for_a_session_in_none() {
        assert_eq!(
            place(2, None, 1, Center, false, false),
            Some(vec![Old(0), Dropped])
        );
        assert_eq!(place(1, None, 0, Center, false, false), Some(vec![Dropped]));
    }

    /// An editor with unsaved tabs is never replaced: the session goes to its right, or at four
    /// panes into its right neighbour (its left one when it is the last). A swap keeps it.
    #[test]
    fn unsaved_editor_is_never_replaced() {
        assert_eq!(
            place(1, None, 0, Center, false, true),
            Some(vec![Old(0), Dropped])
        );
        assert_eq!(
            place(4, None, 1, Center, false, true),
            Some(vec![Old(0), Old(1), Dropped, Old(3)])
        );
        assert_eq!(
            place(4, None, 3, Left, false, true),
            Some(vec![Old(0), Old(1), Dropped, Old(3)])
        );
        assert_eq!(
            place(2, Some(0), 1, Center, false, true),
            Some(vec![Old(1), Old(0)])
        );
        // Its edges still add a pane.
        assert_eq!(
            place(1, None, 0, Left, false, true),
            Some(vec![Dropped, Old(0)])
        );
    }
}
