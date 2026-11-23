use bevy::prelude::*;

mod scene3d;

pub fn run() {
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
    app.add_systems(Update, flat::sync_cards);
}

#[cfg(target_arch = "wasm32")]
mod flat;
