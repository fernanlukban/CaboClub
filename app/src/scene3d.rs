use std::collections::HashMap;
use std::f32::consts::{FRAC_PI_2, PI};

use bevy::{
    asset::RenderAssetUsages,
    camera::ScalingMode,
    core_pipeline::tonemapping::Tonemapping,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use cabo_shared::protocol::{Action, Command, Direction};
use serde::Deserialize;

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CardView {
    slot: u32,
    rank: Option<u8>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlayerView {
    seat_id: u64,
    direction: Direction,
    cards: Vec<CardView>,
    opening_look_slots: Option<[u32; 2]>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MatchFlip {
    revision: u64,
    actor_seat_id: u64,
    slots: Vec<u32>,
    newly_face_up_slots: Vec<u32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PendingPower {
    power_id: u64,
    actor_seat_id: u64,
    phase: String,
    own_slot: Option<u32>,
    target_slot: Option<u32>,
}

#[derive(Clone, Deserialize)]
struct Choice {
    action: String,
    slots: Vec<u32>,
    min: usize,
    max: usize,
}

#[derive(Deserialize)]
struct TestView {
    generation: u64,
}

#[derive(Deserialize)]
struct PrivateLook {
    slot: Option<u32>,
    rank: Option<u8>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MatchOutcome {
    source: String,
    incoming_slot: u32,
    penalty_slot: Option<u32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SceneSnapshot {
    room_id: String,
    deal_id: Option<u64>,
    revision: u64,
    role: String,
    phase: String,
    you_seat_id: Option<u64>,
    turn_seat_id: Option<u64>,
    players: Vec<PlayerView>,
    deck_count: usize,
    discard_top: Option<u8>,
    incoming: Option<CardView>,
    incoming_source: Option<String>,
    match_outcome: Option<MatchOutcome>,
    match_flip: Option<MatchFlip>,
    pending_power: Option<PendingPower>,
    private_power_result: Option<PrivateLook>,
    available_actions: Vec<Choice>,
    test: Option<TestView>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Frame {
    snapshot: Option<SceneSnapshot>,
    #[serde(skip)]
    selection: Vec<u32>,
    busy: bool,
    reduced_motion: bool,
    optimistic: Option<Command>,
}

#[derive(Resource, Default)]
struct Scene {
    frame: Option<Frame>,
    room: Option<(String, Option<u64>)>,
    seen_flip: Option<u64>,
    cards: HashMap<CardKey, Entity>,
    width: f32,
    revision: Option<u64>,
    owners: HashMap<u32, u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum CardKey {
    Hand(u32),
    Match(u32),
    Deck,
    Discard,
    PendingDraw,
}

#[derive(Clone, Copy)]
struct Placement {
    key: CardKey,
    rank: Option<u8>,
    position: Vec3,
    rotation: f32,
    size: f32,
    marked: bool,
    flip: bool,
    interactive: bool,
    home: Vec3,
    home_rotation: f32,
    home_size: f32,
    inspecting: bool,
}

#[derive(Component)]
struct SceneCard {
    placement: Placement,
    front: Entity,
    elapsed: f32,
    motion: Motion,
    exiting: bool,
}

impl SceneCard {
    fn flip_delay(&self) -> f32 {
        if self.placement.inspecting {
            self.motion.duration * 0.65
        } else {
            0.0
        }
    }
}

struct Motion {
    origin: Transform,
    elapsed: f32,
    duration: f32,
    lift: f32,
}

impl Motion {
    fn new(origin: Transform, target: Vec3) -> Self {
        let distance = origin.translation.distance(target);
        Self {
            origin,
            elapsed: 0.0,
            duration: (0.22 + distance * 0.035).min(0.58),
            lift: (distance * 0.11).min(0.65),
        }
    }
}

#[derive(Component)]
struct TableSurface;

#[derive(Resource)]
struct Materials {
    body: Handle<Mesh>,
    surface: Handle<Mesh>,
    edge: Handle<StandardMaterial>,
    gold: Handle<StandardMaterial>,
    back: Handle<StandardMaterial>,
    ranks: Vec<Handle<StandardMaterial>>,
}

pub fn enabled() -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::window()
            .and_then(|window| window.location().search().ok())
            .is_some_and(|query| {
                query
                    .split('&')
                    .any(|entry| entry.trim_start_matches('?') == "scene=3d")
            })
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        true
    }
}

pub fn plugin(app: &mut App) {
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(path) = std::env::args_os().nth(1) {
        let snapshot = std::fs::read(path)
            .ok()
            .and_then(|json| serde_json::from_slice(&json).ok());
        app.insert_resource(Scene {
            frame: Some(Frame {
                snapshot,
                selection: vec![],
                busy: false,
                reduced_motion: false,
                optimistic: None,
            }),
            ..default()
        });
    }
    app.init_resource::<Scene>()
        .add_systems(Startup, setup)
        .add_systems(Update, (reconcile, animate, pick).chain());
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        Camera3d::default(),
        Tonemapping::None,
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::FixedVertical {
                viewport_height: 12.0,
            },
            ..OrthographicProjection::default_3d()
        }),
        Transform::from_xyz(0.0, 16.0, 2.6).looking_at(Vec3::ZERO, Vec3::NEG_Z),
    ));
    commands.insert_resource(AmbientLight {
        color: Color::WHITE,
        brightness: 650.0,
        ..default()
    });
    commands.spawn((
        DirectionalLight {
            illuminance: 3500.0,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.9, -0.45, 0.0)),
    ));
    commands.spawn((
        TableSurface,
        Mesh3d(meshes.add(Cuboid::new(1.0, 0.26, 8.7))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.09, 0.23, 0.19),
            perceptual_roughness: 0.95,
            ..default()
        })),
        Transform::from_xyz(0.0, -0.21, 0.0),
        Visibility::Hidden,
    ));
    let edge = materials.add(Color::srgb(0.75, 0.62, 0.43));
    let gold = materials.add(Color::srgb(1.0, 0.68, 0.16));
    let back = materials.add(StandardMaterial {
        base_color_texture: Some(images.add(card_image(None))),
        unlit: true,
        ..default()
    });
    let ranks = (0..=13)
        .map(|rank| {
            materials.add(StandardMaterial {
                base_color_texture: Some(images.add(card_image(Some(rank)))),
                unlit: true,
                ..default()
            })
        })
        .collect();
    commands.insert_resource(Materials {
        body: meshes.add(Cuboid::new(1.0, 0.07, 1.4)),
        surface: meshes.add(Rectangle::new(1.0, 1.4)),
        edge,
        gold,
        back,
        ranks,
    });
}

