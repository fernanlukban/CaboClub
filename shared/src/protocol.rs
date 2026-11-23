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
    Spy,
    Swap,
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
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerView {
    pub seat_id: u64,
    pub name: String,
    pub score: u16,
    pub ready: bool,
    pub cards: Vec<CardView>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoundResult {
    pub seat_id: u64,
    pub hand_total: u16,
    pub points: u16,
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
    pub players: Vec<PlayerView>,
    pub deck_count: usize,
    pub discard_top: Option<u8>,
    pub incoming: Option<CardView>,
    pub cabo_caller: Option<u64>,
    pub final_turns: Vec<u64>,
    pub results: Vec<RoundResult>,
    pub winners: Vec<u64>,
    pub available_actions: Vec<Choice>,
    pub message: String,
}
