//! Presentation groups retain the provider's exact order and source indices.
#[derive(Debug, PartialEq)]
pub struct Group {
    pub start: usize,
    pub count: usize,
    pub title: String,
}

pub fn home_groups(labels: &[String], columns: usize) -> Vec<Group> {
    let columns = columns.clamp(1, 6);
    let mut groups = Vec::new();
    let mut section = 0;
    while section < labels.len() {
        let mut end = section + 1;
        while end < labels.len() && labels[end] == labels[section] {
            end += 1;
        }
        for start in (section..end).step_by(columns) {
            groups.push(Group {
                start,
                count: (end - start).min(columns),
                title: if start == section {
                    labels[section].clone()
                } else {
                    String::new()
                },
            });
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
            let groups = home_groups(&labels, columns);
            let indices: Vec<_> = groups
                .iter()
                .flat_map(|g| g.start..g.start + g.count)
                .collect();
            assert_eq!(indices, (0..labels.len()).collect::<Vec<_>>());
            assert_eq!(groups.iter().filter(|g| !g.title.is_empty()).count(), 3);
            assert!(groups.iter().all(|g| g.count <= columns));
        }
        assert!(home_groups(&[], 0).is_empty());
    }
}
