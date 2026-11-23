mod turn;
mod view;

use std::{
    collections::{HashMap, HashSet, VecDeque},
    time::{Duration, Instant},
};

use cabo_shared::{
    game::{self, MAX_PLAYERS, MIN_PLAYERS},
    protocol::{
        Action, CardView, Choice, Command, Direction, MatchFlip, MatchOutcome, TestScenario,
        TestView,
    },
    round_start::{Phase as SetupPhase, Preview, RoundStart},
};
use rand::{Rng, SeedableRng, rngs::StdRng, seq::SliceRandom};
use serde::Serialize;
use uuid::Uuid;

const MATCH_FLIP_DURATION: Duration = Duration::from_millis(2600);

#[derive(Clone, Copy)]
struct Card {
    slot: u32,
    rank: u8,
    face_up: bool,
}

impl Card {
    fn face_up(mut self) -> Self {
        self.face_up = true;
        self
    }

    fn view(self, visible: bool) -> CardView {
        CardView {
            slot: self.slot,
            rank: (self.face_up || visible).then_some(self.rank),
            face_up: self.face_up,
        }
    }
}

struct Player {
    id: u64,
    direction: Direction,
    token: String,
    name: String,
    hand: Vec<Card>,
    score: u16,
    reset: bool,
    bot: bool,
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
    opening_marks_visible: bool,
    deck: Vec<Card>,
    discard: Vec<Card>,
    turn: usize,
    step: TurnStep,
    caller: Option<usize>,
    final_turns: VecDeque<usize>,
    results: Vec<(u16, u16)>,
    match_outcome: Option<(usize, MatchOutcome)>,
    pending_match: Option<PendingMatch>,
}

struct PendingMatch {
    flip: MatchFlip,
    deadline: Instant,
    incoming: Option<(Card, bool)>,
    insert_left: bool,
}

#[derive(Clone, Copy)]
enum TurnStep {
    Awaiting,
    Incoming { card: Card, from_deck: bool },
    MatchFlipping,
    Peek(u64),
    PeekViewing(u64, Card),
    SpySelecting(u64),
    SpyReview(u64, u32),
    SpyViewing(u64, usize, Card),
    SwapSelectingOwn(u64),
    SwapSelectingOpponent(u64, u32),
    SwapReview(u64, u32, u32),
}

impl TurnStep {
    fn power(self) -> Option<(u64, &'static str, Option<u32>, Option<u32>)> {
        Some(match self {
            Self::Peek(id) => (id, "peekSelecting", None, None),
            Self::PeekViewing(id, _) => (id, "peekViewing", None, None),
            Self::SpySelecting(id) => (id, "spySelecting", None, None),
            Self::SpyReview(id, target) => (id, "spyReview", None, Some(target)),
            Self::SpyViewing(id, _, card) => (id, "spyViewing", None, Some(card.slot)),
            Self::SwapSelectingOwn(id) => (id, "swapSelectingOwn", None, None),
            Self::SwapSelectingOpponent(id, own) => (id, "swapSelectingOpponent", Some(own), None),
            Self::SwapReview(id, own, target) => (id, "swapReview", Some(own), Some(target)),
            _ => return None,
        })
    }

    fn accepts_target(self) -> bool {
        matches!(
            self,
            Self::SpySelecting(_)
                | Self::SpyReview(..)
                | Self::SwapSelectingOpponent(..)
                | Self::SwapReview(..)
        )
    }
}

struct TestRoom {
    scenario: TestScenario,
    seed: u64,
    generation: u64,
    rng: StdRng,
    next_bot_move: Instant,
}

