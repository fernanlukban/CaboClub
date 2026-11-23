use bevy::prelude::*;

mod scene3d;

fn main() {
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "Cabo Club".into(),
            canvas: Some("#table".into()),
            fit_canvas_to_parent: true,
            ..default()
        }),
        ..default()
    }))
    .insert_resource(ClearColor(Color::NONE));
    if scene3d::enabled() {
        app.add_plugins(scene3d::plugin);
    } else {
        app.add_systems(Startup, setup).add_plugins(card_scene);
    }
    app.run();
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);
}

#[cfg(not(target_arch = "wasm32"))]
fn card_scene(_: &mut App) {}

#[cfg(target_arch = "wasm32")]
fn card_scene(app: &mut App) {
    app.add_systems(Update, browser::sync_cards);
}

#[cfg(target_arch = "wasm32")]
mod browser {
    use std::collections::{HashMap, HashSet};

    use bevy::prelude::*;
    use wasm_bindgen::JsCast;

    #[derive(Component)]
    pub(super) struct CardSprite {
        key: String,
        origin: Vec2,
        target: Vec2,
        elapsed: f32,
    }

    pub(super) fn sync_cards(
        mut commands: Commands,
        time: Res<Time>,
        mut sprites: Query<(Entity, &mut CardSprite, &mut Transform, &mut Sprite)>,
        mut camera: Query<&mut Projection, With<Camera2d>>,
    ) {
        let Some(browser) = web_sys::window() else {
            return;
        };
        let Some(document) = browser.document() else {
            return;
        };
        let Some(canvas) = document.get_element_by_id("table") else {
            return;
        };
        let canvas = canvas.get_bounding_client_rect();
        let reduced = browser
            .match_media("(prefers-reduced-motion: reduce)")
            .ok()
            .flatten()
            .is_some_and(|setting| setting.matches());
        let active = document
            .query_selector(".phone.active")
            .ok()
            .flatten()
            .is_some();
        let mut camera_projection = camera.single_mut().ok();
        let Some(Projection::Orthographic(projection)) = camera_projection.as_deref_mut() else {
            return;
        };
        let desired = if active { 1.09 } else { 1.0 };
        let blend = if reduced {
            1.0
        } else {
            (time.delta_secs() * 7.0).min(1.0)
        };
        projection.scale += (desired - projection.scale) * blend;
        let zoom = projection.scale;
        let Ok(nodes) = document.query_selector_all(".card[data-sprite]") else {
            return;
        };
        let mut visible = HashMap::new();
        for i in 0..nodes.length() {
            let Some(element) = nodes
                .item(i)
                .and_then(|node| node.dyn_into::<web_sys::Element>().ok())
            else {
                continue;
            };
            let Some(key) = element.get_attribute("data-sprite") else {
                continue;
            };
            let rect = element.get_bounding_client_rect();
            if rect.width() <= 0.0 || rect.height() <= 0.0 {
                continue;
            }
            let center = Vec2::new(
                (rect.left() - canvas.left() + (rect.width() - canvas.width()) * 0.5) as f32,
                (canvas.top() - rect.top() + (canvas.height() - rect.height()) * 0.5) as f32,
            );
            let size = Vec2::new(rect.width() as f32, rect.height() as f32);
            visible.insert(key, (center, size, i));
        }

        let mut drawn = HashSet::new();
        for (entity, mut card, mut transform, mut sprite) in &mut sprites {
            let Some(&(center, size, index)) = visible.get(&card.key) else {
                commands.entity(entity).despawn();
                continue;
            };
            drawn.insert(card.key.clone());
            if card.target.distance(center) > 3.0 {
                card.origin = transform.translation.truncate() / zoom;
                card.target = center;
                card.elapsed = 0.0;
            }
            card.elapsed += time.delta_secs();
            let t = if reduced {
                1.0
            } else {
                (card.elapsed / 0.35).min(1.0)
            };
            let eased = t * t * (3.0 - 2.0 * t);
            let float = if reduced {
                0.0
            } else {
                (time.elapsed_secs() * 1.7 + index as f32 * 0.5).sin() * 2.0
            };
            let offset = Vec2::new(0.0, float - 4.0);
            transform.translation =
                ((card.origin.lerp(card.target, eased) + offset) * zoom).extend(0.0);
            transform.rotation = Quat::from_rotation_z(float * 0.009);
            sprite.custom_size = Some(size * zoom);
        }

        for (key, (center, size, _)) in visible {
            if drawn.contains(&key) {
                continue;
            }
            commands.spawn((
                CardSprite {
                    key,
                    origin: center,
                    target: center,
                    elapsed: 1.0,
                },
                Sprite::from_color(Color::srgba(0.02, 0.09, 0.07, 0.72), size * zoom),
                Transform::from_translation((center * zoom).extend(0.0)),
            ));
        }
    }
}
