//! Session-only selection from actual personalized home recommendations.
//!
//! YouTube Music returns Home as shelves ("Quick picks", "Mixed for you",
//! "New releases", "Listen again", ...). Each shelf keeps its own slot budget
//! and rotates within its leading cards on refresh, so one long shelf can't
//! crowd out the rest and refreshes surface new picks instead of the same
//! first two dozen songs. "Listen again" is familiar by definition, so it sits
//! after the discovery shelves.

use std::collections::{HashSet, VecDeque};
use tide_provider::Card;

const MAX_CANDIDATES: usize = 320;
const MAX_SHELVES: usize = 10;
const LEAD_SHELF_CARDS: usize = 12;
const SHELF_CARDS: usize = 8;
const FAMILIAR_SHELF_CARDS: usize = 6;
// Rotate within each shelf's leading cards, never mine its tail for novelty.
const WINDOW_FACTOR: usize = 2;
const RECENT_CARDS: usize = 240;
#[cfg(test)]
const VISIBLE_CARDS: usize = LEAD_SHELF_CARDS + (MAX_SHELVES - 1) * SHELF_CARDS;
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

struct Shelf {
    title: String,
    candidates: Vec<Candidate>,
}

fn familiar(title: &str) -> bool {
    let title = title.to_ascii_lowercase();
    [
        "listen again",
        "keep listening",
        "from your library",
        "recently played",
    ]
    .iter()
    .any(|label| title.contains(label))
}

impl Discovery {
    pub(crate) fn clear(&mut self) {
        self.recent.clear();
        self.last_display.clear();
    }

