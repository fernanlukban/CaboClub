use std::{
    collections::{HashMap, HashSet, VecDeque},
    time::{Duration, Instant},
};

use cabo_shared::{
    game::{self, MAX_PLAYERS, MIN_PLAYERS, STARTING_HAND_SIZE, cabo_power::Power},
    protocol::{
        Action, CardView, Choice, Command, Direction, MatchFlip, MatchOutcome, PendingPower,
        PlayerView, PowerResult, RoundResult, Snapshot, TestScenario, TestView,
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
            Self::SpyViewing(id, ..) => (id, "spyViewing", None, None),
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

    pub fn tick_bot(&mut self, now: Instant) {
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

    fn changed(&mut self, message: &str) {
        self.revision += 1;
        self.last_active = Instant::now();
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

    pub fn actions(&self, role: Role) -> Vec<Choice> {
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

    fn play(&mut self, seat: usize, command: &Command) -> &'static str {
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

    fn resolve_match(&mut self) -> &'static str {
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

    fn finish_turn(&mut self, seat: usize) -> bool {
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
        let incoming = round.and_then(|round| {
            let card = match round.step {
                TurnStep::Incoming { card, .. } => Some(card),
                TurnStep::MatchFlipping => round
                    .pending_match
                    .as_ref()
                    .and_then(|pending| pending.incoming.map(|(card, _)| card)),
                _ => None,
            }?;
            (card.face_up || viewer == Some(round.turn)).then(|| card.view(true))
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
            test_generation: room.test.as_ref().map(|test| test.generation),
            power_id: room
                .round
                .as_ref()
                .and_then(|round| round.step.power().map(|p| p.0)),
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
        room.join("Alex", None).unwrap();
        room.join("Bea", None).unwrap();
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
        let phone_start = command(room, "phone-start", Action::BeginPlay, vec![1]);
        assert_eq!(
            room.command(&host, Role::Seat(0), phone_start),
            Err("actionUnavailable")
        );
        run(room, Role::CreatorBoard, Action::BeginPlay, vec![1]);
        assert!(
            room.actions(Role::CreatorBoard)
                .iter()
                .all(|choice| matches!(
                    choice.action,
                    Action::RestartTest | Action::RestartTestNewSeed
                ))
        );
    }

    #[test]
    fn board_cannot_start_early_and_confirmed_ranks_stay_hidden_on_replay() {
        let mut room = setup();
        let board = room.creator.clone().unwrap();
        let start = command(&room, "early", Action::BeginPlay, vec![1]);
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
    fn board_stages_spy_but_only_actor_reveals_and_acknowledges() {
        let mut room = setup();
        start(&mut room);
        room.round.as_mut().unwrap().deck.last_mut().unwrap().rank = 9;
        run(&mut room, Role::Seat(0), Action::DrawDeck, vec![]);
        run(&mut room, Role::Seat(0), Action::UseDrawnPower, vec![]);
        let target = room.players[1].hand[0];
        let alternative = room.players[1].hand[1].slot;
        let actor = room.players[0].token.clone();
        let board = room.creator.clone().unwrap();
        let racing = command(&room, "race", Action::StagePowerTarget, vec![alternative]);
        run(
            &mut room,
            Role::CreatorBoard,
            Action::StagePowerTarget,
            vec![target.slot],
        );
        assert_eq!(
            room.command(&actor, Role::Seat(0), racing),
            Err("staleRevision")
        );
        let confirm = command(&room, "confirm", Action::ConfirmSpy, vec![target.slot]);
        assert_eq!(
            room.command(&board, Role::CreatorBoard, confirm.clone()),
            Err("actionUnavailable")
        );
        room.command(&actor, Role::Seat(0), confirm.clone())
            .unwrap();
        let reveal = room.snapshot(Role::Seat(0)).private_power_result.unwrap();
        assert_eq!(
            (reveal.slot, reveal.rank),
            (Some(target.slot), Some(target.rank))
        );
        assert!(
            room.snapshot(Role::CreatorBoard)
                .private_power_result
                .is_none()
        );
        assert!(room.snapshot(Role::Seat(1)).private_power_result.is_none());
        assert!(room.actions(Role::CreatorBoard).is_empty());
        assert_eq!(room.round.as_ref().unwrap().turn, 0);
        run(&mut room, Role::Seat(0), Action::AcknowledgeSpy, vec![]);
        room.command(&actor, Role::Seat(0), confirm).unwrap();
        assert!(room.snapshot(Role::Seat(0)).private_power_result.is_none());
        assert_eq!(room.round.as_ref().unwrap().turn, 1);
    }

    #[test]
    fn swap_requires_current_pair_and_is_applied_once() {
        let mut room = setup();
        start(&mut room);
        room.round.as_mut().unwrap().deck.last_mut().unwrap().rank = 11;
        run(&mut room, Role::Seat(0), Action::DrawDeck, vec![]);
        run(&mut room, Role::Seat(0), Action::UseDrawnPower, vec![]);
        let own = room.players[0].hand[0].slot;
        let target = room.players[1].hand[0].slot;
        let actor = room.players[0].token.clone();
        run(
            &mut room,
            Role::Seat(0),
            Action::SelectPowerOwnCard,
            vec![own],
        );
        run(
            &mut room,
            Role::CreatorBoard,
            Action::StagePowerTarget,
            vec![target],
        );
        let incorrect = command(&room, "reverse", Action::ConfirmSwap, vec![target, own]);
        assert_eq!(
            room.command(&actor, Role::Seat(0), incorrect),
            Err("invalidSelection")
        );
        let confirm = command(&room, "swap", Action::ConfirmSwap, vec![own, target]);
        room.command(&actor, Role::Seat(0), confirm.clone())
            .unwrap();
        room.command(&actor, Role::Seat(0), confirm).unwrap();
        assert_eq!(room.players[0].hand[0].slot, target);
        assert_eq!(room.players[1].hand[0].slot, own);
        assert_eq!(
            room.snapshot(Role::Seat(0))
                .private_power_result
                .unwrap()
                .kind,
            "swap"
        );
        assert!(
            room.snapshot(Role::CreatorBoard)
                .private_power_result
                .is_none()
        );
        assert_eq!(room.round.as_ref().unwrap().turn, 1);
    }

    #[test]
    fn bot_room_restarts_with_original_seed_and_sessions() {
        let mut room = Room::new("BOTS".into(), true);
        room.enable_test(TestScenario::Spy, 42);
        let player = room.join("Avery", None).unwrap();
        let board = room.creator.clone().unwrap();
        assert_eq!(room.players.iter().filter(|player| player.bot).count(), 3);
        run(&mut room, Role::CreatorBoard, Action::DealRound, vec![]);
        let original: Vec<_> = room.players[0].hand.iter().map(|c| c.rank).collect();
        assert_eq!(room.round.as_ref().unwrap().deck.last().unwrap().rank, 9);
        for _ in 0..6 {
            room.tick_bot(Instant::now() + Duration::from_secs(1));
        }
        assert!(room.players[1..].iter().all(|bot| {
            room.round
                .as_ref()
                .unwrap()
                .opening
                .preview(bot.id)
                .is_some_and(|preview| matches!(preview, Preview::Confirmed(_)))
        }));
        assert_eq!(
            room.round.as_ref().unwrap().opening.preview(1),
            Some(Preview::Hidden)
        );
        run(&mut room, Role::CreatorBoard, Action::RestartTest, vec![]);
        room.tick_bot(Instant::now() + Duration::from_secs(1));
        assert_eq!(room.role(&board), Some(Role::CreatorBoard));
        assert_eq!(room.role(&player), Some(Role::Seat(0)));
        assert_eq!(room.test.as_ref().unwrap().generation, 2);
        run(&mut room, Role::CreatorBoard, Action::DealRound, vec![]);
        assert_eq!(
            room.players[0]
                .hand
                .iter()
                .map(|c| c.rank)
                .collect::<Vec<_>>(),
            original
        );
    }

    #[test]
    fn explicit_seats_follow_counterclockwise_cabo_and_round_winner_starts() {
        let mut room = Room::new("COMPASS".into(), true);
        for (name, direction) in [
            ("East", Direction::East),
            ("South", Direction::South),
            ("North", Direction::North),
            ("West", Direction::West),
        ] {
            room.join(name, Some(direction)).unwrap();
        }
        assert_eq!(
            room.counterclockwise_seat_indexes(2).collect::<Vec<_>>(),
            [3, 1, 0, 2]
        );
        let ready = |room: &mut Room| {
            for index in 0..4 {
                let pair = slots(&room.players[index])[..2].to_vec();
                run(room, Role::Seat(index), Action::RevealInitialCards, pair);
                run(room, Role::Seat(index), Action::ConfirmInitialLook, vec![]);
            }
        };
        run(&mut room, Role::CreatorBoard, Action::DealRound, vec![]);
        ready(&mut room);
        let board = room.creator.clone().unwrap();
        let missing = command(&room, "missing", Action::BeginPlay, vec![]);
        assert_eq!(
            room.command(&board, Role::CreatorBoard, missing),
            Err("invalidSelection")
        );
        run(&mut room, Role::CreatorBoard, Action::BeginPlay, vec![3]);
        assert_eq!(room.round.as_ref().unwrap().turn, 2);
        run(&mut room, Role::Seat(2), Action::DrawDeck, vec![]);
        run(&mut room, Role::Seat(2), Action::DiscardDrawn, vec![]);
        assert_eq!(room.round.as_ref().unwrap().turn, 3);
        run(&mut room, Role::Seat(3), Action::CallCabo, vec![]);
        assert_eq!(
            room.round
                .as_ref()
                .unwrap()
                .final_turns
                .iter()
                .copied()
                .collect::<Vec<_>>(),
            [1, 0, 2]
        );
        for (index, player) in room.players.iter_mut().enumerate() {
            for card in &mut player.hand {
                card.rank = if index == 0 { 1 } else { 5 };
            }
        }
        for index in [1, 0, 2] {
            assert_eq!(room.round.as_ref().unwrap().turn, index);
            run(&mut room, Role::Seat(index), Action::DrawDeck, vec![]);
            run(&mut room, Role::Seat(index), Action::DiscardDrawn, vec![]);
        }
        assert_eq!(
            room.snapshot(Role::CreatorBoard).next_starter_seat_id,
            Some(1)
        );
        run(&mut room, Role::CreatorBoard, Action::DealNextRound, vec![]);
        ready(&mut room);
        run(&mut room, Role::CreatorBoard, Action::BeginPlay, vec![]);
        assert_eq!(room.round.as_ref().unwrap().turn, 0);
    }

    #[test]
    fn assigned_compass_seats_are_unique_and_skip_open_directions() {
        let mut room = Room::new("TWO".into(), true);
        room.join("West", Some(Direction::West)).unwrap();
        assert_eq!(
            room.join("Another West", Some(Direction::West)),
            Err("seatTaken")
        );
        room.join("East", Some(Direction::East)).unwrap();
        assert_eq!(
            room.counterclockwise_seat_indexes(0).collect::<Vec<_>>(),
            [1, 0]
        );
        assert_eq!(
            room.snapshot(Role::CreatorBoard).players[0].direction,
            Direction::West
        );
    }

    #[test]
    fn taking_last_discard_reveals_a_replacement_and_keeps_the_draw() {
        let mut room = setup();
        start(&mut room);
        let round = room.round.as_ref().unwrap();
        let taken = round.discard[0];
        let replacement = *round.deck.last().unwrap();
        let deck_count = round.deck.len();
        run(&mut room, Role::Seat(0), Action::DrawDiscard, vec![]);
        let round = room.round.as_ref().unwrap();
        assert_eq!(round.discard.last().unwrap().slot, replacement.slot);
        assert!(round.discard.last().unwrap().face_up);
        assert_eq!(round.deck.len(), deck_count - 1);
        assert_eq!(
            room.snapshot(Role::Seat(0)).incoming.unwrap().slot,
            taken.slot
        );
        let trace = room.game_log();
        let entry = trace.entries.back().unwrap();
        assert_eq!(entry.action, Some(Action::DrawDiscard));
        assert_eq!(entry.before.discard_count, 1);
        assert_eq!(entry.after.discard_count, 1);
    }

    #[test]
    fn missed_match_first_refills_before_a_penalty_and_preserves_the_round() {
        let mut room = setup();
        start(&mut room);
        room.players[0].hand[0].rank = 2;
        room.players[0].hand[1].rank = 3;
        room.round.as_mut().unwrap().deck.truncate(1);
        let selected = slots(&room.players[0])[..3].to_vec();
        run(&mut room, Role::Seat(0), Action::CallMatch, selected);
        let deadline = room
            .round
            .as_ref()
            .unwrap()
            .pending_match
            .as_ref()
            .unwrap()
            .deadline;
        room.tick(deadline);
        let round = room.round.as_ref().unwrap();
        assert_eq!(round.discard.len(), 1);
        assert_eq!(room.players[0].hand.len(), 5);
        assert_eq!(room.snapshot(Role::CreatorBoard).phase, "roundResults");
    }

    #[test]
    fn seeded_legal_turn_fuzz_keeps_cards_and_discard_intact() {
        for seed in 0..128 {
            let mut room = Room::new("FUZZ".into(), true);
            room.enable_test(TestScenario::Random, seed);
            room.join("Human", None).unwrap();
            run(&mut room, Role::CreatorBoard, Action::DealRound, vec![]);
            start(&mut room);
            let mut rng = StdRng::seed_from_u64(seed);
            for step in 0..400 {
                let round = room.round.as_ref().unwrap();
                assert!(!round.discard.is_empty(), "seed {seed}, step {step}");
                let incoming = match round.step {
                    TurnStep::Incoming { card, .. } => Some(card.slot),
                    TurnStep::MatchFlipping => round
                        .pending_match
                        .as_ref()
                        .unwrap()
                        .incoming
                        .map(|(card, _)| card.slot),
                    _ => None,
                };
                let all_slots: Vec<_> = round
                    .deck
                    .iter()
                    .chain(&round.discard)
                    .chain(room.players.iter().flat_map(|player| &player.hand))
                    .map(|card| card.slot)
                    .chain(incoming)
                    .collect();
                assert_eq!(all_slots.len(), 52, "seed {seed}, step {step}");
                assert_eq!(
                    all_slots.into_iter().collect::<HashSet<_>>().len(),
                    52,
                    "seed {seed}, step {step}"
                );
                if room.phase != Phase::Playing {
                    break;
                }
                if let Some(pending) = &round.pending_match {
                    room.tick(pending.deadline);
                    continue;
                }
                let role = Role::Seat(round.turn);
                let mut actions = room.actions(role);
                actions.retain(|choice| {
                    !matches!(choice.action, Action::PairBoard | Action::RevokeBoard)
                });
                assert!(!actions.is_empty(), "seed {seed}, step {step}");
                let choice = actions.swap_remove(rng.random_range(0..actions.len()));
                let mut selected = choice.slots;
                if !matches!(choice.action, Action::ConfirmSpy | Action::ConfirmSwap) {
                    selected.shuffle(&mut rng);
                    selected.truncate(rng.random_range(choice.min..=choice.max));
                }
                run(&mut room, role, choice.action, selected);
            }
            assert!(
                room.phase != Phase::Playing,
                "seed {seed} got stuck in an open round"
            );
        }
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
        let flip = room.round.as_ref().unwrap().pending_match.as_ref().unwrap();
        let deadline = flip.deadline;
        let board = room.snapshot(Role::CreatorBoard);
        assert_eq!(board.match_flip.unwrap().slots.len(), 3);
        assert_eq!(board.players[0].cards.len(), 4);
        assert!(
            board.players[0].cards[..3]
                .iter()
                .all(|card| card.face_up && card.rank == Some(4))
        );
        assert_eq!(board.players[0].cards[3].rank, None);
        assert!(
            room.actions(Role::Seat(0))
                .iter()
                .all(|choice| !matches!(choice.action, Action::SkipTurn | Action::CallMatch))
        );
        room.tick(deadline - Duration::from_millis(1));
        assert_eq!(room.round.as_ref().unwrap().turn, 0);
        room.tick(deadline);
        assert_eq!(room.players[0].hand.len(), 2);
        assert!(room.players[0].hand[0].face_up);
        assert_eq!(room.players[0].hand[0].slot, previous_discard.slot);
        assert_eq!(room.round.as_ref().unwrap().discard.last().unwrap().rank, 4);
        assert_eq!(room.round.as_ref().unwrap().turn, 1);
        let revision = room.revision;
        room.command(&token, Role::Seat(0), match_cards).unwrap();
        assert_eq!(room.revision, revision);
    }

    #[test]
    fn peek_stays_private_until_done_even_on_the_last_draw() {
        let mut room = setup();
        start(&mut room);
        let round = room.round.as_mut().unwrap();
        let mut draw = round.deck.pop().unwrap();
        draw.rank = 7;
        round.deck = vec![draw];
        run(&mut room, Role::Seat(0), Action::DrawDeck, vec![]);
        run(&mut room, Role::Seat(0), Action::UseDrawnPower, vec![]);
        let target = room.players[0].hand[0];
        run(&mut room, Role::Seat(0), Action::Peek, vec![target.slot]);
        let actor = room.snapshot(Role::Seat(0));
        assert_eq!(actor.phase, "playing");
        let reveal = actor.private_power_result.unwrap();
        assert_eq!(
            (reveal.slot, reveal.rank),
            (Some(target.slot), Some(target.rank))
        );
        for role in [Role::Seat(1), Role::CreatorBoard] {
            assert!(room.snapshot(role).private_power_result.is_none());
        }
        run(&mut room, Role::Seat(0), Action::AcknowledgePeek, vec![]);
        let actor = room.snapshot(Role::Seat(0));
        assert_eq!(actor.phase, "roundResults");
        assert!(actor.private_power_result.is_none());
    }

    #[test]
    fn host_can_start_after_revoke_and_skip_a_pending_discard_pickup() {
        let mut room = setup();
        for index in 0..2 {
            let pair = slots(&room.players[index])[..2].to_vec();
            run(
                &mut room,
                Role::Seat(index),
                Action::RevealInitialCards,
                pair,
            );
            run(
                &mut room,
                Role::Seat(index),
                Action::ConfirmInitialLook,
                vec![],
            );
        }
        run(&mut room, Role::Seat(0), Action::RevokeBoard, vec![]);
        run(&mut room, Role::Seat(0), Action::BeginPlay, vec![1]);
        run(&mut room, Role::Seat(0), Action::CallCabo, vec![]);
        let discard = room.round.as_ref().unwrap().discard.last().unwrap().slot;
        let hand = slots(&room.players[1]);
        run(&mut room, Role::Seat(1), Action::DrawDiscard, vec![]);
        let guest = room.players[1].token.clone();
        let unauthorized = command(&room, "guest-skip", Action::SkipTurn, vec![]);
        assert_eq!(
            room.command(&guest, Role::Seat(1), unauthorized),
            Err("actionUnavailable")
        );
        let host = room.players[0].token.clone();
        let skip = command(&room, "host-skip", Action::SkipTurn, vec![]);
        room.command(&host, Role::Seat(0), skip.clone()).unwrap();
        assert_eq!(room.snapshot(Role::Seat(0)).phase, "roundResults");
        assert_eq!(
            room.round.as_ref().unwrap().discard.last().unwrap().slot,
            discard
        );
        assert_eq!(slots(&room.players[1]), hand);
        let revision = room.revision;
        room.command(&host, Role::Seat(0), skip).unwrap();
        assert_eq!(room.revision, revision);
    }

    #[test]
    fn two_card_miss_keeps_incoming_without_adding_a_penalty() {
        let mut room = setup();
        start(&mut room);
        room.players[0].hand[0].rank = 4;
        room.players[0].hand[1].rank = 5;
        let pair = slots(&room.players[0])[..2].to_vec();
        run(&mut room, Role::Seat(0), Action::CallMatch, pair);
        let deadline = room
            .round
            .as_ref()
            .unwrap()
            .pending_match
            .as_ref()
            .unwrap()
            .deadline;
        room.tick(deadline);
        let board = room.snapshot(Role::CreatorBoard);
        assert!(board.match_flip.is_none());
        assert_eq!(
            board.players[0]
                .cards
                .iter()
                .map(|card| card.rank)
                .collect::<Vec<_>>(),
            vec![
                Some(4),
                Some(5),
                None,
                None,
                Some(room.players[0].hand[4].rank)
            ]
        );
        let result = room.snapshot(Role::Seat(0)).match_outcome.unwrap();
        assert!(!result.matched);
        assert!(result.penalty_slot.is_none());
        assert!(result.discarded_slots.is_empty());
        assert_eq!(
            room.players[0].hand.last().unwrap().slot,
            result.incoming_slot
        );
        assert!(room.snapshot(Role::Seat(1)).match_outcome.is_none());
        assert!(room.snapshot(Role::CreatorBoard).match_outcome.is_none());
    }

    #[test]
    fn board_marks_opening_slots_without_ranks_until_first_move() {
        let mut room = setup();
        start(&mut room);
        let board = room.snapshot(Role::CreatorBoard);
        for player in &board.players {
            let expected = [player.cards[0].slot, player.cards[1].slot];
            assert_eq!(player.opening_look_slots, Some(expected));
            assert!(player.cards.iter().all(|card| card.rank.is_none()));
        }
        assert!(
            room.snapshot(Role::Seat(0))
                .players
                .iter()
                .all(|player| player.opening_look_slots.is_none())
        );
        run(&mut room, Role::Seat(0), Action::DrawDeck, vec![]);
        assert!(
            room.snapshot(Role::CreatorBoard)
                .players
                .iter()
                .all(|player| player.opening_look_slots.is_none())
        );
    }

    #[test]
    fn face_up_discard_stays_public_when_picked_up_and_kept() {
        let mut room = setup();
        start(&mut room);
        let discard = room.round.as_ref().unwrap().discard.last().unwrap().rank;
        run(&mut room, Role::Seat(0), Action::DrawDiscard, vec![]);
        let incoming = room.snapshot(Role::CreatorBoard).incoming.unwrap();
        assert!(incoming.face_up);
        assert_eq!(incoming.rank, Some(discard));
        let target = room.players[0].hand[0].slot;
        run(
            &mut room,
            Role::Seat(0),
            Action::DiscardHandCard,
            vec![target],
        );
        let kept = &room.snapshot(Role::Seat(1)).players[0].cards[0];
        assert!(kept.face_up);
        assert_eq!(kept.rank, Some(discard));
        run(&mut room, Role::Seat(1), Action::DrawDeck, vec![]);
        assert!(room.snapshot(Role::CreatorBoard).incoming.is_none());
        assert!(!room.snapshot(Role::Seat(1)).incoming.unwrap().face_up);
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
