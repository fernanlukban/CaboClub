use std::f32::consts::{FRAC_PI_2, PI};

use bevy::prelude::*;
use cabo_shared::protocol::{Action, Direction};

use super::{CardKey, Frame, Placement, frame::CardView};

pub(super) fn seat_anchor(direction: Direction, width: f32) -> (Vec3, f32) {
    let horizontal = (width * 0.34).min(7.0);
    match direction {
        Direction::North => (Vec3::new(0.0, 0.0, -2.8), PI),
        Direction::East => (Vec3::new(horizontal, 0.0, 0.0), -FRAC_PI_2),
        Direction::South => (Vec3::new(0.0, 0.0, 2.8), 0.0),
        Direction::West => (Vec3::new(-horizontal, 0.0, 0.0), FRAC_PI_2),
    }
}

pub(super) fn layout(frame: &Frame, width: f32, new_flip: bool) -> Vec<Placement> {
    let Some(snapshot) = &frame.snapshot else {
        return vec![];
    };
    let is_board = snapshot.role == "board";
    if snapshot.phase == "lobby" {
        return vec![];
    }
    let active = snapshot.turn_seat_id.is_some() && snapshot.turn_seat_id == snapshot.you_seat_id;
    let mini = active
        && snapshot.pending_power.as_ref().is_some_and(|power| {
            matches!(
                power.phase.as_str(),
                "peekSelecting"
                    | "peekViewing"
                    | "spySelecting"
                    | "spyReview"
                    | "spyViewing"
                    | "swapSelectingOpponent"
                    | "swapReview"
            )
        });
    let pending = frame
        .optimistic
        .as_ref()
        .filter(|command| command.deal_id == snapshot.deal_id);
    let closing_look = pending.is_some_and(|command| command.action == Action::ConfirmInitialLook)
        && snapshot.phase == "initialPreview";
    let optimistic = pending.filter(|command| command.expected_revision == snapshot.revision);
    let viewing_slot = snapshot
        .private_power_result
        .as_ref()
        .filter(|_| mini && !is_board)
        .and_then(|look| look.slot)
        .or_else(|| {
            snapshot
                .pending_power
                .as_ref()
                .filter(|power| {
                    (mini || is_board) && matches!(power.phase.as_str(), "spyReview" | "spyViewing")
                })
                .and_then(|power| power.target_slot)
        });
    let actor = snapshot.pending_power.as_ref().and_then(|power| {
        snapshot
            .players
            .iter()
            .find(|p| p.seat_id == power.actor_seat_id)
    });
    let viewing_position = if is_board {
        let toward = match actor.map(|player| player.direction) {
            Some(Direction::North) => Vec3::NEG_Z,
            Some(Direction::South) => Vec3::Z,
            Some(Direction::West) => Vec3::NEG_X,
            Some(Direction::East) => Vec3::X,
            None => Vec3::ZERO,
        };
        toward * 1.7 + Vec3::Y * 0.48
    } else {
        Vec3::new(0.0, 0.48, 1.35)
    };
    let mut placements = vec![];
    let mut row = |cards: &[&CardView],
                   origin: Vec3,
                   angle: f32,
                   max_width: f32,
                   card_size: f32,
                   copies: bool,
                   marked: Option<[u32; 2]>| {
        let columns = cards.len().clamp(1, 8);
        let size = card_size.min(max_width / columns as f32 / 1.15);
        let rotation = Quat::from_rotation_y(angle);
        for (i, card) in cards.iter().enumerate() {
            let count = (cards.len() - i / columns * columns).min(columns);
            let x = (i % columns) as f32 - (count - 1) as f32 / 2.0;
            let local = Vec3::new(x * size * 1.15, 0.0, (i / columns) as f32 * size * 1.48);
            let private_rank = snapshot
                .private_power_result
                .as_ref()
                .filter(|look| mini && !is_board && look.slot == Some(card.slot))
                .and_then(|look| look.rank);
            let selected = private_rank.is_some()
                || frame.selection.contains(&card.slot)
                || snapshot
                    .pending_power
                    .as_ref()
                    .is_some_and(|p| [p.own_slot, p.target_slot].contains(&Some(card.slot)));
            let outgoing = optimistic.is_some_and(|command| match command.action {
                Action::DiscardHandCard => command.slots.contains(&card.slot),
                Action::DiscardDrawn => snapshot
                    .incoming
                    .as_ref()
                    .is_some_and(|incoming| incoming.slot == card.slot),
                _ => false,
            });
            let matching = optimistic.is_some_and(|command| {
                command.action == Action::CallMatch && command.slots.contains(&card.slot)
            });
            let home = origin + rotation * local;
            let inspecting = viewing_slot == Some(card.slot) && !copies;
            let position = if inspecting {
                viewing_position
            } else if outgoing {
                Vec3::new(0.75, 0.16, -1.35)
            } else {
                let lift = if matching {
                    0.9
                } else if selected {
                    0.65
                } else {
                    0.0
                };
                let up = if is_board {
                    rotation * Vec3::NEG_Z
                } else {
                    Vec3::NEG_Z
                };
                home + Vec3::Y * lift + up * (lift * size * 0.64)
            };
            placements.push(Placement {
                key: if copies {
                    CardKey::Match(card.slot)
                } else {
                    CardKey::Hand(card.slot)
                },
                rank: if closing_look {
                    None
                } else {
                    private_rank.or(card.rank)
                },
                position,
                rotation: if inspecting && !is_board { 0.0 } else { angle },
                size: if inspecting { size * 1.3 } else { size },
                home,
                home_rotation: angle,
                home_size: size,
                inspecting,
                marked: selected || marked.is_some_and(|pair| pair.contains(&card.slot)),
                flip: new_flip
                    && snapshot
                        .match_flip
                        .as_ref()
                        .is_some_and(|flip| flip.newly_face_up_slots.contains(&card.slot)),
                interactive: !frame.busy
                    && !copies
                    && !inspecting
                    && snapshot.available_actions.iter().any(|action| {
                        !matches!(
                            action.action,
                            Action::BeginPlay | Action::ConfirmSpy | Action::ConfirmSwap
                        ) && action.slots.contains(&card.slot)
                    }),
            });
        }
    };
    if is_board || mini || matches!(snapshot.phase.as_str(), "roundResults" | "gameOver") {
        for player in &snapshot.players {
            let (anchor, angle) = seat_anchor(player.direction, width);
            let mut cards: Vec<_> = player.cards.iter().collect();
            if snapshot.turn_seat_id == Some(player.seat_id) {
                if let Some(incoming) = &snapshot.incoming {
                    cards.push(incoming);
                }
            }
            row(
                &cards,
                anchor,
                angle,
                (width * 0.44).min(6.4),
                0.96,
                false,
                player.opening_look_slots,
            );
        }
    } else if let Some(player) = snapshot
        .players
        .iter()
        .find(|p| Some(p.seat_id) == snapshot.you_seat_id)
    {
        let mut cards: Vec<_> = player.cards.iter().collect();
        if active {
            if let Some(incoming) = &snapshot.incoming {
                cards.push(incoming);
            }
        }
        row(
            &cards,
            Vec3::new(0.0, 0.0, if active { 1.6 } else { 0.65 }),
            0.0,
            width * 0.85,
            if cards.len() > 8 { 0.9 } else { 1.65 },
            false,
            None,
        );
        if let Some(flip) = &snapshot.match_flip {
            if let Some(actor) = snapshot
                .players
                .iter()
                .find(|p| p.seat_id == flip.actor_seat_id && p.seat_id != player.seat_id)
            {
                let cards: Vec<_> = actor
                    .cards
                    .iter()
                    .filter(|card| flip.slots.contains(&card.slot))
                    .collect();
                row(
                    &cards,
                    Vec3::new(0.0, 0.0, -1.9),
                    0.0,
                    width * 0.75,
                    0.95,
                    true,
                    None,
                );
            }
        }
    }
    if is_board || active {
        for (key, rank, x, present, action) in [
            (
                CardKey::Deck,
                None,
                -0.75,
                snapshot.deck_count > 0,
                Action::DrawDeck,
            ),
            (
                CardKey::Discard,
                snapshot.discard_top,
                0.75,
                snapshot.discard_top.is_some(),
                Action::DrawDiscard,
            ),
        ] {
            if !present {
                continue;
            }
            placements.push(Placement {
                key,
                rank,
                position: Vec3::new(x, 0.0, if is_board || mini { 0.0 } else { -1.35 }),
                rotation: 0.0,
                size: 0.9,
                home: Vec3::ZERO,
                home_rotation: 0.0,
                home_size: 0.9,
                inspecting: false,
                marked: false,
                flip: false,
                interactive: !frame.busy
                    && snapshot
                        .available_actions
                        .iter()
                        .any(|choice| choice.action == action),
            });
        }
    }
    let pending_deck = active
        && !mini
        && snapshot.incoming.is_none()
        && optimistic.is_some_and(|command| command.action == Action::DrawDeck);
    let hidden_deck = is_board
        && snapshot.incoming.is_none()
        && snapshot.incoming_source.as_deref() == Some("deck");
    if pending_deck || hidden_deck {
        let actor_direction = snapshot
            .players
            .iter()
            .find(|p| Some(p.seat_id) == snapshot.turn_seat_id)
            .map(|p| p.direction);
        let (position, rotation) = if hidden_deck {
            let horizontal = (width * 0.34).min(7.0);
            match actor_direction {
                Some(Direction::North) => (Vec3::new(0.0, 0.2, -1.7), PI),
                Some(Direction::East) => (Vec3::new(horizontal - 1.1, 0.2, 0.0), -FRAC_PI_2),
                Some(Direction::West) => (Vec3::new(1.1 - horizontal, 0.2, 0.0), FRAC_PI_2),
                _ => (Vec3::new(0.0, 0.2, 1.7), 0.0),
            }
        } else {
            (Vec3::new(width * 0.26, 0.0, 1.6), 0.0)
        };
        placements.push(Placement {
            key: CardKey::PendingDraw,
            rank: None,
            position,
            rotation,
            size: if is_board { 0.9 } else { 1.25 },
            marked: false,
            flip: false,
            interactive: false,
            home: position,
            home_rotation: rotation,
            home_size: 0.9,
            inspecting: false,
        });
    }
    placements
}
