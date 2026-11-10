//! R89: where a dragged session lands among the panes. Pure, so the rules are tested apart from
//! the views; `Workspace::drop_session` applies the result.

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

/// The `panes` panes, left to right, once a session lands on pane `target`'s `zone`; `None`
/// when nothing changes. `from`: the pane already showing that session. `locked`: the target is
/// an editor with unsaved tabs, which is never replaced.
pub fn place(
    panes: usize,
    from: Option<usize>,
    target: usize,
    zone: Zone,
    locked: bool,
) -> Option<Vec<Slot>> {
    let before: Vec<Slot> = (0..panes).map(Slot::Old).collect();
    let mut after = before.clone();
    let full = panes >= MAX_PANES;
    let zone = if full { Zone::Center } else { zone };
    match (zone, from) {
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
    use super::place;

    #[test]
    fn center_swaps_two_panes() {
        assert_eq!(
            place(2, Some(0), 1, Center, false),
            Some(vec![Old(1), Old(0)])
        );
        assert_eq!(
            place(3, Some(2), 0, Center, false),
            Some(vec![Old(2), Old(1), Old(0)])
        );
    }

    /// The session's own pane goes: the count stays.
    #[test]
    fn edge_moves_the_pane_beside_the_target() {
        assert_eq!(
            place(3, Some(0), 2, Right, false),
            Some(vec![Old(1), Old(2), Old(0)])
        );
        assert_eq!(
            place(3, Some(2), 1, Left, false),
            Some(vec![Old(0), Old(2), Old(1)])
        );
        // Already there.
        assert_eq!(place(2, Some(0), 1, Left, false), None);
    }

    #[test]
    fn edge_adds_a_pane_for_a_session_in_none() {
        assert_eq!(place(1, None, 0, Right, false), Some(vec![Old(0), Dropped]));
        assert_eq!(
            place(2, None, 1, Left, false),
            Some(vec![Old(0), Dropped, Old(1)])
        );
    }

    /// At four panes an edge is the center: a swap, or a replacement.
    #[test]
    fn four_panes_turn_edges_into_center() {
        assert_eq!(
            place(4, Some(0), 2, Left, false),
            Some(vec![Old(2), Old(1), Old(0), Old(3)])
        );
        assert_eq!(
            place(4, None, 3, Right, false),
            Some(vec![Old(0), Old(1), Old(2), Dropped])
        );
    }

    #[test]
    fn own_pane_is_a_no_op() {
        for zone in [Left, Center, Right] {
            assert_eq!(place(1, Some(0), 0, zone, false), None, "{zone:?}");
            assert_eq!(place(3, Some(1), 1, zone, false), None, "{zone:?}");
        }
        assert_eq!(place(4, Some(1), 1, Right, false), None);
    }

    /// A History row (or an open session in no pane) dropped on the center takes the pane; the
    /// session it showed is no slot now, and the workspace keeps it in the open list.
    #[test]
    fn center_replaces_for_a_session_in_none() {
        assert_eq!(
            place(2, None, 1, Center, false),
            Some(vec![Old(0), Dropped])
        );
        assert_eq!(place(1, None, 0, Center, false), Some(vec![Dropped]));
    }

    /// An editor with unsaved tabs is never replaced: the session goes to its right, or at four
    /// panes into its right neighbour (its left one when it is the last). A swap keeps it.
    #[test]
    fn unsaved_editor_is_never_replaced() {
        assert_eq!(place(1, None, 0, Center, true), Some(vec![Old(0), Dropped]));
        assert_eq!(
            place(4, None, 1, Center, true),
            Some(vec![Old(0), Old(1), Dropped, Old(3)])
        );
        assert_eq!(
            place(4, None, 3, Left, true),
            Some(vec![Old(0), Old(1), Dropped, Old(3)])
        );
        assert_eq!(
            place(2, Some(0), 1, Center, true),
            Some(vec![Old(1), Old(0)])
        );
        // Its edges still add a pane.
        assert_eq!(place(1, None, 0, Left, true), Some(vec![Dropped, Old(0)]));
    }
}
