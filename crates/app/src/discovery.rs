//! Session-only selection from actual personalized home recommendations.

use std::collections::{HashMap, HashSet, VecDeque};
use tide_provider::Card;

const MAX_CANDIDATES: usize = 240;
const VISIBLE_CARDS: usize = 24;
const RECENT_CARDS: usize = 240;
type Identity = (String, String);

#[derive(Default)]
pub(crate) struct Discovery {
    // Oldest first. Store identities only; upstream card metadata stays fresh.
    recent: VecDeque<Identity>,
    last_display: Vec<Identity>,
}

struct Candidate {
    card: Card,
    identity: Identity,
    recent_index: Option<usize>,
    was_last_display: bool,
    tie_break: u64,
}

impl Discovery {
    pub(crate) fn clear(&mut self) {
        self.recent.clear();
        self.last_display.clear();
    }

    pub(crate) fn select(&mut self, cards: Vec<Card>, mut seed: u64) -> Vec<Card> {
        let mut seen = HashSet::new();
        let mut candidates: Vec<_> = cards
            .into_iter()
            .take(MAX_CANDIDATES)
            .filter_map(|card| {
                // The native Home view currently opens songs and playlists.
                // Keep unsupported/unnamed cards out of both display and history.
                if !matches!(card.kind.as_str(), "song" | "playlist")
                    || card.id.trim().is_empty()
                    || card.title.trim().is_empty()
                {
                    return None;
                }
                let identity = (card.kind.clone(), card.id.clone());
                if !seen.insert(identity.clone()) {
                    return None;
                }
                Some(Candidate {
                    recent_index: self.recent.iter().position(|key| key == &identity),
                    was_last_display: self.last_display.contains(&identity),
                    tie_break: random(&mut seed),
                    card,
                    identity,
                })
            })
            .collect();
        let mut selected = Vec::with_capacity(VISIBLE_CARDS.min(candidates.len()));
        let mut shelves: HashMap<String, usize> = HashMap::new();
        let mut kinds: HashMap<String, usize> = HashMap::new();

        while selected.len() < VISIBLE_CARDS && !candidates.is_empty() {
            let index = candidates
                .iter()
                .enumerate()
                .min_by_key(|(_, candidate)| {
                    (
                        candidate.recent_index.is_some(),
                        candidate.was_last_display,
                        // Balance shelves and kinds within the oldest batch,
                        // while ensuring newer batches cannot starve older ones.
                        candidate.recent_index.map_or(0, |age| age / VISIBLE_CARDS),
                        shelves.get(&candidate.card.shelf).copied().unwrap_or(0),
                        kinds.get(&candidate.card.kind).copied().unwrap_or(0),
                        candidate.tie_break,
                    )
                })
                .map(|(index, _)| index)
                .expect("nonempty candidate pool");
            let candidate = candidates.swap_remove(index);
            *shelves.entry(candidate.card.shelf.clone()).or_default() += 1;
            *kinds.entry(candidate.card.kind.clone()).or_default() += 1;
            selected.push(candidate);
        }

        // Even a small, unchanged upstream pool remains useful. Avoid returning
        // the identical order twice when there is more than one available card.
        if selected.len() > 1
            && selected
                .iter()
                .map(|candidate| &candidate.identity)
                .eq(self.last_display.iter())
        {
            selected.rotate_left(1);
        }
        self.last_display.clear();
        let mut result = Vec::with_capacity(selected.len());
        for candidate in selected {
            self.recent.retain(|key| key != &candidate.identity);
            self.recent.push_back(candidate.identity.clone());
            self.last_display.push(candidate.identity);
            result.push(candidate.card);
        }
        while self.recent.len() > RECENT_CARDS {
            self.recent.pop_front();
        }
        result
    }
}

