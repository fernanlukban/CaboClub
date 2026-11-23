use bevy::prelude::*;

#[derive(Component)]
struct DemoCard;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Cabo Club".into(),
                canvas: Some("#table".into()),
                fit_canvas_to_parent: true,
                ..default()
            }),
            ..default()
        }))
        .insert_resource(ClearColor(Color::srgb(0.04, 0.17, 0.14)))
        .add_systems(Startup, setup)
        .add_systems(Update, flip_cards)
        .run();
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);
    for x in [-96.0, -32.0, 32.0, 96.0] {
        commands.spawn((
            DemoCard,
            Sprite::from_color(Color::srgb(0.22, 0.49, 0.41), Vec2::new(52.0, 76.0)),
            Transform::from_xyz(x, 0.0, 0.0),
        ));
    }
}

fn flip_cards(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    touches: Res<Touches>,
    mut cards: Query<&mut Sprite, With<DemoCard>>,
    mut revealed: Local<bool>,
) {
    if keys.just_pressed(KeyCode::Space)
        || mouse.just_pressed(MouseButton::Left)
        || touches.iter_just_pressed().next().is_some()
    {
        *revealed = !*revealed;
        let color = if *revealed {
            Color::srgb(0.96, 0.90, 0.76)
        } else {
            Color::srgb(0.22, 0.49, 0.41)
        };
        for mut sprite in &mut cards {
            sprite.color = color;
        }
    }
}