fn card_image(rank: Option<u8>) -> Image {
    const W: usize = 80;
    const H: usize = 112;
    const DIGITS: [[u8; 5]; 10] = [
        [7, 5, 5, 5, 7],
        [2, 6, 2, 2, 7],
        [7, 1, 7, 4, 7],
        [7, 1, 7, 1, 7],
        [5, 5, 7, 1, 1],
        [7, 4, 7, 1, 7],
        [7, 4, 7, 5, 7],
        [7, 1, 1, 1, 1],
        [7, 5, 7, 5, 7],
        [7, 5, 7, 1, 7],
    ];
    let mut pixels = Vec::with_capacity(W * H * 4);
    for y in 0..H {
        for x in 0..W {
            let border = !(2..W - 2).contains(&x) || !(2..H - 2).contains(&y);
            let pixel = if border {
                [210, 183, 127, 255]
            } else if rank.is_some() {
                [246, 231, 199, 255]
            } else if (x + y) % 12 < 6 {
                [31, 72, 61, 255]
            } else {
                [43, 91, 73, 255]
            };
            pixels.extend(pixel);
        }
    }
    if let Some(rank) = rank {
        let digits = rank.to_string();
        let mut stamp = |left: usize, top: usize, scale: usize, reverse: bool| {
            let width = (digits.len() * 4 - 1) * scale;
            for (i, digit) in digits.bytes().enumerate() {
                for (y, bits) in DIGITS[(digit - b'0') as usize].iter().enumerate() {
                    for x in 0..3 {
                        if bits & (4 >> x) == 0 {
                            continue;
                        }
                        for dy in 0..scale {
                            for dx in 0..scale {
                                let px = (i * 4 + x) * scale + dx;
                                let py = y * scale + dy;
                                let px = left + if reverse { width - 1 - px } else { px };
                                let py = top + if reverse { 5 * scale - 1 - py } else { py };
                                let offset = (py * W + px) * 4;
                                pixels[offset..offset + 4].copy_from_slice(&[52, 37, 27, 255]);
                            }
                        }
                    }
                }
            }
        };
        stamp((W - (digits.len() * 4 - 1) * 7) / 2, (H - 35) / 2, 7, false);
        stamp(6, 7, 2, false);
        stamp(W - 6 - (digits.len() * 4 - 1) * 2, 7, 2, false);
        stamp(6, H - 17, 2, true);
        stamp(W - 6 - (digits.len() * 4 - 1) * 2, H - 17, 2, true);
    }
    Image::new(
        Extent3d {
            width: W as u32,
            height: H as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

fn layout(frame: &Frame, width: f32, new_flip: bool) -> Vec<Placement> {
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
                home + Vec3::Y
                    * if matching {
                        0.40
                    } else if selected {
                        0.20
                    } else {
                        0.0
                    }
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
                            action.action.as_str(),
                            "beginPlay" | "confirmSpy" | "confirmSwap"
                        ) && action.slots.contains(&card.slot)
                    }),
            });
        }
    };
    if is_board || mini || matches!(snapshot.phase.as_str(), "roundResults" | "gameOver") {
        let horizontal = (width * 0.34).min(7.0);
        for player in &snapshot.players {
            let (x, z, angle) = match player.direction {
                Direction::North => (0.0, -2.8, PI),
                Direction::East => (horizontal, 0.0, -FRAC_PI_2),
                Direction::South => (0.0, 2.8, 0.0),
                Direction::West => (-horizontal, 0.0, FRAC_PI_2),
            };
            let mut cards: Vec<_> = player.cards.iter().collect();
            if snapshot.turn_seat_id == Some(player.seat_id) {
                if let Some(incoming) = &snapshot.incoming {
                    cards.push(incoming);
                }
            }
            row(
                &cards,
                Vec3::new(x, 0.0, z),
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
                "drawDeck",
            ),
            (
                CardKey::Discard,
                snapshot.discard_top,
                0.75,
                snapshot.discard_top.is_some(),
                "drawDiscard",
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

fn reconcile(
    mut commands: Commands,
    mut scene: ResMut<Scene>,
    materials: Res<Materials>,
    window: Single<&Window>,
    mut cards: Query<
        (
            Entity,
            &mut SceneCard,
            &mut Transform,
            &mut MeshMaterial3d<StandardMaterial>,
        ),
        Without<TableSurface>,
    >,
    mut table: Single<(&mut Transform, &mut Visibility), With<TableSurface>>,
    mut faces: Query<&mut MeshMaterial3d<StandardMaterial>, Without<SceneCard>>,
) {
    #[allow(unused_mut)]
    let mut changed = false;
    #[cfg(target_arch = "wasm32")]
    if let Some(frame) = browser::take_frame() {
        let stale = frame
            .snapshot
            .as_ref()
            .zip(scene.frame.as_ref().and_then(|old| old.snapshot.as_ref()))
            .is_some_and(|(incoming, accepted)| {
                incoming.room_id == accepted.room_id && incoming.revision < accepted.revision
            });
        if !stale {
            scene.frame = Some(frame);
            changed = true;
        }
    }
    let width = window.width() / window.height().max(1.0) * 12.0;
    if !changed && (scene.width - width).abs() < 0.01 {
        return;
    }
    scene.width = width;
    let Some(frame) = scene.frame.as_ref() else {
        return;
    };
    let visible = frame
        .snapshot
        .as_ref()
        .is_some_and(|snapshot| snapshot.role == "board" && snapshot.phase != "lobby");
    *table.1 = if visible {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    table.0.scale.x = (width * 0.85).min(20.0);
    let room = frame
        .snapshot
        .as_ref()
        .map(|view| (view.room_id.clone(), view.deal_id));
    let flip = frame
        .snapshot
        .as_ref()
        .and_then(|view| view.match_flip.as_ref())
        .map(|f| f.revision);
    let reset = room != scene.room;
    let placements = layout(
        frame,
        width,
        flip.is_some() && (reset || flip != scene.seen_flip),
    );
    let view = frame.snapshot.as_ref();
    let incoming_slot = view.and_then(|snapshot| snapshot.incoming.as_ref().map(|card| card.slot));
    let draw_source =
        if view.and_then(|snapshot| snapshot.incoming_source.as_deref()) == Some("discard") {
            CardKey::Discard
        } else {
            CardKey::Deck
        };
    let match_outcome = view
        .and_then(|snapshot| snapshot.match_outcome.as_ref())
        .map(|result| {
            (
                result.source == "discardTop",
                result.incoming_slot,
                result.penalty_slot,
            )
        });
    let owners: HashMap<_, _> = view
        .into_iter()
        .flat_map(|view| &view.players)
        .flat_map(|player| player.cards.iter().map(|card| (card.slot, player.seat_id)))
        .collect();
    let revision = view.map(|snapshot| snapshot.revision);
    let changed_revision = revision != scene.revision;
    let deal = reset
        && view.is_some_and(|snapshot| snapshot.phase == "initialPreview")
        && scene
            .room
            .as_ref()
            .zip(room.as_ref())
            .is_some_and(|(from, to)| from.0 == to.0);
    let previous_transforms: HashMap<_, _> = scene
        .cards
        .iter()
        .filter_map(|(&key, &entity)| {
            cards
                .get(entity)
                .ok()
                .map(|(_, _, transform, _)| (key, *transform))
        })
        .collect();
    let previous_owners = std::mem::replace(&mut scene.owners, owners);
    if reset {
        for (entity, card, _, _) in &cards {
            if card.exiting {
                commands.entity(entity).despawn();
            }
        }
        for (_, entity) in scene.cards.drain() {
            commands.entity(entity).despawn();
        }
    }
    scene.room = room;
    scene.seen_flip = flip;
    scene.revision = revision;
    let mut old_cards = std::mem::take(&mut scene.cards);
    let has_pending = placements
        .iter()
        .any(|placement| placement.key == CardKey::PendingDraw);
    if !reset && !has_pending {
        let kept_hidden_draw = if changed_revision
            && incoming_slot.is_none()
            && old_cards.contains_key(&CardKey::PendingDraw)
        {
            placements.iter().find_map(|placement| match placement.key {
                CardKey::Hand(slot)
                    if scene.owners.contains_key(&slot) && !previous_owners.contains_key(&slot) =>
                {
                    Some(slot)
                }
                _ => None,
            })
        } else {
            None
        };
        if let Some(slot) = incoming_slot.or(kept_hidden_draw) {
            if let Some(entity) = old_cards.remove(&CardKey::PendingDraw) {
                old_cards.insert(CardKey::Hand(slot), entity);
            }
        }
    }
    let pile_position = |key| previous_transforms.get(&key).map(|t| t.translation);
    let discard_position = placements
        .iter()
        .find(|p| p.key == CardKey::Discard)
        .map(|p| p.position)
        .or_else(|| pile_position(CardKey::Discard))
        .unwrap_or(Vec3::new(0.75, 0.0, 0.0));
    for placement in placements {
        let material = if placement.marked {
            &materials.gold
        } else {
            &materials.edge
        };
        let face_material = placement
            .rank
            .and_then(|rank| materials.ranks.get(rank as usize))
            .unwrap_or(&materials.back);
        let entity = if let Some(entity) = old_cards.remove(&placement.key) {
            if let Ok((_, mut card, transform, mut edge)) = cards.get_mut(entity) {
                let opening = card.placement.rank.is_none() && placement.rank.is_some();
                let retained_flip = card.elapsed < 0.42 + card.flip_delay()
                    && card.placement.flip
                    && card.placement.rank == placement.rank;
                if card.placement.position != placement.position
                    || card.placement.rotation != placement.rotation
                    || card.placement.size != placement.size
                {
                    card.motion = Motion::new(*transform, placement.position);
                }
                card.placement = Placement {
                    flip: placement.flip || opening || retained_flip,
                    ..placement
                };
                if placement.flip || opening {
                    card.elapsed = 0.0;
                }
                edge.0 = material.clone();
                if let Ok(mut front) = faces.get_mut(card.front) {
                    front.0 = face_material.clone();
                }
            }
            entity
        } else {
            let surface = |material: Handle<StandardMaterial>, bottom: bool| {
                (
                    Mesh3d(materials.surface.clone()),
                    MeshMaterial3d(material),
                    Transform::from_xyz(0.0, if bottom { -0.036 } else { 0.036 }, 0.0)
                        .with_rotation(Quat::from_rotation_x(if bottom {
                            FRAC_PI_2
                        } else {
                            -FRAC_PI_2
                        })),
                )
            };
            let front = commands.spawn(surface(face_material.clone(), false)).id();
            let back = commands.spawn(surface(materials.back.clone(), true)).id();
            let source = match placement.key {
                CardKey::PendingDraw => Some(CardKey::Deck),
                CardKey::Hand(_) if deal => Some(CardKey::Deck),
                CardKey::Hand(slot) if changed_revision && incoming_slot == Some(slot) => {
                    Some(draw_source)
                }
                CardKey::Hand(slot) if changed_revision => {
                    match_outcome.and_then(|(from_discard, incoming, penalty)| {
                        if penalty == Some(slot) {
                            Some(CardKey::Deck)
                        } else if from_discard && incoming == slot {
                            Some(CardKey::Discard)
                        } else {
                            None
                        }
                    })
                }
                _ => None,
            };
            let origin = source.and_then(pile_position).unwrap_or_else(|| {
                if deal {
                    Vec3::new(-0.75, 0.0, 0.0)
                } else {
                    placement.position
                }
            });
            let transform = Transform::from_translation(origin)
                .with_rotation(Quat::from_rotation_y(placement.rotation))
                .with_scale(Vec3::splat(placement.size));
            let mut motion = Motion::new(transform, placement.position);
            if deal {
                if matches!(placement.key, CardKey::Hand(_)) {
                    motion.elapsed = -0.035 * (scene.cards.len() as f32);
                }
            }
            commands
                .spawn((
                    SceneCard {
                        placement,
                        front,
                        elapsed: 0.0,
                        motion,
                        exiting: false,
                    },
                    Mesh3d(materials.body.clone()),
                    MeshMaterial3d(material.clone()),
                    transform,
                ))
                .add_children(&[front, back])
                .id()
        };
        scene.cards.insert(placement.key, entity);
    }
    for (key, entity) in old_cards {
        let Ok((_, mut card, transform, _)) = cards.get_mut(entity) else {
            continue;
        };
        let destination = if card.placement.inspecting
            && matches!(key, CardKey::Hand(slot) if scene.owners.contains_key(&slot))
        {
            Some(card.placement.home)
        } else if !reset && changed_revision {
            match key {
                CardKey::Hand(slot) | CardKey::Match(slot) if !scene.owners.contains_key(&slot) => {
                    Some(discard_position)
                }
                CardKey::PendingDraw if revision.is_some() && incoming_slot.is_none() => {
                    Some(discard_position)
                }
                CardKey::Hand(slot) => scene
                    .owners
                    .get(&slot)
                    .zip(previous_owners.get(&slot))
                    .and_then(|(new, old)| {
                        if new == old {
                            return None;
                        }
                        scene.owners.iter().find_map(|(&other, owner)| {
                            (owner == old && previous_owners.get(&other) == Some(new))
                                .then(|| {
                                    previous_transforms
                                        .get(&CardKey::Hand(other))
                                        .map(|t| t.translation)
                                })
                                .flatten()
                        })
                    }),
                _ => None,
            }
        } else {
            None
        };
        if let Some(target) = destination {
            if let Ok(mut front) = faces.get_mut(card.front) {
                front.0 = materials.back.clone();
            }
            card.placement.position = target;
            card.placement.rank = None;
            card.placement.flip = false;
            card.placement.interactive = false;
            card.placement.rotation = if card.placement.inspecting {
                card.placement.home_rotation
            } else {
                0.0
            };
            card.placement.size = if card.placement.inspecting {
                card.placement.home_size
            } else {
                0.9
            };
            card.placement.inspecting = false;
            card.motion = Motion::new(*transform, target);
            card.exiting = true;
        } else {
            commands.entity(entity).despawn();
        }
    }
}

fn animate(
    mut commands: Commands,
    time: Res<Time>,
    scene: Res<Scene>,
    mut cards: Query<(Entity, &mut SceneCard, &mut Transform)>,
) {
    let reduced = scene
        .frame
        .as_ref()
        .is_some_and(|frame| frame.reduced_motion);
    for (entity, mut card, mut transform) in &mut cards {
        let target = card.placement.position;
        card.motion.elapsed += time.delta_secs();
        let t = if reduced {
            1.0
        } else {
            (card.motion.elapsed / card.motion.duration).clamp(0.0, 1.0)
        };
        let ease = t * t * (3.0 - 2.0 * t);
        let arc = (PI * t).sin() * card.motion.lift;
        let delta = target - card.motion.origin.translation;
        let side = Vec3::new(delta.z, 0.0, -delta.x).normalize_or_zero();
        transform.translation =
            card.motion.origin.translation.lerp(target, ease) + (Vec3::Y + side * 0.32) * arc;
        transform.scale = card
            .motion
            .origin
            .scale
            .lerp(Vec3::splat(card.placement.size), ease);
        let yaw = Quat::from_rotation_y(card.placement.rotation);
        if card.placement.flip && !reduced {
            let flip_delay = card.flip_delay();
            card.elapsed += time.delta_secs();
            let angle = PI * (1.0 - ((card.elapsed - flip_delay) / 0.42).clamp(0.0, 1.0));
            transform.rotation = yaw * Quat::from_rotation_z(angle);
        } else {
            transform.rotation = card.motion.origin.rotation.slerp(yaw, ease);
        }
        if card.exiting && t >= 1.0 {
            commands.entity(entity).despawn();
        }
    }
}

fn pick(camera: Single<(&Camera, &GlobalTransform), With<Camera3d>>, cards: Query<&SceneCard>) {
    #[cfg(target_arch = "wasm32")]
    if let Some(point) = browser::take_pick() {
        let (camera, transform) = *camera;
        let Ok(ray) = camera.viewport_to_world(transform, point) else {
            return;
        };
        for card in &cards {
            let p = card.placement;
            if !p.interactive {
                continue;
            }
            let Some(distance) = ray.intersect_plane(p.position, InfinitePlane3d::new(Vec3::Y))
            else {
                continue;
            };
            let delta = Quat::from_rotation_y(-p.rotation) * (ray.get_point(distance) - p.position);
            if delta.x.abs() <= p.size * 0.5 && delta.z.abs() <= p.size * 0.7 {
                browser::emit_pick(p.key);
                break;
            }
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = (camera, cards);
}

#[cfg(target_arch = "wasm32")]
mod browser {
    use super::*;
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
            choice.action.as_str(),
            "revealInitialCards" | "discardHandCard" | "callMatch" | "peek"
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
                .find(|choice| choice.action == action)?;
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
                            .find(|choice| {
                                matches!(
                                    choice.action.as_str(),
                                    "stagePowerTarget" | "selectPowerOwnCard"
                                ) && choice.slots.contains(&slot)
                            })
                            .map(|choice| choice.action.clone())
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
}
