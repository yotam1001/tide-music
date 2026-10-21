//! Session-only selection from actual personalized home recommendations.

use std::collections::{HashSet, VecDeque};
use tide_provider::Card;

const MAX_CANDIDATES: usize = 240;
const VISIBLE_CARDS: usize = 24;
// Vary within the leading two screens, never mine the full pool for novelty.
const RELEVANCE_WINDOW: usize = 48;
const RECENT_CARDS: usize = RELEVANCE_WINDOW;
const LAST_DISPLAY_PENALTY: usize = 16;
const RECENT_PENALTY: usize = 8;
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
    rank: usize,
}

impl Discovery {
    pub(crate) fn clear(&mut self) {
        self.recent.clear();
        self.last_display.clear();
    }

    pub(crate) fn select(&mut self, cards: Vec<Card>) -> Vec<Card> {
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
                    rank: seen.len() - 1,
                    card,
                    identity,
                })
            })
            .take(RELEVANCE_WINDOW)
            .collect();
        if candidates.is_empty() {
            // The caller keeps the current Home screen on an empty response.
            // Keep its recency state too, rather than treating it as a reset.
            return Vec::new();
        }
        // Provider order is the only relevance signal we actually have. Give
        // repeat cards a bounded penalty, instead of letting every unseen card
        // outrank all familiar songs or enforcing a song/playlist split.
        candidates.sort_by_key(|candidate| {
            let penalty = if self.last_display.contains(&candidate.identity) {
                LAST_DISPLAY_PENALTY
            } else if self.recent.contains(&candidate.identity) {
                RECENT_PENALTY
            } else {
                0
            };
            (candidate.rank + penalty, candidate.rank)
        });
        candidates.truncate(VISIBLE_CARDS);
        // Display the chosen cards in their original order and shelf context.
        candidates.sort_by_key(|candidate| candidate.rank);
        self.last_display.clear();
        let mut result = Vec::with_capacity(candidates.len());
        for candidate in candidates {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn card(index: usize) -> Card {
        Card {
            artists: Vec::new(),
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
    fn first_page_preserves_upstream_order_without_random_shelf_balancing() {
        let selected = Discovery::default().select(pool(MAX_CANDIDATES));
        assert_eq!(order(&selected), order(&pool(VISIBLE_CARDS)));
    }

    #[test]
    fn unseen_playlists_do_not_force_out_the_leading_recommended_songs() {
        let mut cards = pool(MAX_CANDIDATES);
        for (index, card) in cards.iter_mut().enumerate() {
            card.kind = if index < 32 { "song" } else { "playlist" }.into();
            card.shelf = if index < 32 { "For you" } else { "Explore" }.into();
        }
        let mut discovery = Discovery::default();
        let leading_songs = identities(&cards[..16]);
        let first = discovery.select(cards.clone());
        assert!(first.iter().all(|card| card.kind == "song"));
        for _ in 0..12 {
            let selected = discovery.select(cards.clone());
            assert_eq!(selected.len(), VISIBLE_CARDS);
            assert!(leading_songs.is_subset(&identities(&selected)));
            assert!(
                selected
                    .iter()
                    .filter(|card| card.kind == "playlist")
                    .count()
                    <= 8
            );
        }
    }

    #[test]
    fn refresh_changes_the_selection_without_promoting_distant_unseen_cards() {
        let mut discovery = Discovery::default();
        let mut previous = identities(&discovery.select(pool(MAX_CANDIDATES)));
        let anchors = identities(&pool(8));
        for _ in 0..32 {
            let selected = discovery.select(pool(MAX_CANDIDATES));
            let current = identities(&selected);
            assert_eq!(current.len(), VISIBLE_CARDS);
            assert_ne!(current, previous);
            assert!(anchors.is_subset(&current));
            let ranks: Vec<usize> = selected
                .iter()
                .map(|card| card.id.strip_prefix("candidate-").unwrap().parse().unwrap())
                .collect();
            assert!(ranks.iter().all(|&rank| rank < RELEVANCE_WINDOW));
            assert!(ranks.windows(2).all(|pair| pair[0] < pair[1]));
            previous = current;
        }
    }

    #[test]
    fn newly_ranked_top_cards_outrank_the_previous_selection() {
        let mut discovery = Discovery::default();
        discovery.select(pool(MAX_CANDIDATES));
        let fresh: Vec<_> = (1000..1000 + VISIBLE_CARDS).map(card).collect();
        let mut updated = fresh.clone();
        updated.extend(pool(MAX_CANDIDATES));
        assert_eq!(order(&discovery.select(updated)), order(&fresh));
    }

    #[test]
    fn empty_responses_preserve_recency_for_the_screen_the_caller_keeps() {
        let mut discovery = Discovery::default();
        discovery.select(pool(MAX_CANDIDATES));
        let recent = discovery.recent.clone();
        let last_display = discovery.last_display.clone();
        assert!(discovery.select(Vec::new()).is_empty());
        assert_eq!(discovery.recent, recent);
        assert_eq!(discovery.last_display, last_display);
    }

    #[test]
    fn upstream_metadata_and_shelf_context_are_preserved() {
        let selected = Discovery::default().select(pool(MAX_CANDIDATES));
        for actual in &selected {
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
    }

    #[test]
    fn small_pools_stay_nonempty_deduplicate_and_keep_upstream_order() {
        let mut discovery = Discovery::default();
        for count in [1, 2, 7, VISIBLE_CARDS] {
            discovery.clear();
            let first = discovery.select(pool(count));
            let second = discovery.select(pool(count));
            assert_eq!(first.len(), count);
            assert_eq!(second.len(), count);
            assert_eq!(order(&first), order(&second));
        }
        let mut repeated = vec![card(0), card(0), card(1), card(1)];
        repeated[1].title = "Duplicate metadata must not replace first".into();
        let selected = discovery.select(repeated);
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
        assert!(discovery.select(Vec::new()).is_empty());
    }

    #[test]
    fn playable_kinds_share_ids_without_colliding() {
        let first = card(0);
        let mut second = card(0);
        second.kind = "playlist".into();
        second.title = "Actual upstream playlist".into();
        let selected = Discovery::default().select(vec![first, second]);
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
        let selected = discovery.select(cards);
        assert_eq!(identities(&selected), identities(&[card(200), card(201)]));
        assert_eq!(discovery.recent.len(), 2);
        assert_eq!(discovery.last_display.len(), 2);
        let mut unsupported = card(202);
        unsupported.kind = "album".into();
        assert!(discovery.select(vec![unsupported]).is_empty());
        assert_eq!(discovery.recent.len(), 2);
    }

    #[test]
    fn reset_restores_upstream_order_and_memory_and_inputs_are_bounded() {
        let mut discovery = Discovery::default();
        let first = discovery.select(pool(MAX_CANDIDATES + 50));
        assert!(first.iter().all(|card| card
            .id
            .strip_prefix("candidate-")
            .unwrap()
            .parse::<usize>()
            .unwrap()
            < MAX_CANDIDATES));
        assert_eq!(
            order(&first),
            order(&Discovery::default().select(pool(MAX_CANDIDATES)))
        );
        discovery.clear();
        assert_eq!(
            order(&first),
            order(&discovery.select(pool(MAX_CANDIDATES)))
        );
        for offset in 0..25 {
            let cards = (0..MAX_CANDIDATES)
                .map(|index| card(index + offset * MAX_CANDIDATES))
                .collect();
            discovery.select(cards);
            assert!(discovery.recent.len() <= RECENT_CARDS);
            assert!(discovery.last_display.len() <= VISIBLE_CARDS);
        }
    }
}
