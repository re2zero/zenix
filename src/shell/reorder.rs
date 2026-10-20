//! Reordering by pointer: the payload a drag carries and the gap it lands in.
//!
//! Two lists reorder this way — the tabs along the top and the workspaces down
//! the sidebar — and both end in the daemon (`tab.move`, `workspace.move`). The
//! daemon's own rule for `insert_index` is the one thing worth naming: the index
//! counts the moving row **still in place**, so the gap before a row at index
//! `n` is `n`, and the gap after the last row is the row count. Moving a row
//! into either of the two gaps around it changes nothing.
//!
//! What a drop lands on decides the gap: a row inserts before itself, and the
//! list's own container appends, which is how a row reaches the end without a
//! separate placeholder.

/// A tab being carried along the strip.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TabDrag {
    pub(crate) tab_id: String,
    /// Its index in the strip when the drag began.
    pub(crate) from: usize,
}

/// A workspace row being carried down the sidebar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WorkspaceDrag {
    pub(crate) workspace_id: String,
    /// Its index in the sidebar when the drag began.
    pub(crate) from: usize,
}

/// The gap before the row at `onto`, or `None` when dropping there would leave
/// the order as it is.
pub(crate) fn gap_before(from: usize, onto: usize) -> Option<usize> {
    (onto != from && onto != from + 1).then_some(onto)
}

/// The gap after the last row, or `None` when the row is already there.
pub(crate) fn gap_at_end(from: usize, count: usize) -> Option<usize> {
    (count > 1 && from + 1 != count).then_some(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Dropping a row on the row it already sits before, or on the one after
    /// it, is not a move: both gaps are where it already is.
    #[test]
    fn the_two_gaps_around_a_row_are_not_moves() {
        assert_eq!(gap_before(2, 2), None);
        assert_eq!(gap_before(2, 3), None);
        assert_eq!(gap_before(2, 0), Some(0));
        assert_eq!(gap_before(0, 2), Some(2));
        assert_eq!(gap_before(3, 1), Some(1));
    }

    #[test]
    fn the_end_gap_is_the_row_count_and_a_row_already_there_stays() {
        // [a, b, c] and a drag of a (0) to the end: insert 3 -> [b, c, a].
        assert_eq!(gap_at_end(0, 3), Some(3));
        // c is already last.
        assert_eq!(gap_at_end(2, 3), None);
        // One row has nowhere to go.
        assert_eq!(gap_at_end(0, 1), None);
        assert_eq!(gap_at_end(0, 0), None);
    }
}
