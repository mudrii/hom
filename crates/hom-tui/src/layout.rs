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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_layout_only_returns_first_pane_when_multiple_exist() {
        let panes = vec![1, 2, 3];
        let areas = compute_pane_areas(Rect::new(0, 0, 90, 30), &panes, &LayoutKind::Single);

        assert_eq!(areas.len(), 1);
        assert_eq!(areas[0], (1, Rect::new(0, 0, 90, 30)));
    }

    #[test]
    fn grid_layout_covers_all_panes_without_zero_sized_regions() {
        let panes = vec![1, 2, 3, 4, 5];
        let areas = compute_pane_areas(Rect::new(0, 0, 100, 40), &panes, &LayoutKind::Grid);

        assert_eq!(areas.len(), panes.len());
        assert_eq!(areas.iter().map(|(id, _)| *id).collect::<Vec<_>>(), panes);
        assert!(
            areas
                .iter()
                .all(|(_, rect)| rect.width > 0 && rect.height > 0)
        );
    }

    #[test]
    fn tabbed_layout_gives_every_pane_full_area() {
        let panes = vec![10, 20];
        let area = Rect::new(3, 4, 70, 20);
        let areas = compute_pane_areas(area, &panes, &LayoutKind::Tabbed);

        assert_eq!(areas, vec![(10, area), (20, area)]);
    }

    #[test]
    fn pane_at_position_respects_boundaries() {
        let pane_areas = vec![(1, Rect::new(0, 0, 50, 10)), (2, Rect::new(50, 0, 50, 10))];

        assert_eq!(pane_at_position(&pane_areas, 0, 0), Some(1));
        assert_eq!(pane_at_position(&pane_areas, 49, 9), Some(1));
        assert_eq!(pane_at_position(&pane_areas, 50, 0), Some(2));
        assert_eq!(pane_at_position(&pane_areas, 99, 9), Some(2));
        assert_eq!(pane_at_position(&pane_areas, 100, 9), None);
    }
}
