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

#[cfg(test)]
mod tests {
    use super::*;

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
