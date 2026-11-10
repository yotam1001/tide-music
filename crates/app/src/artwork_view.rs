//! Bounded thumbnail selection follows the scroll viewport, including a small
//! look-ahead. Models drop image references outside this range as it moves.
use std::ops::Range;

const OVERSCAN: usize = 2;
const MAX_VISIBLE_IMAGES: usize = 40;

pub fn visible_range(
    scroll_y: f32,
    viewport_height: f32,
    heights: impl IntoIterator<Item = f32>,
) -> Range<usize> {
    let top = if scroll_y.is_finite() {
        (-scroll_y).max(0.0)
    } else {
        0.0
    };
    let height = if viewport_height.is_finite() && viewport_height > 0.0 {
        viewport_height
    } else {
        720.0
    };
    let mut y = 0.0;
    let mut first = None;
    let mut last = 0;
    let mut count = 0;
    for (index, row_height) in heights.into_iter().enumerate() {
        count = index + 1;
        let next = y + row_height.max(1.0);
        if next > top && y < top + height {
            first.get_or_insert(index);
            last = index + 1;
        }
        y = next;
    }
    let Some(first) = first else { return 0..0 };
    let start = first.saturating_sub(OVERSCAN);
    start
        ..last
            .saturating_add(OVERSCAN)
            .min(count)
            .min(start + MAX_VISIBLE_IMAGES)
}

/// Card grid geometry as laid out by the interface.
pub struct Grid {
    pub columns: usize,
    /// Card height plus the gap to the next row.
    pub row_height: f32,
    /// Content above the first row inside the scrolled area.
    pub top: f32,
    /// Fixed cards placed before the model's items (Liked Songs).
    pub leading: usize,
}

/// Model indices whose cards intersect the viewport of a scrolled grid, with
/// one row of look-ahead in each direction and the same image bound as lists.
pub fn grid_visible_range(
    scroll_y: f32,
    viewport_height: f32,
    count: usize,
    grid: Grid,
) -> Range<usize> {
    let columns = grid.columns.max(1);
    let row_height = if grid.row_height.is_finite() && grid.row_height > 1.0 {
        grid.row_height
    } else {
        240.0
    };
    let rows = (count + grid.leading).div_ceil(columns);
    let heights = (0..rows).map(|row| row_height + if row == 0 { grid.top.max(0.0) } else { 0.0 });
    let visible_rows = visible_range(scroll_y, viewport_height, heights);
    if visible_rows.is_empty() {
        return 0..0;
    }
    let first_slot = visible_rows.start * columns;
    let last_slot = visible_rows.end * columns;
    let start = first_slot.saturating_sub(grid.leading).min(count);
    let end = last_slot.saturating_sub(grid.leading).min(count);
    start..end.min(start + MAX_VISIBLE_IMAGES)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_rows_map_to_card_indices_after_leading_cards() {
        let grid = |columns| Grid {
            columns,
            row_height: 200.0,
            top: 110.0,
            leading: 1,
        };
        // Rows 0..=3 visible plus overscan; slot 0 is Liked Songs.
        assert_eq!(grid_visible_range(0.0, 500.0, 100, grid(4)), 0..15);
        let scrolled = grid_visible_range(-2110.0, 400.0, 100, grid(4));
        assert!(scrolled.start >= 30 && scrolled.end <= 60, "{scrolled:?}");
        assert_eq!(grid_visible_range(0.0, 500.0, 0, grid(4)), 0..0);
        assert!(grid_visible_range(0.0, f32::MAX, 500, grid(6)).len() <= MAX_VISIBLE_IMAGES);
    }

    #[test]
    fn scrolling_reaches_images_after_the_old_fixed_limit() {
        assert_eq!(visible_range(0.0, 380.0, [76.0; 100]), 0..7);
        assert_eq!(visible_range(-1520.0, 380.0, [76.0; 100]), 18..27);
        assert_eq!(visible_range(-7220.0, 380.0, [76.0; 100]), 93..100);
    }

    #[test]
    fn mixed_rows_and_queue_headers_use_their_actual_heights() {
        assert_eq!(
            visible_range(-142.0, 66.0, [76.0, 66.0, 94.0, 66.0, 66.0, 66.0]),
            0..5
        );
        assert_eq!(visible_range(-7600.0, 380.0, [76.0; 100]), 0..0);
    }

    #[test]
    fn large_or_invalid_viewports_keep_a_fixed_image_bound() {
        let range = visible_range(-760.0, f32::MAX, [76.0; 2000]);
        assert_eq!(range.len(), MAX_VISIBLE_IMAGES);
        assert_eq!(visible_range(f32::NAN, f32::NAN, []), 0..0);
    }
}
