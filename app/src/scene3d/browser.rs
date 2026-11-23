use super::{CardKey, Frame, Scene, SceneCard, layout::seat_anchor};
use bevy::prelude::*;
use cabo_shared::protocol::{Action, Choice, Command, Direction};
use std::cell::RefCell;
use wasm_bindgen::prelude::*;

#[derive(PartialEq)]
struct SelectionScope {
    room: String,
    deal: Option<u64>,
    seat: Option<u64>,
    turn: Option<u64>,
    phase: String,
    power: Option<u64>,
}

struct CommandContext {
    scope: SelectionScope,
    selection: Vec<u32>,
    busy: bool,
    revision: u64,
    deal_id: Option<u64>,
    power_id: Option<u64>,
    test_generation: Option<u64>,
    actions: Vec<Choice>,
}

thread_local! {
    static FRAME: RefCell<Option<Frame>> = const { RefCell::new(None) };
    static LAST_LAYOUT: RefCell<String> = const { RefCell::new(String::new()) };
    static PICK: RefCell<Option<Vec2>> = const { RefCell::new(None) };
    static COMMAND_CONTEXT: RefCell<Option<CommandContext>> = const { RefCell::new(None) };
}

#[wasm_bindgen]
pub fn set_scene_frame(json: &str) -> Option<String> {
    let mut frame: Frame = serde_json::from_str(json).ok()?;
    COMMAND_CONTEXT.with_borrow_mut(|current| {
        let Some(snapshot) = &frame.snapshot else {
            *current = None;
            return Some(());
        };
        if current.as_ref().is_some_and(|old| {
            old.scope.room == snapshot.room_id && old.revision > snapshot.revision
        }) {
            return None;
        }
        let power_id = snapshot.pending_power.as_ref().map(|power| power.power_id);
        let scope = SelectionScope {
            room: snapshot.room_id.clone(),
            deal: snapshot.deal_id,
            seat: snapshot.you_seat_id,
            turn: snapshot.turn_seat_id,
            phase: snapshot.phase.clone(),
            power: power_id,
        };
        let mut selection = current
            .take()
            .filter(|old| old.scope == scope)
            .map_or_else(Vec::new, |old| old.selection);
        selection.retain(|slot| selection_limit(&snapshot.available_actions, *slot) > 0);
        if !selection.is_empty()
            && !snapshot.available_actions.iter().any(|choice| {
                selects_cards(choice)
                    && selection.len() <= choice.max
                    && selection.iter().all(|slot| choice.slots.contains(slot))
            })
        {
            selection.clear();
        }
        frame.selection.clone_from(&selection);
        *current = Some(CommandContext {
            scope,
            selection,
            busy: frame.busy,
            revision: snapshot.revision,
            deal_id: snapshot.deal_id,
            power_id,
            test_generation: snapshot.test.as_ref().map(|test| test.generation),
            actions: snapshot.available_actions.clone(),
        });
        Some(())
    })?;
    let selection = serde_json::to_string(&frame.selection).ok()?;
    FRAME.set(Some(frame));
    Some(selection)
}

fn selects_cards(choice: &Choice) -> bool {
    matches!(
        choice.action,
        Action::RevealInitialCards | Action::DiscardHandCard | Action::CallMatch | Action::Peek
    )
}

fn selection_limit(actions: &[Choice], slot: u32) -> usize {
    actions
        .iter()
        .filter(|choice| selects_cards(choice) && choice.slots.contains(&slot))
        .map(|choice| choice.max)
        .max()
        .unwrap_or(0)
}

#[wasm_bindgen]
pub fn toggle_scene_selection(slot: u32) -> bool {
    COMMAND_CONTEXT.with_borrow_mut(|context| {
        let Some(context) = context.as_mut().filter(|context| !context.busy) else {
            return false;
        };
        let limit = selection_limit(&context.actions, slot);
        if limit == 0 {
            return false;
        }
        if let Some(index) = context
            .selection
            .iter()
            .position(|&selected| selected == slot)
        {
            context.selection.remove(index);
        } else if limit == 1 {
            context.selection.clear();
            context.selection.push(slot);
        } else if context.selection.len() < limit {
            context.selection.push(slot);
        }
        true
    })
}

