use super::{Card, MATCH_FLIP_DURATION, PendingMatch, Phase, Room, Round, TurnStep};
use cabo_shared::{
    game::{self, STARTING_HAND_SIZE, cabo_power::Power},
    protocol::{Action, Command, MatchFlip, MatchOutcome, TestScenario},
    round_start::RoundStart,
};
use rand::seq::SliceRandom;
use std::{
    collections::{HashSet, VecDeque},
    time::Instant,
};

impl Room {
    pub(super) fn deal(&mut self) {
        let old = self.round.take();
        self.cancel_backup =
            old.map(|round| (round, self.players.iter().map(|p| p.hand.clone()).collect()));
        let mut ranks = vec![0, 0, 13, 13];
        for rank in 1..=12 {
            ranks.extend([rank; 4]);
        }
        let mut ids = HashSet::new();
        let mut deck: Vec<_> = ranks
            .into_iter()
            .map(|rank| {
                let slot = loop {
                    let id = rand::random();
                    if ids.insert(id) {
                        break id;
                    }
                };
                Card {
                    slot,
                    rank,
                    face_up: false,
                }
            })
            .collect();
        if let Some(test) = &mut self.test {
            deck.shuffle(&mut test.rng);
        } else {
            deck.shuffle(&mut rand::rng());
        }
        self.swap_receipts.fill(None);
        for p in &mut self.players {
            p.hand.clear();
        }
        for _ in 0..STARTING_HAND_SIZE {
            for player in &mut self.players {
                player.hand.push(deck.pop().unwrap());
            }
        }
        let discard = vec![deck.pop().unwrap().face_up()];
        if self.round_number == 0
            && let Some(test) = &self.test
        {
            let rank = match test.scenario {
                TestScenario::Spy => Some(9),
                TestScenario::Swap => Some(11),
                TestScenario::Random => None,
            };
            if let Some(rank) = rank {
                if let Some(index) = deck.iter().position(|card| card.rank == rank) {
                    let top = deck.len() - 1;
                    deck.swap(index, top);
                } else {
                    let replacement = deck.last_mut().unwrap();
                    let card = self
                        .players
                        .iter_mut()
                        .flat_map(|p| &mut p.hand)
                        .find(|card| card.rank == rank)
                        .unwrap();
                    std::mem::swap(replacement, card);
                }
            }
        }
        let turn = if self.phase == Phase::Finished {
            0
        } else {
            self.starter
        };
        self.round = Some(Round {
            id: self.next_deal,
            opening: RoundStart::new(&self.players.iter().map(|p| p.id).collect::<Vec<_>>())
                .unwrap(),
            opening_marks_visible: true,
            deck,
            discard,
            turn,
            step: TurnStep::Awaiting,
            caller: None,
            final_turns: VecDeque::new(),
            results: vec![],
            match_outcome: None,
            pending_match: None,
        });
        self.next_deal += 1;
        self.phase = Phase::Setup;
    }

