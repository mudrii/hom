//! Pane layout management — compute areas for each pane within the grid.

use ratatui::layout::{Constraint, Direction, Layout, Rect};

use hom_core::{LayoutKind, PaneId};

/// Compute pane areas within a given region based on layout kind and pane count.
pub fn compute_pane_areas(
    area: Rect,
    pane_ids: &[PaneId],
    layout_kind: &LayoutKind,
) -> Vec<(PaneId, Rect)> {
    if pane_ids.is_empty() {
        return Vec::new();
    }

    if pane_ids.len() == 1 {
        return vec![(pane_ids[0], area)];
    }

    let areas = match layout_kind {
        LayoutKind::Single => {
            // Only show the first pane
            vec![area]
        }
        LayoutKind::HSplit => {
            // Horizontal split — panes stacked vertically
            let constraints: Vec<Constraint> = pane_ids
                .iter()
                .map(|_| Constraint::Ratio(1, pane_ids.len() as u32))
                .collect();
            Layout::default()
                .direction(Direction::Vertical)
                .constraints(constraints)
                .split(area)
                .to_vec()
        }
        LayoutKind::VSplit => {
            // Vertical split — panes side by side
            let constraints: Vec<Constraint> = pane_ids
                .iter()
                .map(|_| Constraint::Ratio(1, pane_ids.len() as u32))
                .collect();
            Layout::default()
                .direction(Direction::Horizontal)
                .constraints(constraints)
                .split(area)
                .to_vec()
        }
        LayoutKind::Grid => {
            // Auto-grid: compute rows/cols to fit N panes
            compute_grid_areas(area, pane_ids.len())
        }
        LayoutKind::Tabbed => {
            // All panes get the full area (only focused one is rendered)
            pane_ids.iter().map(|_| area).collect()
        }
    };

    pane_ids
        .iter()
        .zip(areas)
        .map(|(&id, rect)| (id, rect))
        .collect()
}

/// Compute a grid layout for N panes.
fn compute_grid_areas(area: Rect, count: usize) -> Vec<Rect> {
    let cols = (count as f64).sqrt().ceil() as usize;
    let rows = count.div_ceil(cols);

    let row_constraints: Vec<Constraint> = (0..rows)
        .map(|_| Constraint::Ratio(1, rows as u32))
        .collect();

    let row_areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints(row_constraints)
        .split(area);

    let mut result = Vec::with_capacity(count);
    let mut remaining = count;

    for row_area in row_areas.iter() {
        let cols_in_row = remaining.min(cols);
        let col_constraints: Vec<Constraint> = (0..cols_in_row)
            .map(|_| Constraint::Ratio(1, cols_in_row as u32))
            .collect();

        let col_areas = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(col_constraints)
            .split(*row_area);

        for col_area in col_areas.iter() {
            result.push(*col_area);
        }

        remaining -= cols_in_row;
    }

    result
}

/// Find which pane is at a given screen coordinate.
pub fn pane_at_position(pane_areas: &[(PaneId, Rect)], col: u16, row: u16) -> Option<PaneId> {
    pane_areas
        .iter()
        .find(|(_, area)| {
            col >= area.x
                && col < area.x + area.width
                && row >= area.y
                && row < area.y + area.height
        })
        .map(|(id, _)| *id)
}