#[wasm_bindgen]
pub fn clear_scene_selection() {
    COMMAND_CONTEXT.with_borrow_mut(|context| {
        if let Some(context) = context {
            context.selection.clear();
        }
    });
}

#[wasm_bindgen]
pub fn build_scene_command(
    action: &str,
    slots: &str,
    command_id: String,
    insert_left: bool,
) -> Option<String> {
    let slots: Option<Vec<u32>> = serde_json::from_str(slots).ok()?;
    let intent: Action = serde::Deserialize::deserialize(serde::de::value::StrDeserializer::<
        serde::de::value::Error,
    >::new(action))
    .ok()?;
    COMMAND_CONTEXT.with_borrow(|context| {
        let context = context.as_ref()?;
        let choice = context
            .actions
            .iter()
            .find(|choice| choice.action == intent)?;
        let slots = slots.unwrap_or_else(|| {
            if choice.max == 0 {
                vec![]
            } else {
                context.selection.clone()
            }
        });
        if !(choice.min..=choice.max).contains(&slots.len())
            || slots
                .iter()
                .enumerate()
                .any(|(i, slot)| !choice.slots.contains(slot) || slots[..i].contains(slot))
            || (intent == Action::ConfirmSwap && slots != choice.slots)
        {
            return None;
        }
        serde_json::to_string(&Command {
            command_id,
            expected_revision: context.revision,
            action: intent,
            slots,
            insert_left,
            deal_id: context.deal_id,
            power_id: context.power_id,
            test_generation: context.test_generation,
        })
        .ok()
    })
}

#[wasm_bindgen]
pub fn pick_scene_card(x: f32, y: f32) {
    PICK.set(Some(Vec2::new(x, y)));
}

#[wasm_bindgen(
    inline_js = "export function emitCaboPick(key) { window.dispatchEvent(new CustomEvent('cabo-scene-pick', {detail:key})); }"
)]
extern "C" {
    #[wasm_bindgen(js_name = emitCaboPick)]
    fn emit_cabo_pick(key: &str);
}

#[derive(serde::Serialize)]
struct OverlayAnchor {
    key: String,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    angle: f32,
}

