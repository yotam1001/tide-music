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
    Spotlight,
    Tiles,
    Compact,
}

pub fn home_groups(labels: &[String], collections: &[bool], columns: usize) -> Vec<Group> {
    let columns = columns.clamp(1, 6);
    let mut groups = Vec::new();
    let mut section = 0;
    while section < labels.len() {
        let mut end = section + 1;
        while end < labels.len() && labels[end] == labels[section] {
            end += 1;
        }
        let mut start = section;
        if section == 0 {
            let count = (end - start).min(if columns >= 5 {
                5
            } else if columns >= 3 {
                3
            } else {
                1
            });
            groups.push(Group {
                start,
                count,
                title: labels[section].clone(),
                layout: Layout::Spotlight,
            });
            start += count;
        }
        while start < end {
            let collection = collections.get(start).copied().unwrap_or(false);
            let mut run_end = start + 1;
            while run_end < end && collections.get(run_end).copied().unwrap_or(false) == collection
            {
                run_end += 1;
            }
            let tiles = collection && run_end - start > 1;
            // Pair isolated playlists with neighbouring songs instead of
            // wasting a whole cover shelf on a single item.
            let count = if tiles {
                (run_end - start).min(columns)
            } else {
                (end - start).min(columns.min(2))
            };
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
        for columns in 1..=6 {
            let groups = home_groups(&labels, &[false, true, false, true, true, false], columns);
            let indices: Vec<_> = groups
                .iter()
                .flat_map(|g| g.start..g.start + g.count)
                .collect();
            assert_eq!(indices, (0..labels.len()).collect::<Vec<_>>());
            assert_eq!(groups.iter().filter(|g| !g.title.is_empty()).count(), 3);
            assert!(groups
                .iter()
                .all(|g| g.count <= columns || g.layout == Layout::Spotlight));
            assert_eq!(groups[0].layout, Layout::Spotlight);
        }
        assert!(home_groups(&[], &[], 0).is_empty());
    }
    #[test]
    fn narrow_mixed_recommendations_use_compact_pairs() {
        let groups = home_groups(
            &vec!["Listen again".into(); 7],
            &[true, false, true, false, false, false, false],
            2,
        );
        assert_eq!(groups[0].count, 1);
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
