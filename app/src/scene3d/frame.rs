use cabo_shared::protocol::{CardTravel, Choice, Command, Direction};
use serde::Deserialize;

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CardView {
    pub(super) slot: u32,
    pub(super) rank: Option<u8>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PlayerView {
    pub(super) seat_id: u64,
    pub(super) direction: Direction,
    pub(super) cards: Vec<CardView>,
    pub(super) opening_look_slots: Option<[u32; 2]>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct MatchFlip {
    pub(super) revision: u64,
    pub(super) actor_seat_id: u64,
    pub(super) slots: Vec<u32>,
    pub(super) newly_face_up_slots: Vec<u32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PendingPower {
    pub(super) power_id: u64,
    pub(super) actor_seat_id: u64,
    pub(super) phase: String,
    pub(super) own_slot: Option<u32>,
    pub(super) target_slot: Option<u32>,
}

#[derive(Deserialize)]
pub(super) struct TestView {
    pub(super) generation: u64,
}

#[derive(Deserialize)]
pub(super) struct PrivateLook {
    pub(super) slot: Option<u32>,
    pub(super) rank: Option<u8>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct MatchOutcome {
    pub(super) source: String,
    pub(super) incoming_slot: u32,
    pub(super) penalty_slot: Option<u32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SceneSnapshot {
    pub(super) room_id: String,
    pub(super) deal_id: Option<u64>,
    pub(super) revision: u64,
    pub(super) role: String,
    pub(super) phase: String,
    pub(super) you_seat_id: Option<u64>,
    pub(super) turn_seat_id: Option<u64>,
    pub(super) players: Vec<PlayerView>,
    pub(super) deck_count: usize,
    pub(super) discard_top: Option<u8>,
    pub(super) incoming: Option<CardView>,
    pub(super) incoming_source: Option<String>,
    pub(super) match_outcome: Option<MatchOutcome>,
    pub(super) card_travel: Option<CardTravel>,
    pub(super) match_flip: Option<MatchFlip>,
    pub(super) pending_power: Option<PendingPower>,
    pub(super) private_power_result: Option<PrivateLook>,
    pub(super) available_actions: Vec<Choice>,
    pub(super) test: Option<TestView>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Frame {
    pub(super) snapshot: Option<SceneSnapshot>,
    #[serde(skip)]
    pub(super) selection: Vec<u32>,
    pub(super) busy: bool,
    pub(super) reduced_motion: bool,
    pub(super) optimistic: Option<Command>,
}