// SplitMix64: deterministic selection without an additional runtime dependency.
fn random(seed: &mut u64) -> u64 {
    *seed = seed.wrapping_add(0x9e3779b97f4a7c15);
    let mut value = *seed;
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d049bb133111eb);
    value ^ (value >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(index: usize) -> Card {
        Card {
            id: format!("candidate-{index}"),
            title: format!("Personalized title {index}"),
            subtitle: format!("Artist {index}"),
            kind: ["song", "playlist"][index % 2].into(),
            shelf: format!("Shelf {}", (index / 2) % 8),
            artwork_url: Some(format!("https://i.ytimg.com/{index}")),
            duration_seconds: Some(180 + index as u64),
        }
    }

    fn pool(count: usize) -> Vec<Card> {
        (0..count).map(card).collect()
    }

    fn identities(cards: &[Card]) -> HashSet<Identity> {
        cards
            .iter()
            .map(|card| (card.kind.clone(), card.id.clone()))
            .collect()
    }

    fn order(cards: &[Card]) -> Vec<&str> {
        cards.iter().map(|card| card.id.as_str()).collect()
    }

    #[test]
    fn unchanged_large_pool_exhausts_unseen_before_reusing_any_candidates() {
        let mut discovery = Discovery::default();
        let mut shown = HashSet::new();
        for refresh in 0..10 {
            let cards = discovery.select(pool(MAX_CANDIDATES), refresh);
            assert_eq!(cards.len(), VISIBLE_CARDS);
            for identity in identities(&cards) {
                assert!(shown.insert(identity), "repeated before pool was exhausted");
            }
        }
        assert_eq!(shown.len(), MAX_CANDIDATES);
        let reused = identities(&discovery.select(pool(MAX_CANDIDATES), 10));
        assert_eq!(reused.len(), VISIBLE_CARDS);
        assert!(reused.is_subset(&shown));
    }

    #[test]
    fn successive_refreshes_are_disjoint_and_reuse_does_not_starve_old_cards() {
        let mut discovery = Discovery::default();
        let first = identities(&discovery.select(pool(48), 7));
        let second = identities(&discovery.select(pool(48), 7));
        assert!(first.is_disjoint(&second));
        for _ in 0..5 {
            assert_eq!(identities(&discovery.select(pool(48), 7)), first);
            assert_eq!(identities(&discovery.select(pool(48), 7)), second);
        }
    }

    #[test]
    fn selection_balances_shelves_and_kinds_and_preserves_metadata() {
        let selected = Discovery::default().select(pool(MAX_CANDIDATES), 42);
        let mut shelves = HashMap::<&str, usize>::new();
        let mut kinds = HashMap::<&str, usize>::new();
        for actual in &selected {
            *shelves.entry(&actual.shelf).or_default() += 1;
            *kinds.entry(&actual.kind).or_default() += 1;
            let index: usize = actual
                .id
                .strip_prefix("candidate-")
                .unwrap()
                .parse()
                .unwrap();
            let expected = card(index);
            assert_eq!(actual.title, expected.title);
            assert_eq!(actual.subtitle, expected.subtitle);
            assert_eq!(actual.artwork_url, expected.artwork_url);
            assert_eq!(actual.duration_seconds, expected.duration_seconds);
            assert_eq!(actual.shelf, expected.shelf);
            assert_eq!(actual.kind, expected.kind);
        }
        assert_eq!(shelves.len(), 8);
        assert!(shelves.values().all(|&count| count == 3));
        assert_eq!(kinds.len(), 2);
        assert!(kinds.values().all(|&count| count == 12));
    }

    #[test]
    fn small_pools_stay_nonempty_deduplicate_and_change_order() {
        let mut discovery = Discovery::default();
        for count in [1, 2, 7, VISIBLE_CARDS] {
            discovery.clear();
            let first = discovery.select(pool(count), 1);
            let second = discovery.select(pool(count), 1);
            assert_eq!(first.len(), count);
            assert_eq!(second.len(), count);
            if count > 1 {
                assert_ne!(order(&first), order(&second));
            }
        }
        let mut repeated = vec![card(0), card(0), card(1), card(1)];
        repeated[1].title = "Duplicate metadata must not replace first".into();
        let selected = discovery.select(repeated, 1);
        assert_eq!(selected.len(), 2);
        assert_eq!(identities(&selected).len(), 2);
        assert_eq!(
            selected
                .iter()
                .find(|card| card.id == "candidate-0")
                .unwrap()
                .title,
            "Personalized title 0"
        );
        assert!(discovery.select(Vec::new(), 1).is_empty());
    }

    #[test]
    fn playable_kinds_share_ids_without_colliding() {
        let first = card(0);
        let mut second = card(0);
        second.kind = "playlist".into();
        second.title = "Actual upstream playlist".into();
        let selected = Discovery::default().select(vec![first, second], 2);
        assert_eq!(selected.len(), 2);
        assert_eq!(identities(&selected).len(), 2);
        assert!(selected
            .iter()
            .any(|card| card.kind == "playlist" && card.title == "Actual upstream playlist"));
    }

    #[test]
    fn unsupported_or_unnamed_cards_never_enter_display_or_history() {
        let mut cards: Vec<_> = ["album", "artist", "unknown", "", "Song"]
            .into_iter()
            .enumerate()
            .map(|(index, kind)| {
                let mut card = card(index);
                card.kind = kind.into();
                card
            })
            .collect();
        for blank in ["", " \t\n"] {
            let mut missing_id = card(100);
            missing_id.id = blank.into();
            cards.push(missing_id);
            let mut missing_title = card(101);
            missing_title.title = blank.into();
            cards.push(missing_title);
        }
        cards.push(card(200));
        cards.push(card(201));
        let mut discovery = Discovery::default();
        let selected = discovery.select(cards, 9);
        assert_eq!(identities(&selected), identities(&[card(200), card(201)]));
        assert_eq!(discovery.recent.len(), 2);
        assert_eq!(discovery.last_display.len(), 2);
        let mut unsupported = card(202);
        unsupported.kind = "album".into();
        assert!(discovery.select(vec![unsupported], 9).is_empty());
        assert_eq!(discovery.recent.len(), 2);
    }

    #[test]
    fn seed_and_reset_are_reproducible_and_memory_and_inputs_are_bounded() {
        let mut discovery = Discovery::default();
        let first = discovery.select(pool(MAX_CANDIDATES + 50), 99);
        assert!(first.iter().all(|card| card
            .id
            .strip_prefix("candidate-")
            .unwrap()
            .parse::<usize>()
            .unwrap()
            < MAX_CANDIDATES));
        assert_eq!(
            order(&first),
            order(&Discovery::default().select(pool(MAX_CANDIDATES), 99))
        );
        discovery.clear();
        assert_eq!(
            order(&first),
            order(&discovery.select(pool(MAX_CANDIDATES), 99))
        );
        for offset in 0..25 {
            let cards = (0..MAX_CANDIDATES)
                .map(|index| card(index + offset * MAX_CANDIDATES))
                .collect();
            discovery.select(cards, offset as u64);
            assert!(discovery.recent.len() <= RECENT_CARDS);
            assert!(discovery.last_display.len() <= VISIBLE_CARDS);
        }
    }
}