    pub(super) fn play(&mut self, seat: usize, command: &Command) -> &'static str {
        use Action::*;
        let turn_order: Vec<_> = self
            .counterclockwise_seat_indexes(seat)
            .filter(|&index| index != seat)
            .collect();
        self.swap_receipts[seat] = None;
        let round = self.round.as_mut().unwrap();
        round.opening_marks_visible = false;
        let incoming = match round.step {
            TurnStep::Incoming { card, .. } => Some(card),
            _ => None,
        };
        let note = match command.action {
            DrawDeck | DrawDiscard => {
                let from_deck = command.action == DrawDeck;
                let pile = if from_deck {
                    &mut round.deck
                } else {
                    &mut round.discard
                };
                let card = pile.pop().unwrap();
                round.step = TurnStep::Incoming { card, from_deck };
                if !from_deck && round.discard.is_empty() {
                    round.discard.push(
                        round
                            .deck
                            .pop()
                            .expect("an open turn has a deck card")
                            .face_up(),
                    );
                }
                return "A player picked up a card.";
            }
            DiscardHandCard => {
                let hand = &mut self.players[seat].hand;
                let index = hand
                    .iter()
                    .position(|c| c.slot == command.slots[0])
                    .unwrap();
                round
                    .discard
                    .push(std::mem::replace(&mut hand[index], incoming.unwrap()).face_up());
                "A player traded a card."
            }
            DiscardDrawn => {
                round.discard.push(incoming.unwrap().face_up());
                "A player discarded their draw."
            }
            SkipTurn => {
                if let Some(card) = incoming {
                    round.discard.push(card.face_up());
                }
                "The host skipped the active player's turn."
            }
            CallMatch => {
                let mut newly_face_up_slots = Vec::new();
                for card in &mut self.players[seat].hand {
                    if command.slots.contains(&card.slot) && !card.face_up {
                        newly_face_up_slots.push(card.slot);
                        card.face_up = true;
                    }
                }
                round.pending_match = Some(PendingMatch {
                    flip: MatchFlip {
                        revision: self.revision + 1,
                        actor_seat_id: self.players[seat].id,
                        slots: command.slots.clone(),
                        newly_face_up_slots,
                    },
                    deadline: Instant::now() + MATCH_FLIP_DURATION,
                    incoming: match round.step {
                        TurnStep::Incoming { card, from_deck } => Some((card, from_deck)),
                        _ => None,
                    },
                    insert_left: command.insert_left,
                });
                round.step = TurnStep::MatchFlipping;
                return "A player is revealing a match.";
            }
            CallCabo => {
                round.caller = Some(seat);
                round.final_turns = turn_order.iter().copied().collect();
                "Cabo called. Everyone else gets one final turn."
            }
            UseDrawnPower => {
                let card = incoming.unwrap();
                round.discard.push(card.face_up());
                let id = self.next_power;
                self.next_power += 1;
                round.step = match game::card_power(card.rank).unwrap() {
                    Power::Peek => TurnStep::Peek(id),
                    Power::Spy => TurnStep::SpySelecting(id),
                    Power::Swap => TurnStep::SwapSelectingOwn(id),
                };
                return "A player activated a power.";
            }
            Peek => {
                let card = *self
                    .players
                    .iter()
                    .flat_map(|p| &p.hand)
                    .find(|c| c.slot == command.slots[0])
                    .unwrap();
                let id = round.step.power().unwrap().0;
                round.step = TurnStep::PeekViewing(id, card);
                return "A player is finishing a private look.";
            }
            SelectPowerOwnCard => {
                let id = round.step.power().unwrap().0;
                round.step = TurnStep::SwapSelectingOpponent(id, command.slots[0]);
                return "A player chose their card.";
            }
            StagePowerTarget => {
                let target = command.slots[0];
                round.step = match round.step {
                    TurnStep::SpySelecting(id) | TurnStep::SpyReview(id, _) => {
                        TurnStep::SpyReview(id, target)
                    }
                    TurnStep::SwapSelectingOpponent(id, own) | TurnStep::SwapReview(id, own, _) => {
                        TurnStep::SwapReview(id, own, target)
                    }
                    _ => unreachable!(),
                };
                return "An opponent card was selected.";
            }
            ConfirmSpy => {
                let (id, _, _, _) = round.step.power().unwrap();
                let (owner, card) = self
                    .players
                    .iter()
                    .enumerate()
                    .find_map(|(i, player)| {
                        player
                            .hand
                            .iter()
                            .find(|c| c.slot == command.slots[0])
                            .map(|card| (i, *card))
                    })
                    .unwrap();
                round.step = TurnStep::SpyViewing(id, owner, card);
                return "A player is finishing a private look.";
            }
            AcknowledgePeek | AcknowledgeSpy => "A player finished looking.",
            ConfirmSwap => {
                self.swap_receipts[seat] = Some(round.step.power().unwrap().0);
                let own = self.players[seat]
                    .hand
                    .iter()
                    .position(|c| c.slot == command.slots[0])
                    .unwrap();
                let (other, target) = self
                    .players
                    .iter()
                    .enumerate()
                    .find_map(|(i, p)| {
                        p.hand
                            .iter()
                            .position(|c| c.slot == command.slots[1])
                            .map(|j| (i, j))
                    })
                    .unwrap();
                let card = self.players[seat].hand[own];
                let replacement = std::mem::replace(&mut self.players[other].hand[target], card);
                self.players[seat].hand[own] = replacement;
                "A player swapped two cards."
            }
            _ => unreachable!(),
        };
        if self.finish_turn(seat) {
            "The round is complete."
        } else {
            note
        }
    }

    pub(super) fn resolve_match(&mut self) -> &'static str {
        let round = self.round.as_mut().unwrap();
        let pending = round.pending_match.take().unwrap();
        let seat = round.turn;
        let source = match pending.incoming {
            Some((_, true)) => "deck",
            Some((_, false)) => "discardPickup",
            _ => "discardTop",
        };
        let card = pending
            .incoming
            .map(|(card, _)| card)
            .unwrap_or_else(|| round.discard.pop().unwrap());
        let hand = &mut self.players[seat].hand;
        let selected: Vec<_> = hand
            .iter()
            .enumerate()
            .filter(|(_, c)| pending.flip.slots.contains(&c.slot))
            .map(|(index, c)| (index, *c))
            .collect();
        let ranks: Vec<_> = selected.iter().map(|(_, card)| card.rank).collect();
        let mut outcome = MatchOutcome {
            revision: self.revision + 1,
            matched: game::cards_match(&ranks),
            source,
            incoming_slot: card.slot,
            placement: if pending.insert_left { "left" } else { "right" },
            discarded_slots: vec![],
            penalty_slot: None,
        };
        let message = if outcome.matched {
            outcome.placement = "firstSelected";
            outcome.discarded_slots = selected.iter().map(|(_, card)| card.slot).collect();
            for (index, _) in selected.iter().rev() {
                hand.remove(*index);
            }
            round.discard.extend(selected.iter().map(|(_, c)| *c));
            hand.insert(selected[0].0, card);
            "The match was correct."
        } else {
            if pending.insert_left {
                hand.insert(0, card);
            } else {
                hand.push(card);
            }
            if round.discard.is_empty() {
                round.discard.push(
                    round
                        .deck
                        .pop()
                        .expect("an open turn has a deck card")
                        .face_up(),
                );
            }
            if selected.len() >= 3 {
                if let Some(penalty) = round.deck.pop() {
                    outcome.penalty_slot = Some(penalty.slot);
                    hand.push(penalty);
                }
            }
            if outcome.penalty_slot.is_some() {
                "No match. Incoming card kept and one penalty card added."
            } else {
                "No match. Incoming card kept."
            }
        };
        round.match_outcome = Some((seat, outcome));
        self.finish_turn(seat);
        message
    }

    pub(super) fn finish_turn(&mut self, seat: usize) -> bool {
        let next_seat = self
            .counterclockwise_seat_indexes(seat)
            .find(|&index| index != seat)
            .unwrap();
        let round = self.round.as_mut().unwrap();
        round.step = TurnStep::Awaiting;
        if round.final_turns.front() == Some(&seat) {
            round.final_turns.pop_front();
        }
        if round.deck.is_empty() || (round.caller.is_some() && round.final_turns.is_empty()) {
            self.score();
            return true;
        }
        round.turn = round.final_turns.front().copied().unwrap_or(next_seat);
        false
    }

    fn score(&mut self) {
        let order: Vec<_> = self.counterclockwise_seat_indexes(self.starter).collect();
        let round = self.round.as_mut().unwrap();
        let hands: Vec<_> = self
            .players
            .iter()
            .map(|p| p.hand.iter().map(|c| c.rank).collect())
            .collect();
        round.results = game::round_points(&hands, round.caller);
        for (p, &(_, points)) in self.players.iter_mut().zip(&round.results) {
            p.score += points;
            if p.score == 100 && !p.reset {
                p.score = 50;
                p.reset = true;
            }
        }
        self.starter = order
            .into_iter()
            .min_by_key(|&i| round.results[i].1)
            .unwrap();
        self.phase = if self.players.iter().any(|p| p.score > 100) {
            Phase::Finished
        } else {
            Phase::Results
        };
    }
}
