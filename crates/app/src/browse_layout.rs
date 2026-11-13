//! Presentation groups retain the provider's exact order and source indices.
#[derive(Debug, PartialEq)]
pub struct Group {
    pub start: usize,
    pub count: usize,
    pub title: String,
    pub layout: Layout,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Layout {
    Tiles,
    Compact,
    /// Quick picks: one sideways row of small square song cards.
    Picks,
}

pub fn home_groups(labels: &[String], columns: usize) -> Vec<Group> {
    let columns = columns.clamp(1, 8);
    let mut groups = Vec::new();
    let mut section = 0;
    while section < labels.len() {
        let mut end = section + 1;
        while end < labels.len() && labels[end] == labels[section] {
            end += 1;
        }
        // Shelves read as cover cards, as on YouTube Music; "Quick picks" is
        // a scannable song list there, so it stays compact.
        let quick_picks = labels[section].trim().eq_ignore_ascii_case("quick picks");
        // A lone item still reads as a card instead of a half-empty list row.
        let tiles = columns >= 3 && !quick_picks;
        let picks = columns >= 3 && quick_picks;
        // Card shelves (and Quick picks) are one row that scrolls sideways,
        // as in Spotify and YouTube Music. Narrow windows use paired rows.
        let row_size = if tiles || picks {
            end - section
        } else {
            columns.min(2)
        };
        let mut start = section;
        while start < end {
            let count = (end - start).min(row_size);
            groups.push(Group {
                start,
                count,
                title: if start == section {
                    labels[section].clone()
                } else {
                    String::new()
                },
                layout: if tiles {
                    Layout::Tiles
                } else if picks {
                    Layout::Picks
                } else {
                    Layout::Compact
                },
            });
            start += count;
        }
        section = end;
    }
    groups
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn responsive_sections_never_reorder_or_drop_provider_entries() {
        let labels: Vec<_> = [
            "Listen again",
            "Listen again",
            "Listen again",
            "Mixes",
            "Mixes",
            "Listen again",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        for columns in 1..=8 {
            let groups = home_groups(&labels, columns);
            let indices: Vec<_> = groups
                .iter()
                .flat_map(|g| g.start..g.start + g.count)
                .collect();
            assert_eq!(indices, (0..labels.len()).collect::<Vec<_>>());
            assert_eq!(groups.iter().filter(|g| !g.title.is_empty()).count(), 3);
            // Wide windows: one scrolling shelf per section. Narrow: paired rows.
            if columns >= 3 {
                assert_eq!(groups.len(), 3);
            } else {
                assert!(groups.iter().all(|g| g.count <= columns));
            }
            let expected = if columns >= 3 {
                Layout::Tiles
            } else {
                Layout::Compact
            };
            assert_eq!(groups[0].layout, expected);
        }
        let picks = home_groups(&vec!["Quick picks".into(); 6], 6);
        assert_eq!(picks.len(), 1);
        assert_eq!((picks[0].layout, picks[0].count), (Layout::Picks, 6));
        let narrow = home_groups(&vec!["Quick picks".into(); 6], 2);
        assert!(narrow
            .iter()
            .all(|g| g.layout == Layout::Compact && g.count <= 2));
        let lone = home_groups(&["Listen again".into()], 6);
        assert_eq!(lone[0].layout, Layout::Tiles);
        assert!(home_groups(&[], 0).is_empty());
    }
    #[test]
    fn narrow_mixed_recommendations_use_compact_pairs() {
        let groups = home_groups(&vec!["Listen again".into(); 7], 2);
        assert_eq!(groups[0].count, 2);
        assert_eq!(groups[1].layout, Layout::Compact);
        assert_eq!(groups[1].count, 2);
        assert_eq!(
            groups
                .iter()
                .flat_map(|g| g.start..g.start + g.count)
                .collect::<Vec<_>>(),
            (0..7).collect::<Vec<_>>()
        );
    }
}
