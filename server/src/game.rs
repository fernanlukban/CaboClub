use std::{
    collections::{HashMap, HashSet, VecDeque},
    time::{Duration, Instant},
};

use cabo_shared::{
    game::{self, MAX_PLAYERS, MIN_PLAYERS, STARTING_HAND_SIZE, cabo_power::Power},
    protocol::{Action, CardView, Choice, Command, PlayerView, RoundResult, Snapshot},
    round_start::{Phase as SetupPhase, Preview, RoundStart},
};
use rand::seq::SliceRandom;
use uuid::Uuid;

#[derive(Clone, Copy)]
struct Card {
    slot: u32,
    rank: u8,
}

impl Card {
    fn view(self, visible: bool) -> CardView {
        CardView {
            slot: self.slot,
            rank: visible.then_some(self.rank),
        }
    }
}

struct Player {
    id: u64,
    token: String,
    name: String,
    hand: Vec<Card>,
    score: u16,
    reset: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Seat(usize),
    CreatorBoard,
    Board,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Lobby,
    Setup,
    Playing,
    Results,
    Finished,
}

struct Round {
    id: u64,
    opening: RoundStart,
    deck: Vec<Card>,
    discard: Vec<Card>,
    turn: usize,
    step: TurnStep,
    caller: Option<usize>,
    final_turns: VecDeque<usize>,
    results: Vec<(u16, u16)>,
    private_reveal: Option<(usize, Card)>,
}

#[derive(Clone, Copy)]
enum TurnStep {
    Awaiting,
    Incoming { card: Card, from_deck: bool },
    Power(Power),
}

pub struct Room {
    code: String,
    id: String,
    players: Vec<Player>,
    creator: Option<String>,
    board: Option<String>,
    pairing: Option<(String, Instant)>,
    revision: u64,
    phase: Phase,
    round_number: u32,
    starter: usize,
    next_deal: u64,
    round: Option<Round>,
    cancel_backup: Option<(Round, Vec<Vec<Card>>)>,
    message: String,
    replay: HashMap<(String, String), (Command, String)>,
    replay_order: VecDeque<(String, String)>,
    pub last_active: Instant,
}

impl Room {
    pub fn new(code: String, on_board: bool) -> Self {
        Self {
            code,
            id: secret(),
            players: vec![],
            creator: on_board.then(secret),
            board: None,
            pairing: None,
            revision: 0,
            phase: Phase::Lobby,
            round_number: 0,
            starter: 0,
            next_deal: 1,
            round: None,
            cancel_backup: None,
            last_active: Instant::now(),
            message: String::new(),
            replay: HashMap::new(),
            replay_order: VecDeque::new(),
        }
    }

    pub fn creator_token(&self) -> Option<&str> {
        self.creator.as_deref()
    }