pub(super) fn publish_layout(
    camera: Single<(&Camera, &GlobalTransform), With<Camera3d>>,
    scene: Res<Scene>,
    cards: Query<(&SceneCard, &Transform)>,
) {
    let Some(snapshot) = scene.frame.as_ref().and_then(|f| f.snapshot.as_ref()) else {
        return;
    };
    let (camera, view) = *camera;
    let project = |point| camera.world_to_viewport(view, point).ok();
    let mut anchors = Vec::with_capacity(scene.cards.len() + 6);
    let mut anchor_point = |key: String, position: Vec3, angle: f32| {
        if let Some(point) = project(position) {
            anchors.push(OverlayAnchor {
                key,
                x: point.x.round(),
                y: point.y.round(),
                width: 0.0,
                height: 0.0,
                angle,
            });
        }
    };
    let board = snapshot.role == "board";
    let show_table = board
        || matches!(snapshot.phase.as_str(), "roundResults" | "gameOver")
        || snapshot.pending_power.as_ref().is_some_and(|power| {
            Some(power.actor_seat_id) == snapshot.you_seat_id
                && matches!(
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
    if show_table {
        for (key, direction) in [
            ("north", Direction::North),
            ("east", Direction::East),
            ("south", Direction::South),
            ("west", Direction::West),
        ] {
            let (position, rotation) = seat_anchor(direction, scene.width);
            let offset = if !board && matches!(direction, Direction::East | Direction::West) {
                Vec3::NEG_Z * 2.6
            } else {
                Quat::from_rotation_y(rotation) * Vec3::Z * if board { 1.22 } else { 1.05 }
            };
            anchor_point(
                format!("seat:{key}"),
                position + offset,
                if board { -rotation.to_degrees() } else { 0.0 },
            );
        }
    }
    for (card, _) in &cards {
        if card.exiting {
            continue;
        }
        if let Some(key) = match card.placement.key {
            CardKey::Deck => Some("deck"),
            CardKey::Discard => Some("discard"),
            _ => None,
        } {
            anchor_point(
                format!("label:{key}"),
                card.placement.position + Vec3::Z * 0.83,
                0.0,
            );
        }
    }
    for (card, mesh) in &cards {
        if card.exiting {
            continue;
        }
        let key = match card.placement.key {
            CardKey::Hand(slot) => format!("hand:{slot}"),
            CardKey::Match(slot) => format!("match:{slot}"),
            CardKey::Deck => "pile:deck".into(),
            CardKey::Discard => "pile:discard".into(),
            CardKey::PendingDraw => continue,
        };
        let corners = [
            Vec3::new(-0.5, 0.036, -0.7),
            Vec3::new(-0.5, 0.036, 0.7),
            Vec3::new(0.5, 0.036, -0.7),
            Vec3::new(0.5, 0.036, 0.7),
        ];
        let bounds = corners
            .into_iter()
            .filter_map(|corner| project(mesh.transform_point(corner)))
            .fold(None::<(Vec2, Vec2)>, |bounds, point| {
                Some(match bounds {
                    None => (point, point),
                    Some((min, max)) => (min.min(point), max.max(point)),
                })
            });
        if let Some((min, max)) = bounds {
            let center = (min + max) * 0.5;
            anchors.push(OverlayAnchor {
                key,
                x: center.x.round(),
                y: center.y.round(),
                width: (max.x - min.x).round(),
                height: (max.y - min.y).round(),
                angle: 0.0,
            });
        }
    }
    anchors.sort_unstable_by(|a, b| a.key.cmp(&b.key));
    if let Ok(json) =
        serde_json::to_string(&(snapshot.room_id.as_str(), snapshot.revision, anchors))
    {
        LAST_LAYOUT.with_borrow_mut(|last| {
            if *last != json {
                emit_cabo_layout(&json);
                *last = json;
            }
        });
    }
}

#[wasm_bindgen(
    inline_js = "export function emitCaboLayout(json) { window.dispatchEvent(new CustomEvent('cabo-scene-layout', {detail:json})); }"
)]
extern "C" {
    #[wasm_bindgen(js_name = emitCaboLayout)]
    fn emit_cabo_layout(json: &str);
}

pub(super) fn take_frame() -> Option<Frame> {
    FRAME.take()
}
pub(super) fn take_pick() -> Option<Vec2> {
    PICK.take()
}
pub(super) fn emit_pick(key: CardKey) {
    match key {
        CardKey::Hand(slot) => {
            let direct = COMMAND_CONTEXT.with_borrow(|context| {
                context.as_ref().and_then(|context| {
                    context
                        .actions
                        .iter()
                        .filter(|choice| choice.slots.contains(&slot))
                        .find_map(|choice| match choice.action {
                            Action::StagePowerTarget => Some("stagePowerTarget"),
                            Action::SelectPowerOwnCard => Some("selectPowerOwnCard"),
                            _ => None,
                        })
                })
            });
            if let Some(action) = direct {
                emit_cabo_pick(&format!("{action}:{slot}"));
            } else if toggle_scene_selection(slot) {
                emit_cabo_pick("selection");
            }
        }
        CardKey::Deck => emit_cabo_pick("deck"),
        CardKey::Discard => emit_cabo_pick("discard"),
        CardKey::Match(_) | CardKey::PendingDraw => {}
    }
}
