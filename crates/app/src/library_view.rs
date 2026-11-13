//! Library filtering and sorting. The UI shows entries in this order but keeps
//! each entry's source index, so opening an item always opens the right one.

/// Library sort orders shown in the Library header.
pub const SORT_RECENT: i32 = 0;
pub const SORT_TITLE: i32 = 1;

/// Indices of the entries whose title or subtitle contains `filter`
/// (case-insensitive), in YouTube Music's order or alphabetically by title.
pub fn order<'a>(
    entries: impl IntoIterator<Item = (&'a str, &'a str)>,
    filter: &str,
    sort: i32,
) -> Vec<usize> {
    let needle = filter.trim().to_lowercase();
    let mut matches: Vec<(usize, String)> = entries
        .into_iter()
        .enumerate()
        .filter(|(_, (title, subtitle))| {
            needle.is_empty()
                || title.to_lowercase().contains(&needle)
                || subtitle.to_lowercase().contains(&needle)
        })
        .map(|(index, (title, _))| (index, title.to_lowercase()))
        .collect();
    if sort == SORT_TITLE {
        // Stable: equal titles keep YouTube Music's order.
        matches.sort_by(|a, b| a.1.cmp(&b.1));
    }
    matches.into_iter().map(|(index, _)| index).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENTRIES: [(&str, &str); 4] = [
        ("Weekend favourites", "56 tracks"),
        ("Late night focus", "24 tracks"),
        ("Morning run", "Nova Wells"),
        ("ambient", "Late set"),
    ];

    #[test]
    fn empty_filter_keeps_provider_order() {
        assert_eq!(order(ENTRIES, "", SORT_RECENT), [0, 1, 2, 3]);
        assert_eq!(order(ENTRIES, "   ", SORT_RECENT), [0, 1, 2, 3]);
    }

    #[test]
    fn filter_matches_title_or_subtitle_ignoring_case() {
        assert_eq!(order(ENTRIES, "LATE", SORT_RECENT), [1, 3]);
        assert_eq!(order(ENTRIES, "nova", SORT_RECENT), [2]);
        assert!(order(ENTRIES, "missing", SORT_RECENT).is_empty());
    }

    #[test]
    fn title_sort_is_case_insensitive_and_keeps_source_indices() {
        assert_eq!(order(ENTRIES, "", SORT_TITLE), [3, 1, 2, 0]);
        assert_eq!(order(ENTRIES, "late", SORT_TITLE), [3, 1]);
    }
}