    pub fn join(&mut self, name: &str) -> Result<String, &'static str> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 22 {
            return Err("invalidName");
        }
        if self.phase != Phase::Lobby || self.players.len() >= MAX_PLAYERS {
            return Err("roomUnavailable");
        }
        let token = secret();
        self.players.push(Player {
            id: self.players.len() as u64 + 1,
            token: token.clone(),
            name: name.into(),
            hand: vec![],
            score: 0,
            reset: false,
        });
        self.changed("A player joined the table.");
        Ok(token)
    }

    pub fn pair(&mut self, code: &str) -> Result<String, &'static str> {
        if !self
            .pairing
            .as_ref()
            .is_some_and(|(key, expiry)| key == code && *expiry > Instant::now())
        {
            return Err("invalidPairing");
        }
        self.pairing = None;
        let token = secret();
        self.board = Some(token.clone());
        Ok(token)
    }

    pub fn role(&self, token: &str) -> Option<Role> {
        if self.creator.as_deref() == Some(token) {
            return Some(Role::CreatorBoard);
        }
        if self.board.as_deref() == Some(token) {
            return Some(Role::Board);
        }
        self.players
            .iter()
            .position(|p| p.token == token)
            .map(Role::Seat)
    }

    fn changed(&mut self, message: &str) {
        self.revision += 1;
        self.last_active = Instant::now();
        self.message = message.into();
    }

    pub fn actions(&self, role: Role) -> Vec<Choice> {
        use Action::*;
        let mut choices = vec![];
        let host = role == Role::Seat(0);
        if host {
            choices.push(Choice::button(PairBoard));
            if self.board.is_some() || self.creator.is_some() {
                choices.push(Choice::button(RevokeBoard));
            }
        }
        if (host || role == Role::CreatorBoard) && self.players.len() >= MIN_PLAYERS {
            match self.phase {
                Phase::Lobby => choices.push(Choice::button(DealRound)),
                Phase::Results => choices.push(Choice::button(DealNextRound)),
                Phase::Finished => choices.push(Choice::button(DealNewGame)),
                _ => {}
            }
        }
        let Some(round) = &self.round else {
            return choices;
        };
        if self.phase == Phase::Setup {
            if host {
                choices.push(Choice::button(CancelSetup));
            }
            if round.opening.phase() == SetupPhase::ReadyToStart
                && matches!(role, Role::Board | Role::CreatorBoard)
            {
                choices.push(Choice::button(BeginPlay));
            }
            if let Role::Seat(index) = role {
                let player = &self.players[index];
                match round.opening.preview(player.id) {
                    Some(Preview::Hidden) => {
                        choices.push(Choice::select(RevealInitialCards, slots(player), 2, 2))
                    }
                    Some(Preview::Revealed(_)) => choices.push(Choice::button(ConfirmInitialLook)),
                    _ => {}
                }
            }
            return choices;
        }
        let Role::Seat(index) = role else {
            return choices;
        };
        if self.phase != Phase::Playing || round.turn != index {
            return choices;
        }
        let hand = &self.players[index];
        let opposing = || {
            self.players
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != index)
                .flat_map(|(_, p)| slots(p))
                .collect::<Vec<_>>()
        };
        let match_cards = |choices: &mut Vec<Choice>| {
            if hand.hand.len() >= 2 {
                let mut choice = Choice::select(CallMatch, slots(hand), 2, hand.hand.len());
                choice.choose_side = true;
                choices.push(choice);
            }
        };
        match round.step {
            TurnStep::Awaiting => {
                let draw = game::DrawContext {
                    round_active: true,
                    is_current_player: true,
                    move_in_progress: false,
                    deck_has_cards: !round.deck.is_empty(),
                    discard_has_cards: !round.discard.is_empty(),
                };
                if draw.validate(game::DrawSource::Deck).is_ok() {
                    choices.push(Choice::button(DrawDeck));
                }
                if draw.validate(game::DrawSource::Discard).is_ok() {
                    choices.push(Choice::button(DrawDiscard));
                    match_cards(&mut choices);
                }
                if round.caller.is_none() {
                    choices.push(Choice::button(CallCabo));
                }
            }
            TurnStep::Incoming { card, from_deck } => {
                choices.push(Choice::select(DiscardHandCard, slots(hand), 1, 1));
                if from_deck {
                    choices.push(Choice::button(DiscardDrawn));
                    if game::card_power(card.rank).is_some() {
                        choices.push(Choice::button(UseDrawnPower));
                    }
                }
                match_cards(&mut choices);
            }
            TurnStep::Power(Power::Peek) => choices.push(Choice::select(Peek, slots(hand), 1, 1)),
            TurnStep::Power(Power::Spy) => choices.push(Choice::select(Spy, opposing(), 1, 1)),
            TurnStep::Power(Power::Swap) => {
                let mut choice = Choice::select(Swap, slots(hand), 2, 2);
                choice.targets = opposing();
                choices.push(choice);
            }
        }
        choices
    }

    pub fn command(
        &mut self,
        token: &str,
        role: Role,
        command: Command,
    ) -> Result<String, &'static str> {
        if command.command_id.is_empty() || command.command_id.len() > 80 {
            return Err("invalidCommandId");
        }
        let key = (token.to_owned(), command.command_id.clone());
        if let Some((previous, outcome)) = self.replay.get(&key) {
            return if previous == &command {
                Ok(outcome.clone())
            } else {
                Err("commandIdReused")
            };
        }
        if self.revision != command.expected_revision {
            return Err("staleRevision");
        }
        if matches!(
            command.action,
            Action::RevealInitialCards
                | Action::ConfirmInitialLook
                | Action::BeginPlay
                | Action::CancelSetup
        ) && self.round.as_ref().map(|round| round.id) != command.deal_id
        {
            return Err("staleDeal");
        }
        let choice = self
            .actions(role)
            .into_iter()
            .find(|choice| choice.action == command.action)
            .ok_or("actionUnavailable")?;
        let unique: HashSet<_> = command.slots.iter().collect();
        if !(choice.min..=choice.max).contains(&command.slots.len())
            || unique.len() != command.slots.len()
        {
            return Err("invalidSelection");
        }
        let valid_slots = if command.action == Action::Swap {
            choice.slots.contains(&command.slots[0]) && choice.targets.contains(&command.slots[1])
        } else {
            command.slots.iter().all(|id| choice.slots.contains(id))
        };
        if !valid_slots {
            return Err("invalidSelection");
        }

        let outcome = self.apply(role, &command)?;
        self.replay.insert(key.clone(), (command, outcome.clone()));
        self.replay_order.push_back(key);
        if self.replay_order.len() > 512 {
            self.replay.remove(&self.replay_order.pop_front().unwrap());
        }
        Ok(outcome)
    }

    fn apply(&mut self, role: Role, command: &Command) -> Result<String, &'static str> {
        use Action::*;
        let seat = match role {
            Role::Seat(index) => index,
            _ => 0,
        };
        let message = match command.action {
            PairBoard => {
                let code = secret()[..10].to_uppercase();
                self.pairing = Some((code.clone(), Instant::now() + Duration::from_secs(300)));
                self.changed("A board pairing code was created.");
                return Ok(code);
            }
            RevokeBoard => {
                self.creator = None;
                self.board = None;
                self.pairing = None;
                "Board access revoked."
            }
            DealRound | DealNextRound | DealNewGame => {
                self.deal();
                "Choose two cards to look at."
            }
            CancelSetup => {
                self.phase = if self.round_number == 0 {
                    Phase::Lobby
                } else if self.round_number > 0 && self.players.iter().any(|p| p.score > 100) {
                    Phase::Finished
                } else {
                    Phase::Results
                };
                if self.phase == Phase::Lobby {
                    self.round = None;
                    for player in &mut self.players {
                        player.hand.clear();
                    }
                }
                if let Some(backup) = self.cancel_backup.take() {
                    self.round = Some(backup.0);
                    for (player, hand) in self.players.iter_mut().zip(backup.1) {
                        player.hand = hand;
                    }
                }
                "The deal was canceled."
            }
            RevealInitialCards => {
                let player = &self.players[seat];
                let positions: Vec<_> = command
                    .slots
                    .iter()
                    .map(|id| player.hand.iter().position(|c| c.slot == *id).unwrap())
                    .collect();
                self.round
                    .as_mut()
                    .unwrap()
                    .opening
                    .reveal(player.id, [positions[0], positions[1]])
                    .map_err(|_| "invalidPreview")?;
                "A player looked at their cards."
            }
            ConfirmInitialLook => {
                self.round
                    .as_mut()
                    .unwrap()
                    .opening
                    .confirm(self.players[seat].id)
                    .map_err(|_| "invalidPreview")?;
                "A player is ready."
            }
            BeginPlay => {
                let round = self.round.as_mut().unwrap();
                round.opening.begin_play().map_err(|_| "notReady")?;
                if self.round_number == 0 || self.players.iter().any(|p| p.score > 100) {
                    self.round_number = 0;
                    self.starter = 0;
                    for p in &mut self.players {
                        p.score = 0;
                        p.reset = false;
                    }
                }
                self.round_number += 1;
                self.cancel_backup = None;
                self.phase = Phase::Playing;
                "The round has started."
            }
            _ => self.play(seat, command),
        };
        self.changed(message);
        Ok(message.into())
    }

    fn deal(&mut self) {
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
                Card { slot, rank }
            })
            .collect();
        deck.shuffle(&mut rand::rng());
        for p in &mut self.players {
            p.hand.clear();
        }
        for _ in 0..STARTING_HAND_SIZE {
            for player in &mut self.players {
                player.hand.push(deck.pop().unwrap());
            }
        }
        let discard = vec![deck.pop().unwrap()];
        let turn = if self.phase == Phase::Finished {
            0
        } else {
            self.starter
        };
        self.round = Some(Round {
            id: self.next_deal,
            opening: RoundStart::new(&self.players.iter().map(|p| p.id).collect::<Vec<_>>())
                .unwrap(),
            deck,
            discard,
            turn,
            step: TurnStep::Awaiting,
            caller: None,
            final_turns: VecDeque::new(),
            results: vec![],
            private_reveal: None,
        });
        self.next_deal += 1;
        self.phase = Phase::Setup;
    }

    fn play(&mut self, seat: usize, command: &Command) -> &'static str {
        use Action::*;
        let round = self.round.as_mut().unwrap();
        let incoming = match round.step {
            TurnStep::Incoming { card, .. } => Some(card),
            _ => None,
        };
        if round.private_reveal.is_some_and(|(owner, _)| owner == seat) {
            round.private_reveal = None;
        }
        let note = match command.action {
            DrawDeck | DrawDiscard => {
                let from_deck = command.action == DrawDeck;
                let pile = if from_deck {
                    &mut round.deck
                } else {
                    &mut round.discard
                };
                round.step = TurnStep::Incoming {
                    card: pile.pop().unwrap(),
                    from_deck,
                };
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
                    .push(std::mem::replace(&mut hand[index], incoming.unwrap()));
                "A player traded a card."
            }
            DiscardDrawn => {
                round.discard.push(incoming.unwrap());
                "A player discarded their draw."
            }
            CallMatch => {
                let card = incoming.unwrap_or_else(|| round.discard.pop().unwrap());
                let hand = &mut self.players[seat].hand;
                let selected: Vec<_> = hand
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| command.slots.contains(&c.slot))
                    .map(|(index, c)| (index, *c))
                    .collect();
                if game::cards_match(&selected.iter().map(|(_, c)| c.rank).collect::<Vec<_>>()) {
                    for (index, _) in selected.iter().rev() {
                        hand.remove(*index);
                    }
                    round.discard.extend(selected.iter().map(|(_, c)| *c));
                    hand.insert(selected[0].0, card);
                    "The match was correct."
                } else {
                    if command.insert_left {
                        hand.insert(0, card);
                    } else {
                        hand.push(card);
                    }
                    if selected.len() >= 3 {
                        if let Some(penalty) = round.deck.pop() {
                            hand.push(penalty);
                        }
                    }
                    "The match missed. A penalty was added."
                }
            }
            CallCabo => {
                round.caller = Some(seat);
                round.final_turns = (1..self.players.len())
                    .map(|offset| (seat + offset) % self.players.len())
                    .collect();
                "Cabo called. Everyone else gets one final turn."
            }
            UseDrawnPower => {
                let card = incoming.unwrap();
                round.discard.push(card);
                round.step = TurnStep::Power(game::card_power(card.rank).unwrap());
                return "A player activated a power.";
            }
            Peek | Spy => {
                let card = *self
                    .players
                    .iter()
                    .flat_map(|p| &p.hand)
                    .find(|c| c.slot == command.slots[0])
                    .unwrap();
                round.private_reveal = Some((seat, card));
                "A player looked at a card."
            }
            Swap => {
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
        round.step = TurnStep::Awaiting;
        if round.final_turns.front() == Some(&seat) {
            round.final_turns.pop_front();
        }
        if round.deck.is_empty() || (round.caller.is_some() && round.final_turns.is_empty()) {
            self.score();
            return "The round is complete.";
        }
        round.turn = round
            .final_turns
            .front()
            .copied()
            .unwrap_or((seat + 1) % self.players.len());
        note
    }

    fn score(&mut self) {
        let round = self.round.as_mut().unwrap();
        let hands: Vec<_> = self
            .players
            .iter()
            .map(|p| p.hand.iter().map(|c| c.rank).collect())
            .collect();
        round.results = game::round_points(&hands, round.caller);
        round.private_reveal = None;
        for (p, &(_, points)) in self.players.iter_mut().zip(&round.results) {
            p.score += points;
            if p.score == 100 && !p.reset {
                p.score = 50;
                p.reset = true;
            }
        }
        let previous = self.starter;
        self.starter = (1..=self.players.len())
            .map(|offset| (previous + offset) % self.players.len())
            .min_by_key(|&i| round.results[i].1)
            .unwrap();
        self.phase = if self.players.iter().any(|p| p.score > 100) {
            Phase::Finished
        } else {
            Phase::Results
        };
    }

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
                let power = round.and_then(|r| r.private_reveal).is_some_and(|(owner, card)| viewer == Some(owner) && c.slot == card.slot);
                c.view(public || opening || power)
            }).collect();
            PlayerView { seat_id:p.id, name:p.name.clone(), score:p.score,
                ready: self.phase == Phase::Setup && preview == Some(Preview::Confirmed), cards }
        }).collect();
        let incoming = round.and_then(|r| {
            if viewer != Some(r.turn) {
                return None;
            }
            match r.step {
                TurnStep::Incoming { card, .. } => Some(card.view(true)),
                _ => None,
            }
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
            players,
            deck_count: round.map_or(0, |r| r.deck.len()),
            discard_top: round.and_then(|r| r.discard.last()).map(|c| c.rank),
            incoming,
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

fn slots(player: &Player) -> Vec<u32> {
    player.hand.iter().map(|card| card.slot).collect()
}

pub fn secret() -> String {
    Uuid::new_v4().simple().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(room: &Room, id: &str, action: Action, slots: Vec<u32>) -> Command {
        Command {
            command_id: id.into(),
            expected_revision: room.revision,
            action,
            slots,
            insert_left: false,
            deal_id: room.round.as_ref().map(|round| round.id),
        }
    }

    fn run(room: &mut Room, role: Role, action: Action, slots: Vec<u32>) {
        let token = match role {
            Role::Seat(index) => room.players[index].token.clone(),
            Role::CreatorBoard => room.creator.clone().unwrap(),
            Role::Board => room.board.clone().unwrap(),
        };
        let cmd = command(room, &format!("revision-{}", room.revision), action, slots);
        room.command(&token, role, cmd).unwrap();
    }

    fn setup() -> Room {
        let mut room = Room::new("TESTROOM".into(), true);
        room.join("Alex").unwrap();
        room.join("Bea").unwrap();
        run(&mut room, Role::CreatorBoard, Action::DealRound, vec![]);
        room
    }

    fn start(room: &mut Room) {
        for index in 0..room.players.len() {
            let pair = slots(&room.players[index])[..2].to_vec();
            run(room, Role::Seat(index), Action::RevealInitialCards, pair);
            run(room, Role::Seat(index), Action::ConfirmInitialLook, vec![]);
        }
        let host = room.players[0].token.clone();
        let phone_start = command(room, "phone-start", Action::BeginPlay, vec![]);
        assert_eq!(
            room.command(&host, Role::Seat(0), phone_start),
            Err("actionUnavailable")
        );
        run(room, Role::CreatorBoard, Action::BeginPlay, vec![]);
        assert!(room.actions(Role::CreatorBoard).is_empty());
    }

    #[test]
    fn board_cannot_start_early_and_confirmed_ranks_stay_hidden_on_replay() {
        let mut room = setup();
        let board = room.creator.clone().unwrap();
        let start = command(&room, "early", Action::BeginPlay, vec![]);
        assert_eq!(
            room.command(&board, Role::CreatorBoard, start),
            Err("actionUnavailable")
        );
        let player = room.players[0].token.clone();
        let pair = slots(&room.players[0])[..2].to_vec();
        let reveal = command(&room, "reveal", Action::RevealInitialCards, pair);
        room.command(&player, Role::Seat(0), reveal.clone())
            .unwrap();
        assert_eq!(
            room.snapshot(Role::Seat(0)).players[0]
                .cards
                .iter()
                .filter(|card| card.rank.is_some())
                .count(),
            2
        );
        for role in [Role::CreatorBoard, Role::Seat(1)] {
            assert!(
                room.snapshot(role).players[0]
                    .cards
                    .iter()
                    .all(|card| card.rank.is_none())
            );
        }
        run(&mut room, Role::Seat(0), Action::ConfirmInitialLook, vec![]);
        room.command(&player, Role::Seat(0), reveal).unwrap();
        assert!(
            room.snapshot(Role::Seat(0)).players[0]
                .cards
                .iter()
                .all(|card| card.rank.is_none())
        );
    }

    #[test]
    fn three_card_match_first_keeps_previous_discard_and_replays_once() {
        let mut room = setup();
        start(&mut room);
        let previous_discard = *room.round.as_ref().unwrap().discard.last().unwrap();
        for card in &mut room.players[0].hand[..3] {
            card.rank = 4;
        }
        let pair = slots(&room.players[0])[..3].to_vec();
        let token = room.players[0].token.clone();
        let match_cards = command(&room, "match", Action::CallMatch, pair);
        room.command(&token, Role::Seat(0), match_cards.clone())
            .unwrap();
        assert_eq!(room.players[0].hand.len(), 2);
        assert_eq!(room.players[0].hand[0].slot, previous_discard.slot);
        assert_eq!(room.round.as_ref().unwrap().discard.last().unwrap().rank, 4);
        assert_eq!(room.round.as_ref().unwrap().turn, 1);
        let revision = room.revision;
        room.command(&token, Role::Seat(0), match_cards).unwrap();
        assert_eq!(room.revision, revision);
    }

    #[test]
    fn cabo_scores_once_after_the_other_player_finishes() {
        let mut room = setup();
        start(&mut room);
        run(&mut room, Role::Seat(0), Action::CallCabo, vec![]);
        run(&mut room, Role::Seat(1), Action::DrawDeck, vec![]);
        let token = room.players[1].token.clone();
        let finish = command(&room, "finish", Action::DiscardDrawn, vec![]);
        room.command(&token, Role::Seat(1), finish.clone()).unwrap();
        assert_eq!(room.snapshot(Role::CreatorBoard).phase, "roundResults");
        let scores: Vec<_> = room.players.iter().map(|player| player.score).collect();
        room.command(&token, Role::Seat(1), finish).unwrap();
        assert_eq!(
            room.players
                .iter()
                .map(|player| player.score)
                .collect::<Vec<_>>(),
            scores
        );
    }
}