    pub(crate) fn select(&mut self, cards: Vec<Card>) -> Vec<Card> {
        let mut seen = HashSet::new();
        let mut shelves: Vec<Shelf> = Vec::new();
        for card in cards.into_iter().take(MAX_CANDIDATES) {
            // Keep supported collections in provider order alongside songs.
            // Opening validates their canonical API identity separately.
            if !matches!(card.kind.as_str(), "song" | "playlist" | "album" | "artist")
                || card.id.trim().is_empty()
                || card.title.trim().is_empty()
            {
                continue;
            }
            let identity = (card.kind.clone(), card.id.clone());
            if !seen.insert(identity.clone()) {
                continue;
            }
            let rank = seen.len() - 1;
            let candidate = Candidate {
                card,
                identity,
                rank,
            };
            if let Some(shelf) = shelves
                .iter_mut()
                .find(|shelf| shelf.title == candidate.card.shelf)
            {
                shelf.candidates.push(candidate);
            } else if shelves.len() < MAX_SHELVES {
                shelves.push(Shelf {
                    title: candidate.card.shelf.clone(),
                    candidates: vec![candidate],
                });
            }
        }
        if shelves.is_empty() {
            // The caller keeps the current Home screen on an empty response.
            // Keep its recency state too, rather than treating it as a reset.
            return Vec::new();
        }
        // Discovery shelves first in provider order, familiar ones after.
        shelves.sort_by_key(|shelf| familiar(&shelf.title));
        let lead = shelves.iter().position(|shelf| !familiar(&shelf.title));
        let mut chosen = Vec::new();
        for (index, shelf) in shelves.into_iter().enumerate() {
            let quota = if familiar(&shelf.title) {
                FAMILIAR_SHELF_CARDS
            } else if Some(index) == lead {
                LEAD_SHELF_CARDS
            } else {
                SHELF_CARDS
            };
            chosen.extend(self.rotate(shelf.candidates, quota));
        }
        self.last_display.clear();
        let mut result = Vec::with_capacity(chosen.len());
        for candidate in chosen {
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

    /// Provider order is the relevance signal. The leading third of each
    /// shelf always stays; the other slots cycle through the rest of the
    /// shelf's leading window, unseen cards first and then the ones shown
    /// longest ago, so every refresh brings new picks without reaching into
    /// the shelf's weaker tail.
    fn rotate(&self, mut candidates: Vec<Candidate>, quota: usize) -> Vec<Candidate> {
        candidates.truncate(quota * WINDOW_FACTOR);
        let anchors = (quota / 3).max(1).min(candidates.len());
        let mut rest = candidates.split_off(anchors);
        rest.sort_by_cached_key(|candidate| {
            let shown = self
                .recent
                .iter()
                .position(|identity| identity == &candidate.identity)
                .map_or(0, |index| index + 1);
            (shown, candidate.rank)
        });
        rest.truncate(quota - anchors);
        candidates.extend(rest);
        // Display the chosen cards in their original order.
        candidates.sort_by_key(|candidate| candidate.rank);
        candidates
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHELF_SIZE: usize = 24;

    fn card(index: usize) -> Card {
        Card {
            artists: Vec::new(),
            id: format!("candidate-{index}"),
            title: format!("Personalized title {index}"),
            subtitle: format!("Artist {index}"),
            kind: ["song", "playlist"][index % 2].into(),
            shelf: format!("Shelf {}", index / SHELF_SIZE),
            artwork_url: Some(format!("https://i.ytimg.com/{index}")),
            featured_artwork_url: None,
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

    fn index(card: &Card) -> usize {
        card.id.strip_prefix("candidate-").unwrap().parse().unwrap()
    }

    fn shelf_cards<'a>(cards: &'a [Card], shelf: &str) -> Vec<&'a Card> {
        cards.iter().filter(|card| card.shelf == shelf).collect()
    }

    #[test]
    fn first_page_takes_each_shelf_lead_in_upstream_order() {
        let selected = Discovery::default().select(pool(SHELF_SIZE * 4));
        let expected: Vec<usize> = (0..LEAD_SHELF_CARDS)
            .chain((1..4).flat_map(|shelf| shelf * SHELF_SIZE..shelf * SHELF_SIZE + SHELF_CARDS))
            .collect();
        assert_eq!(selected.iter().map(index).collect::<Vec<_>>(), expected);
    }

    #[test]
    fn every_shelf_is_represented_and_a_long_one_cannot_crowd_out_the_rest() {
        let mut cards = pool(SHELF_SIZE * 3);
        for card in &mut cards[..SHELF_SIZE * 2] {
            card.shelf = "Quick picks".into();
        }
        let selected = Discovery::default().select(cards);
        assert_eq!(
            shelf_cards(&selected, "Quick picks").len(),
            LEAD_SHELF_CARDS
        );
        assert_eq!(shelf_cards(&selected, "Shelf 2").len(), SHELF_CARDS);
    }

    #[test]
    fn listen_again_moves_after_discovery_shelves_with_a_smaller_budget() {
        let mut cards = pool(SHELF_SIZE * 3);
        for card in &mut cards[..SHELF_SIZE] {
            card.shelf = "Listen again".into();
        }
        let selected = Discovery::default().select(cards);
        let first_familiar = selected
            .iter()
            .position(|card| card.shelf == "Listen again")
            .unwrap();
        assert!(selected[..first_familiar]
            .iter()
            .all(|card| card.shelf != "Listen again"));
        assert_eq!(selected[0].shelf, "Shelf 1");
        assert_eq!(shelf_cards(&selected, "Shelf 1").len(), LEAD_SHELF_CARDS);
        assert_eq!(
            shelf_cards(&selected, "Listen again").len(),
            FAMILIAR_SHELF_CARDS
        );
    }

    #[test]
    fn refresh_rotates_each_shelf_but_keeps_its_leading_picks() {
        let mut discovery = Discovery::default();
        let mut previous = identities(&discovery.select(pool(SHELF_SIZE * 4)));
        let anchors = identities(&[card(0), card(1), card(SHELF_SIZE)]);
        let mut ever_shown = HashSet::new();
        for _ in 0..16 {
            let selected = discovery.select(pool(SHELF_SIZE * 4));
            let current = identities(&selected);
            assert_ne!(current, previous);
            assert!(anchors.is_subset(&current));
            for shelf in 0..4 {
                let ranks: Vec<usize> = shelf_cards(&selected, &format!("Shelf {shelf}"))
                    .into_iter()
                    .map(index)
                    .collect();
                let quota = if shelf == 0 {
                    LEAD_SHELF_CARDS
                } else {
                    SHELF_CARDS
                };
                assert_eq!(ranks.len(), quota);
                assert!(ranks
                    .iter()
                    .all(|&rank| rank < shelf * SHELF_SIZE + quota * WINDOW_FACTOR));
                assert!(ranks.windows(2).all(|pair| pair[0] < pair[1]));
            }
            ever_shown.extend(current.iter().cloned());
            previous = current;
        }
        // Rotation reaches every card in each shelf's leading window.
        let first_page = LEAD_SHELF_CARDS + 3 * SHELF_CARDS;
        assert_eq!(ever_shown.len(), first_page * WINDOW_FACTOR);
    }

    #[test]
    fn newly_ranked_top_cards_outrank_the_previous_selection() {
        let mut discovery = Discovery::default();
        discovery.select(pool(SHELF_SIZE));
        let fresh: Vec<_> = (1000..1000 + LEAD_SHELF_CARDS)
            .map(|index| {
                let mut card = card(index);
                card.shelf = "Shelf 0".into();
                card
            })
            .collect();
        let mut updated = fresh.clone();
        updated.extend(pool(SHELF_SIZE));
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
            let expected = card(index(actual));
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
        for count in [1, 2, 7, LEAD_SHELF_CARDS] {
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
        let mut cards: Vec<_> = ["podcast", "video-channel", "unknown", "", "Song"]
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
        unsupported.kind = "podcast".into();
        assert!(discovery.select(vec![unsupported]).is_empty());
        assert_eq!(discovery.recent.len(), 2);
    }

    #[test]
    fn albums_and_artists_keep_their_original_recommendation_rank() {
        let mut cards = pool(4);
        cards[1].kind = "album".into();
        cards[1].id = "MPREb_fixture".into();
        cards[2].kind = "artist".into();
        cards[2].id = "UC1234567890123456789012".into();
        let expected = order(&cards)
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let selected = Discovery::default().select(cards);
        assert_eq!(order(&selected), expected);
    }

    #[test]
    fn reset_restores_upstream_order_and_memory_and_inputs_are_bounded() {
        let mut discovery = Discovery::default();
        let first = discovery.select(pool(MAX_CANDIDATES + 50));
        assert!(first.iter().all(|card| index(card) < MAX_CANDIDATES));
        assert!(first.len() <= VISIBLE_CARDS);
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
