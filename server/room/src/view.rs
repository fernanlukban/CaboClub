use super::{Phase, Role, Room, TurnStep};
use cabo_shared::{
    protocol::{
        CardPile, CardTravel, PendingPower, PlayerView, PowerResult, RoundResult, Snapshot,
    },
    round_start::{Phase as SetupPhase, Preview},
};

impl Room {
    pub fn snapshot(&self, role: Role) -> Snapshot {
        let viewer = match role {
            Role::Seat(i) => Some(i),
            _ => None,
        };
        let round = self.round.as_ref();
        let public = matches!(self.phase, Phase::Results | Phase::Finished);
        let phase = match self.phase {
            Phase::Lobby => "lobby",
            Phase::Setup => {
                if round.unwrap().opening.phase() == SetupPhase::ReadyToStart {
                    "readyToStart"
                } else {
                    "initialPreview"
                }
            }
            Phase::Playing => "playing",
            Phase::Results => "roundResults",
            Phase::Finished => "gameOver",
        };
        let players = self.players.iter().enumerate().map(|(i,p)| {
            let preview = round.and_then(|r| r.opening.preview(p.id));
            let cards = p.hand.iter().enumerate().map(|(j,c)| {
                let opening = self.phase == Phase::Setup && viewer == Some(i)
                    && matches!(preview, Some(Preview::Revealed(positions)) if positions.contains(&j));
                c.view(public || opening)
            }).collect();
            let opening_look_slots = match preview {
                Some(Preview::Confirmed(positions))
                    if viewer.is_none() && round.is_some_and(|r| r.opening_marks_visible) =>
                {
                    Some(positions.map(|position| p.hand[position].slot))
                }
                _ => None,
            };
            PlayerView { seat_id:p.id, direction:p.direction, name:p.name.clone(), score:p.score,
                ready: self.phase == Phase::Setup && matches!(preview, Some(Preview::Confirmed(_))),
                opening_look_slots, bot:p.bot, cards }
        }).collect();
        let held = round.and_then(|round| match round.step {
            TurnStep::Incoming { card, from_deck } => Some((card, from_deck)),
            TurnStep::MatchFlipping => round
                .pending_match
                .as_ref()
                .and_then(|pending| pending.incoming),
            _ => None,
        });
        let incoming_source = held.map(|(_, from_deck)| if from_deck { "deck" } else { "discard" });
        let incoming = held.and_then(|(card, _)| {
            (card.face_up || round.is_some_and(|round| viewer == Some(round.turn)))
                .then(|| card.view(true))
        });
        let pending_power = round.and_then(|round| {
            let (power_id, phase, own_slot, target_slot) = round.step.power()?;
            let is_actor = viewer == Some(round.turn);
            Some(PendingPower {
                power_id,
                phase,
                kind: if phase.starts_with("peek") {
                    "peek"
                } else if phase.starts_with("spy") {
                    "spy"
                } else {
                    "swap"
                },
                actor_seat_id: self.players[round.turn].id,
                own_slot: own_slot.filter(|_| is_actor),
                target_slot: target_slot.filter(|_| is_actor || viewer.is_none()),
            })
        });
        let private_power_result = round
            .and_then(|round| match round.step {
                TurnStep::PeekViewing(id, card) if viewer == Some(round.turn) => {
                    Some(PowerResult {
                        result_id: id,
                        kind: "peek",
                        slot: Some(card.slot),
                        rank: Some(card.rank),
                        target_name: None,
                    })
                }
                TurnStep::SpyViewing(id, target, card) if viewer == Some(round.turn) => {
                    Some(PowerResult {
                        result_id: id,
                        kind: "spy",
                        slot: Some(card.slot),
                        rank: Some(card.rank),
                        target_name: Some(self.players[target].name.clone()),
                    })
                }
                _ => None,
            })
            .or_else(|| {
                let id = self.swap_receipts[viewer?]?;
                Some(PowerResult {
                    result_id: id,
                    kind: "swap",
                    slot: None,
                    rank: None,
                    target_name: None,
                })
            });
        let results = round
            .map(|r| {
                r.results
                    .iter()
                    .enumerate()
                    .map(|(i, &(hand_total, points))| RoundResult {
                        seat_id: self.players[i].id,
                        hand_total,
                        points,
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mut winners = vec![];
        if self.phase == Phase::Finished {
            let points = &round.unwrap().results;
            let best = self
                .players
                .iter()
                .enumerate()
                .map(|(i, p)| (p.score, points[i].1))
                .min()
                .unwrap();
            winners.extend(
                self.players
                    .iter()
                    .enumerate()
                    .filter(|(i, p)| (p.score, points[*i].1) == best)
                    .map(|(_, p)| p.id),
            );
        }
        Snapshot {
            protocol_version: 1,
            room_id: self.id.clone(),
            room_code: self.code.clone(),
            revision: self.revision,
            role: if viewer.is_some() { "player" } else { "board" },
            you_seat_id: viewer.map(|i| self.players[i].id),
            host_seat_id: self.players.first().map(|p| p.id),
            phase,
            round: if self.phase == Phase::Setup && self.players.iter().any(|p| p.score > 100) {
                1
            } else {
                self.round_number + u32::from(self.phase == Phase::Setup)
            },
            deal_id: round.map(|r| r.id),
            turn_seat_id: round
                .filter(|_| self.phase == Phase::Playing)
                .map(|r| self.players[r.turn].id),
            next_starter_seat_id: (self.phase == Phase::Results
                || (self.phase == Phase::Setup && !self.choosing_starter()))
            .then(|| self.players[self.starter].id),
            players,
            deck_count: round.map_or(0, |r| r.deck.len()),
            discard_top: round.and_then(|r| r.discard.last()).map(|c| c.rank),
            incoming,
            incoming_source,
            pending_power,
            private_power_result,
            match_flip: round
                .and_then(|r| r.pending_match.as_ref())
                .map(|pending| pending.flip.clone()),
            match_outcome: round
                .and_then(|round| round.match_outcome.as_ref())
                .filter(|(seat, outcome)| {
                    viewer == Some(*seat) && outcome.revision == self.revision
                })
                .map(|(_, outcome)| outcome.clone()),
            card_travel: round
                .and_then(|round| round.match_outcome.as_ref())
                .filter(|(seat, outcome)| {
                    outcome.source == "discardTop"
                        && self.players[*seat]
                            .hand
                            .iter()
                            .any(|card| card.slot == outcome.incoming_slot)
                })
                .map(|(seat, outcome)| CardTravel {
                    revision: outcome.revision,
                    slot: outcome.incoming_slot,
                    from: CardPile::Discard,
                    to_seat_id: self.players[*seat].id,
                }),
            test: self.test_view(),
            cabo_caller: round.and_then(|r| r.caller).map(|i| self.players[i].id),
            final_turns: round
                .map(|r| r.final_turns.iter().map(|&i| self.players[i].id).collect())
                .unwrap_or_default(),
            results,
            winners,
            available_actions: self.actions(role),
            message: self.message.clone(),
        }
    }
}