const LOG_LIMIT: usize = 1024;

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogState {
    revision: u64,
    phase: &'static str,
    deal_id: Option<u64>,
    turn_seat: Option<u64>,
    turn_step: Option<&'static str>,
    deck_count: usize,
    discard_count: usize,
    discard_top: Option<u8>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    sequence: u64,
    event: &'static str,
    action: Option<Action>,
    seat: Option<u64>,
    board: bool,
    slot_count: usize,
    result: &'static str,
    before: LogState,
    after: LogState,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameLog<'a> {
    version: u8,
    truncated: bool,
    test: Option<TestView>,
    entries: &'a VecDeque<LogEntry>,
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
    next_power: u64,
    swap_receipts: [Option<u64>; MAX_PLAYERS],
    test: Option<TestRoom>,
    round: Option<Round>,
    cancel_backup: Option<(Round, Vec<Vec<Card>>)>,
    message: String,
    replay: HashMap<(String, String), (Command, String)>,
    replay_order: VecDeque<(String, String)>,
    log: VecDeque<LogEntry>,
    next_log_sequence: u64,
    last_active: Instant,
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
            next_power: 1,
            swap_receipts: [None; MAX_PLAYERS],
            test: None,
            round: None,
            cancel_backup: None,
            last_active: Instant::now(),
            message: String::new(),
            replay: HashMap::new(),
            replay_order: VecDeque::new(),
            log: VecDeque::new(),
            next_log_sequence: 1,
        }
    }

    pub fn game_log(&self) -> GameLog<'_> {
        GameLog {
            version: 1,
            truncated: self.next_log_sequence > LOG_LIMIT as u64 + 1,
            test: self.test_view(),
            entries: &self.log,
        }
    }

    fn test_view(&self) -> Option<TestView> {
        self.test.as_ref().map(|test| TestView {
            scenario: test.scenario,
            seed: test.seed.to_string(),
            generation: test.generation,
        })
    }

    fn log_state(&self) -> LogState {
        let round = self.round.as_ref();
        LogState {
            revision: self.revision,
            phase: match self.phase {
                Phase::Lobby => "lobby",
                Phase::Setup => "setup",
                Phase::Playing => "playing",
                Phase::Results => "roundResults",
                Phase::Finished => "gameOver",
            },
            deal_id: round.map(|round| round.id),
            turn_seat: round
                .filter(|_| self.phase == Phase::Playing)
                .map(|round| self.players[round.turn].id),
            turn_step: round.filter(|_| self.phase == Phase::Playing).map(|round| {
                match round.step {
                    TurnStep::Awaiting => "awaiting",
                    TurnStep::MatchFlipping => "matchFlipping",
                    TurnStep::Incoming {
                        from_deck: true, ..
                    } => "incomingDeck",
                    TurnStep::Incoming {
                        from_deck: false, ..
                    } => "incomingDiscard",
                    step => step.power().unwrap().1,
                }
            }),
            deck_count: round.map_or(0, |round| round.deck.len()),
            discard_count: round.map_or(0, |round| round.discard.len()),
            discard_top: round
                .and_then(|round| round.discard.last())
                .map(|card| card.rank),
        }
    }

    fn record(&mut self, mut entry: LogEntry) {
        entry.sequence = self.next_log_sequence;
        self.next_log_sequence += 1;
        if self.log.len() == LOG_LIMIT {
            self.log.pop_front();
        }
        self.log.push_back(entry);
    }

    pub fn enable_test(&mut self, scenario: TestScenario, seed: u64) {
        self.test = Some(TestRoom {
            scenario,
            seed,
            generation: 1,
            rng: StdRng::seed_from_u64(seed),
            next_bot_move: Instant::now(),
        });
    }

    pub fn tick(&mut self, now: Instant) {
        if self
            .round
            .as_ref()
            .and_then(|round| round.pending_match.as_ref())
            .is_some_and(|pending| now >= pending.deadline)
        {
            let before = self.log_state();
            let note = self.resolve_match();
            self.changed(note);
            self.record(LogEntry {
                sequence: 0,
                event: "matchResolved",
                action: None,
                seat: before.turn_seat,
                board: false,
                slot_count: 0,
                result: "accepted",
                before,
                after: self.log_state(),
            });
            return;
        }
        self.tick_bot(now);
    }

    fn tick_bot(&mut self, now: Instant) {
        if !self
            .test
            .as_ref()
            .is_some_and(|test| now >= test.next_bot_move)
        {
            return;
        }
        let Some(round) = &self.round else { return };
        let actor = match self.phase {
            Phase::Setup => (1..self.players.len()).find(|&i| {
                self.players[i].bot
                    && !matches!(
                        round.opening.preview(self.players[i].id),
                        Some(Preview::Confirmed(_))
                    )
            }),
            Phase::Playing if self.players[round.turn].bot => Some(round.turn),
            _ => None,
        };
        let Some(actor) = actor else { return };
        let role = Role::Seat(actor);
        let mut choices = self.actions(role);
        if choices.is_empty() {
            return;
        }
        let test = self.test.as_mut().unwrap();
        let choice = choices.swap_remove(test.rng.random_range(0..choices.len()));
        let mut slots = choice.slots;
        if !matches!(choice.action, Action::ConfirmSpy | Action::ConfirmSwap) {
            let count = test.rng.random_range(choice.min..=choice.max);
            slots.shuffle(&mut test.rng);
            slots.truncate(count);
        }
        let token = self.players[actor].token.clone();
        let command = Command {
            command_id: format!("bot-{}", self.revision),
            expected_revision: self.revision,
            deal_id: Some(round.id),
            power_id: round.step.power().map(|p| p.0),
            test_generation: None,
            action: choice.action,
            slots,
            insert_left: test.rng.random(),
        };
        if self.command(&token, role, command).is_err() {
            self.test.as_mut().unwrap().next_bot_move = now + Duration::from_millis(700);
        }
    }

    pub fn creator_token(&self) -> Option<&str> {
        self.creator.as_deref()
    }

    pub fn join(
        &mut self,
        name: &str,
        requested: Option<Direction>,
    ) -> Result<String, &'static str> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 22 {
            return Err("invalidName");
        }
        if self.phase != Phase::Lobby || self.players.len() >= MAX_PLAYERS {
            return Err("roomUnavailable");
        }
        let mut open = Direction::DEFAULT_SEATS.into_iter().filter(|direction| {
            !self
                .players
                .iter()
                .any(|player| player.direction == *direction)
        });
        let direction = match requested {
            Some(direction) if open.any(|available| available == direction) => direction,
            Some(_) => return Err("seatTaken"),
            None => open.next().ok_or("roomUnavailable")?,
        };
        let before = self.log_state();
        let token = secret();
        let seat = self.players.len() as u64 + 1;
        self.players.push(Player {
            id: self.players.len() as u64 + 1,
            direction,
            token: token.clone(),
            name: name.into(),
            hand: vec![],
            score: 0,
            reset: false,
            bot: false,
        });
        if self.test.is_some() {
            for (index, direction) in Direction::DEFAULT_SEATS
                .into_iter()
                .filter(|&candidate| candidate != direction)
                .enumerate()
            {
                let index = index + 1;
                self.players.push(Player {
                    id: index as u64 + 1,
                    direction,
                    token: secret(),
                    name: format!("Bot {index}"),
                    hand: vec![],
                    score: 0,
                    reset: false,
                    bot: true,
                });
            }
        }
        self.changed("A player joined the table.");
        self.record(LogEntry {
            sequence: 0,
            event: "join",
            action: None,
            seat: Some(seat),
            board: false,
            slot_count: 0,
            result: "accepted",
            before,
            after: self.log_state(),
        });
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

    pub fn last_active(&self) -> Instant {
        self.last_active
    }

    pub fn touch(&mut self) {
        self.last_active = Instant::now();
    }

    fn changed(&mut self, message: &str) {
        self.revision += 1;
        self.touch();
        self.message = message.into();
        if let Some(test) = &mut self.test {
            test.next_bot_move = Instant::now() + Duration::from_millis(680);
        }
    }

    fn choosing_starter(&self) -> bool {
        self.round_number == 0
            || self.phase == Phase::Finished
            || self.players.iter().any(|player| player.score > 100)
    }

    fn counterclockwise_seat_indexes(&self, from: usize) -> impl Iterator<Item = usize> + '_ {
        self.players[from]
            .direction
            .counterclockwise_from()
            .filter_map(|direction| {
                self.players
                    .iter()
                    .position(|player| player.direction == direction)
            })
    }

    fn actions(&self, role: Role) -> Vec<Choice> {
        use Action::*;
        let mut choices = vec![];
        let host = role == Role::Seat(0);
        if role == Role::CreatorBoard
            && self.test.is_some()
            && !self.players.is_empty()
            && self.phase != Phase::Lobby
        {
            choices.push(Choice::button(RestartTest));
            choices.push(Choice::button(RestartTestNewSeed));
        }
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
                && (matches!(role, Role::Board | Role::CreatorBoard)
                    || (host && self.board.is_none() && self.creator.is_none()))
            {
                if self.choosing_starter() {
                    let drill = self.test.as_ref().is_some_and(|test| {
                        matches!(test.scenario, TestScenario::Spy | TestScenario::Swap)
                    });
                    let starters = self
                        .players
                        .iter()
                        .filter(|player| !drill || !player.bot)
                        .map(|player| player.id as u32)
                        .collect();
                    choices.push(Choice::select(BeginPlay, starters, 1, 1));
                } else {
                    choices.push(Choice::button(BeginPlay));
                }
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
        if self.phase != Phase::Playing || matches!(round.step, TurnStep::MatchFlipping) {
            return choices;
        }
        if host {
            choices.push(Choice::button(SkipTurn));
        }
        let index = round.turn;
        let hand = &self.players[index];
        let opposing = || {
            self.players
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != index)
                .flat_map(|(_, player)| slots(player))
                .collect()
        };
        if role != Role::Seat(index) {
            if matches!(role, Role::Board | Role::CreatorBoard)
                && !hand.bot
                && round.step.accepts_target()
            {
                choices.push(Choice::select(StagePowerTarget, opposing(), 1, 1));
            }
            return choices;
        }
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
            TurnStep::MatchFlipping => unreachable!(),
            TurnStep::Peek(_) => choices.push(Choice::select(Peek, slots(hand), 1, 1)),
            TurnStep::PeekViewing(..) => choices.push(Choice::button(AcknowledgePeek)),
            TurnStep::SpySelecting(_) => {}
            TurnStep::SpyReview(_, target) => {
                choices.push(Choice::select(ConfirmSpy, vec![target], 1, 1))
            }
            TurnStep::SpyViewing(..) => choices.push(Choice::button(AcknowledgeSpy)),
            TurnStep::SwapSelectingOwn(_) | TurnStep::SwapSelectingOpponent(..) => {
                choices.push(Choice::select(SelectPowerOwnCard, slots(hand), 1, 1));
            }
            TurnStep::SwapReview(_, own, target) => {
                choices.push(Choice::select(SelectPowerOwnCard, slots(hand), 1, 1));
                choices.push(Choice::select(ConfirmSwap, vec![own, target], 2, 2));
            }
        }
        if round.step.accepts_target() {
            choices.push(Choice::select(StagePowerTarget, opposing(), 1, 1));
        }
        choices
    }

    pub fn command(
        &mut self,
        token: &str,
        role: Role,
        command: Command,
    ) -> Result<String, &'static str> {
        let before = self.log_state();
        let action = command.action;
        let slot_count = command.slots.len();
        let replay = self
            .replay
            .contains_key(&(token.to_owned(), command.command_id.clone()));
        let result = self.apply_command(token, role, command);
        let seat = match role {
            Role::Seat(index) => Some(self.players[index].id),
            _ => None,
        };
        self.record(LogEntry {
            sequence: 0,
            event: "command",
            action: Some(action),
            seat,
            board: seat.is_none(),
            slot_count,
            before,
            after: self.log_state(),
            result: match &result {
                Ok(_) if replay => "replayed",
                Ok(_) => "accepted",
                Err(error) => error,
            },
        });
        result
    }

    fn apply_command(
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
        let restart = matches!(
            command.action,
            Action::RestartTest | Action::RestartTestNewSeed
        );
        if restart && command.test_generation != self.test.as_ref().map(|test| test.generation) {
            return Err("staleTest");
        }
        let test_preview = self.test.is_some()
            && matches!(
                command.action,
                Action::RevealInitialCards | Action::ConfirmInitialLook
            )
            && command.expected_revision <= self.revision;
        if !restart && !test_preview && self.revision != command.expected_revision {
            return Err("staleRevision");
        }
        if matches!(
            command.action,
            Action::RevealInitialCards
                | Action::ConfirmInitialLook
                | Action::BeginPlay
                | Action::CancelSetup
                | Action::SkipTurn
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
        if !command.slots.iter().all(|id| choice.slots.contains(id))
            || (command.action == Action::ConfirmSwap && command.slots != choice.slots)
        {
            return Err("invalidSelection");
        }
        if matches!(
            command.action,
            Action::Peek
                | Action::AcknowledgePeek
                | Action::SelectPowerOwnCard
                | Action::StagePowerTarget
                | Action::ConfirmSpy
                | Action::ConfirmSwap
                | Action::AcknowledgeSpy
        ) {
            let round = self.round.as_ref().unwrap();
            if command.deal_id != Some(round.id)
                || command.power_id != round.step.power().map(|p| p.0)
            {
                return Err("stalePower");
            }
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
                let choose = self.choosing_starter();
                let starter = command.slots.first().map(|&id| {
                    self.players
                        .iter()
                        .position(|player| player.id == u64::from(id))
                        .unwrap()
                });
                let round = self.round.as_mut().unwrap();
                round.opening.begin_play().map_err(|_| "notReady")?;
                if choose {
                    self.round_number = 0;
                    self.starter = starter.unwrap();
                    round.turn = self.starter;
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
            RestartTest | RestartTestNewSeed => {
                let test = self.test.as_mut().unwrap();
                test.generation += 1;
                if command.action == RestartTestNewSeed {
                    test.seed = rand::random();
                }
                test.rng = StdRng::seed_from_u64(test.seed);
                self.round_number = 0;
                self.starter = 0;
                self.round = None;
                self.cancel_backup = None;
                self.swap_receipts.fill(None);
                self.phase = Phase::Lobby;
                for player in &mut self.players {
                    player.hand.clear();
                    player.score = 0;
                    player.reset = false;
                }
                "Test restarted. Deal cards to begin."
            }
            SkipTurn => {
                let actor = self.round.as_ref().unwrap().turn;
                self.play(actor, command);
                let message = format!("{}’s turn was skipped.", self.players[actor].name);
                self.changed(&message);
                return Ok(message);
            }
            _ => {
                let actor = self.round.as_ref().unwrap().turn;
                self.play(actor, command)
            }
        };
        self.changed(message);
        Ok(message.into())
    }
}

fn slots(player: &Player) -> Vec<u32> {
    player.hand.iter().map(|card| card.slot).collect()
}

pub fn secret() -> String {
    Uuid::new_v4().simple().to_string()
}

#[cfg(test)]
mod tests;
