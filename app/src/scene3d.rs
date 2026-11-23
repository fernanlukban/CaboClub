use std::{
    collections::HashMap,
    f32::consts::{FRAC_PI_2, PI},
};

use bevy::{camera::ScalingMode, core_pipeline::tonemapping::Tonemapping, prelude::*};
use cabo_shared::protocol::CardPile;

#[cfg(target_arch = "wasm32")]
mod browser;
mod frame;
mod layout;
mod textures;

use frame::Frame;
use layout::layout;
use textures::card_image;

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
    returning_focus: bool,
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
    #[cfg(target_arch = "wasm32")]
    app.add_systems(Update, browser::publish_layout.after(animate));
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
    let card_travel = view
        .and_then(|snapshot| snapshot.card_travel)
        .filter(|travel| scene.revision.is_some_and(|seen| seen < travel.revision));
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
    // Returning the old focus and entering the new focus are two motions in one transition.
    let focus_delay = if !reset && placements.iter().any(|placement| placement.inspecting) {
        placements
            .iter()
            .filter(|placement| !placement.inspecting)
            .filter_map(|placement| {
                let entity = old_cards.get(&placement.key)?;
                let (_, card, transform, _) = cards.get(*entity).ok()?;
                if card.placement.inspecting
                    && transform.translation.distance(placement.position) >= 0.05
                {
                    Some(Motion::new(*transform, placement.position).duration)
                } else if card.returning_focus {
                    Some((card.motion.duration - card.motion.elapsed).max(0.0))
                } else {
                    None
                }
            })
            .fold(0.0_f32, f32::max)
    } else {
        0.0
    };
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
                    card.returning_focus = !placement.inspecting
                        && (card.placement.inspecting || card.returning_focus);
                    if placement.inspecting && !card.placement.inspecting {
                        card.motion.elapsed = -focus_delay;
                    }
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
                CardKey::PendingDraw if !reset => Some(CardKey::Deck),
                CardKey::Hand(_) if deal => Some(CardKey::Deck),
                CardKey::Hand(slot)
                    if !reset && changed_revision && incoming_slot == Some(slot) =>
                {
                    Some(draw_source)
                }
                CardKey::Hand(slot) if !reset && changed_revision => card_travel
                    .filter(|travel| {
                        travel.slot == slot && scene.owners.get(&slot) == Some(&travel.to_seat_id)
                    })
                    .map(|travel| match travel.from {
                        CardPile::Discard => CardKey::Discard,
                    })
                    .or_else(|| {
                        match_outcome.and_then(|(from_discard, incoming, penalty)| {
                            if penalty == Some(slot) {
                                Some(CardKey::Deck)
                            } else if from_discard && incoming == slot {
                                Some(CardKey::Discard)
                            } else {
                                None
                            }
                        })
                    }),
                _ => None,
            };
            let origin = source.and_then(pile_position).unwrap_or_else(|| {
                if source == Some(CardKey::Discard) {
                    discard_position
                } else if deal {
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
                        returning_focus: false,
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
            if card.motion.elapsed >= 0.0 {
                card.elapsed += time.delta_secs();
            }
            let angle = PI * (1.0 - ((card.elapsed - flip_delay) / 0.42).clamp(0.0, 1.0));
            transform.rotation = yaw * Quat::from_rotation_z(angle);
        } else {
            transform.rotation = card.motion.origin.rotation.slerp(yaw, ease);
        }
        if t >= 1.0 {
            card.returning_focus = false;
            if card.exiting {
                commands.entity(entity).despawn();
            }
        }
    }
}

fn pick(
    camera: Single<(&Camera, &GlobalTransform), With<Camera3d>>,
    cards: Query<(&SceneCard, &Transform)>,
) {
    #[cfg(target_arch = "wasm32")]
    if let Some(point) = browser::take_pick() {
        let (camera, transform) = *camera;
        let Ok(ray) = camera.viewport_to_world(transform, point) else {
            return;
        };
        let nearest = cards
            .iter()
            .filter(|(card, _)| card.placement.interactive)
            .filter_map(|(card, mesh)| {
                let normal = mesh.rotation * Vec3::Y;
                let distance =
                    ray.intersect_plane(mesh.translation, InfinitePlane3d::new(normal))?;
                let local = mesh.rotation.inverse() * (ray.get_point(distance) - mesh.translation);
                (local.x.abs() <= mesh.scale.x * 0.5 && local.z.abs() <= mesh.scale.z * 0.7)
                    .then_some((distance, card.placement.key))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0));
        if let Some((_, key)) = nearest {
            browser::emit_pick(key);
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = (camera, cards);
}
