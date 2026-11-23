use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Action {
    DealRound,
    DealNextRound,
    DealNewGame,
    CancelSetup,
    RevealInitialCards,
    ConfirmInitialLook,
    BeginPlay,
    DrawDeck,
    DrawDiscard,
    DiscardDrawn,
    DiscardHandCard,
    CallMatch,
    CallCabo,
    UseDrawnPower,
    Peek,
    AcknowledgePeek,
    SkipTurn,
    SelectPowerOwnCard,
    StagePowerTarget,
    ConfirmSpy,
    ConfirmSwap,
    AcknowledgeSpy,
    RestartTest,
    RestartTestNewSeed,
    PairBoard,
    RevokeBoard,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Command {
    pub command_id: String,
    pub expected_revision: u64,
    pub action: Action,
    #[serde(default)]
    pub slots: Vec<u32>,
    #[serde(default)]
    pub insert_left: bool,
    pub deal_id: Option<u64>,
    pub power_id: Option<u64>,
    pub test_generation: Option<u64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Choice {
    pub action: Action,
    pub slots: Vec<u32>,
    pub targets: Vec<u32>,
    pub min: usize,
    pub max: usize,
    pub choose_side: bool,
}

impl Choice {
    pub fn button(action: Action) -> Self {
        Self {
            action,
            slots: vec![],
            targets: vec![],
            min: 0,
            max: 0,
            choose_side: false,
        }
    }

    pub fn select(action: Action, slots: Vec<u32>, min: usize, max: usize) -> Self {
        Self {
            slots,
            min,
            max,
            ..Self::button(action)
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CardView {
    pub slot: u32,
    pub rank: Option<u8>,
    pub face_up: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Direction {
    North,
    West,
    South,
    East,
}

impl Direction {
    pub const COUNTERCLOCKWISE: [Self; 4] = [Self::North, Self::West, Self::South, Self::East];
    pub const DEFAULT_SEATS: [Self; 4] = [Self::South, Self::North, Self::West, Self::East];

    pub fn counterclockwise_from(self) -> impl Iterator<Item = Self> {
        let start = Self::COUNTERCLOCKWISE
            .iter()
            .position(|&direction| direction == self)
            .unwrap();
        (1..=4).map(move |offset| Self::COUNTERCLOCKWISE[(start + offset) % 4])
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerView {
    pub seat_id: u64,
    pub direction: Direction,
    pub name: String,
    pub score: u16,
    pub ready: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opening_look_slots: Option<[u32; 2]>,
    pub bot: bool,
    pub cards: Vec<CardView>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoundResult {
    pub seat_id: u64,
    pub hand_total: u16,
    pub points: u16,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TestScenario {
    Random,
    Spy,
    Swap,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestView {
    pub scenario: TestScenario,
    pub seed: String,
    pub generation: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingPower {
    pub power_id: u64,
    pub actor_seat_id: u64,
    pub kind: &'static str,
    pub phase: &'static str,
    pub own_slot: Option<u32>,
    pub target_slot: Option<u32>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PowerResult {
    pub result_id: u64,
    pub kind: &'static str,
    pub slot: Option<u32>,
    pub rank: Option<u8>,
    pub target_name: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchFlip {
    pub revision: u64,
    pub actor_seat_id: u64,
    pub slots: Vec<u32>,
    pub newly_face_up_slots: Vec<u32>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchOutcome {
    pub revision: u64,
    pub matched: bool,
    pub source: &'static str,
    pub incoming_slot: u32,
    pub placement: &'static str,
    pub discarded_slots: Vec<u32>,
    pub penalty_slot: Option<u32>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub protocol_version: u8,
    pub room_id: String,
    pub room_code: String,
    pub revision: u64,
    pub role: &'static str,
    pub you_seat_id: Option<u64>,
    pub host_seat_id: Option<u64>,
    pub phase: &'static str,
    pub round: u32,
    pub deal_id: Option<u64>,
    pub turn_seat_id: Option<u64>,
    pub next_starter_seat_id: Option<u64>,
    pub players: Vec<PlayerView>,
    pub deck_count: usize,
    pub discard_top: Option<u8>,
    pub incoming: Option<CardView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub incoming_source: Option<&'static str>,
    pub pending_power: Option<PendingPower>,
    pub private_power_result: Option<PowerResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_flip: Option<MatchFlip>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_outcome: Option<MatchOutcome>,
    pub test: Option<TestView>,
    pub cabo_caller: Option<u64>,
    pub final_turns: Vec<u64>,
    pub results: Vec<RoundResult>,
    pub winners: Vec<u64>,
    pub available_actions: Vec<Choice>,
    pub message: String,
}
