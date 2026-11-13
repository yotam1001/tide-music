//! Bounded thumbnail selection follows the scroll viewport, including a small
//! look-ahead. Models drop image references outside this range as it moves.
use std::ops::Range;

const OVERSCAN: usize = 2;
const MAX_VISIBLE_IMAGES: usize = 40;
const PER_GROUP_IMAGES: usize = 20;

pub fn grid_visible_range(
    scroll_y: f32,
    height: f32,
    count: usize,
    columns: usize,
    row_height: f32,
) -> Range<usize> {
    let columns = columns.clamp(1, 8);
    let rows = visible_range(
        scroll_y,
        height,
        (0..count.div_ceil(columns)).map(|_| row_height),
    );
    let start = (rows.start * columns).min(count);
    start
        ..(rows.end * columns)
            .min(count)
            .min(start + MAX_VISIBLE_IMAGES)
}

/// Images for the visible Home groups. A card shelf scrolls sideways, so each
/// visible group loads `PER_GROUP_IMAGES` entries from its first card on screen
/// (`first_shown(group.start)`, 0 until paged). The total stays bounded.
pub fn group_image_range(
    groups: &[crate::browse_layout::Group],
    visible: Range<usize>,
    first_shown: impl Fn(usize) -> usize,
) -> Vec<usize> {
    let mut images = Vec::new();
    for group in groups.get(visible).unwrap_or_default() {
        let first = first_shown(group.start).min(group.count.saturating_sub(1));
        let take = (group.count - first).min(PER_GROUP_IMAGES);
        let room = MAX_VISIBLE_IMAGES - images.len();
        let start = group.start + first;
        images.extend(start..start + take.min(room));
        if images.len() == MAX_VISIBLE_IMAGES {
            break;
        }
    }
    images
}

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
    #[test]
    fn grids_follow_scroll_and_stay_bounded_across_column_changes() {
        assert_eq!(grid_visible_range(-2060.0, 412.0, 200, 4, 206.0), 32..56);
        for columns in 1..=6 {
            let range = grid_visible_range(-2060.0, f32::MAX, 2000, columns, 206.0);
            assert!(range.len() <= MAX_VISIBLE_IMAGES);
        }
        // One long scrolling shelf: only its first cards load.
        let groups = crate::browse_layout::home_groups(&vec!["Mixes".into(); 200], 6);
        assert_eq!(
            group_image_range(&groups, 0..1, |_| 0),
            (0..20).collect::<Vec<_>>()
        );
        assert!(group_image_range(&groups, 0..0, |_| 0).is_empty());
        // Paged to card 50: covers follow the cards on screen.
        assert_eq!(
            group_image_range(&groups, 0..1, |_| 50),
            (50..70).collect::<Vec<_>>()
        );
        assert_eq!(group_image_range(&groups, 0..1, |_| 999), vec![199]);
        // Several shelves share the fixed bound, each starting at its first card.
        let labels: Vec<String> = (0..10)
            .flat_map(|s| vec![format!("Shelf {s}"); 20])
            .collect();
        let groups = crate::browse_layout::home_groups(&labels, 6);
        let images = group_image_range(&groups, 0..10, |_| 0);
        assert_eq!(images.len(), MAX_VISIBLE_IMAGES);
        assert_eq!(&images[20..22], &[20, 21]);
    }
}
