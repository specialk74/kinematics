use bevy::prelude::*;
use rand::prelude::*;

pub const CARRIER_SPAWN_TIME: f32 = 0.500;
pub const BELT_SPEED: f32 = 100.0;
pub const CARRIER_DIVERT_SPEED: f32 = 50.0;
pub const WIDTH: u32 = 1024;
pub const HEIGTH: u32 = 768;

#[derive(PartialEq)]
pub enum CarrierType {
    Empty,
    WithTube,
}

#[derive(Component)]
struct Carrier(CarrierType);

#[derive(Component)]
struct Tube;

#[derive(PartialEq)]
pub enum DivertType {
    Std,
    Nsd,
}

#[derive(Component)]
struct Divert(DivertType);

fn main() {
    App::new()
        .init_resource::<CarrierSpawnTimer>()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Carrier Flow".to_string(),
                resolution: (WIDTH, HEIGTH).into(),
                ..Default::default()
            }),
            ..Default::default()
        }))
        .add_systems(Startup, setup)
        .add_systems(Update, move_carrier)
        .add_systems(Update, tick_carrier_spawn_timer)
        .add_systems(Update, spawn_carrier)
        .add_systems(Update, despawn_offscreen)
        .run();
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);

    commands.spawn((
        Text2d::new("@"),
        TextFont {
            font_size: 12.0,
            font: default(),
            ..default()
        },
        TextColor(Color::WHITE),
        Transform::from_translation(Vec3::ZERO),
        Divert(DivertType::Nsd),
    ));
}

#[derive(Resource, Deref, DerefMut)]
pub struct CarrierSpawnTimer(Timer);

impl Default for CarrierSpawnTimer {
    fn default() -> Self {
        CarrierSpawnTimer(Timer::from_seconds(
            CARRIER_SPAWN_TIME,
            TimerMode::Repeating,
        ))
    }
}

pub fn tick_carrier_spawn_timer(mut timer: ResMut<CarrierSpawnTimer>, time: Res<Time>) {
    timer.tick(time.delta());
}

fn spawn_carrier(mut commands: Commands, timer: Res<CarrierSpawnTimer>) {
    if !timer.is_finished() {
        return;
    }

    let mut rng = rand::rng();
    if rng.random::<u32>() > u32::MAX / 2 {
        commands.spawn((
            Text2d::new("O"),
            TextFont {
                font_size: 50.0,
                ..default()
            },
            Transform::from_translation(Vec3 {
                x: WIDTH as f32 / 2.0 - 50.0,
                y: 0.0,
                z: 0.0,
            }),
            TextColor(Color::srgb(1.0, 0.0, 1.0)),
            Carrier(CarrierType::WithTube),
            children![Tube],
        ));
    } else {
        commands.spawn((
            Text2d::new("<"),
            TextFont {
                font_size: 50.0,
                ..default()
            },
            Transform::from_translation(Vec3 {
                x: WIDTH as f32 / 2.0 - 50.0,
                y: 0.0,
                z: 0.0,
            }),
            TextColor(Color::srgb(1.0, 1.0, 0.0)),
            Carrier(CarrierType::Empty),
        ));
    }
}

fn move_carrier(time: Res<Time>, query: Query<(&Carrier, &mut Transform)>) {
    for (carrier, mut transform) in query {
        if carrier.0 == CarrierType::WithTube {
            let pos = transform.translation.x.abs();
            //println!("pos: {pos}");
            if pos < 16.0 {
                transform.translation.y += BELT_SPEED * time.delta_secs();
                transform.translation.x -= CARRIER_DIVERT_SPEED * time.delta_secs();
            } else if transform.translation.x < -300.0 && transform.translation.x > -(300.0 + 32.0)
            {
                transform.translation.y -= BELT_SPEED * time.delta_secs();
                transform.translation.x -= CARRIER_DIVERT_SPEED * time.delta_secs();
            } else {
                transform.translation.x -= BELT_SPEED * time.delta_secs();
            }
        } else {
            transform.translation.x -= BELT_SPEED * time.delta_secs();
        }
    }
}

fn despawn_offscreen(
    mut commands: Commands,
    query: Query<(Entity, &Transform), With<Carrier>>,
    camera_query: Query<(&Camera, &GlobalTransform)>,
) {
    let (camera, camera_transform) = camera_query.single().unwrap();
    for (entity, transform) in query.iter() {
        if let Some(pos) = camera.world_to_ndc(camera_transform, transform.translation) {
            // Add a buffer (e.g., 1.2 instead of 1.0) to hide things before they pop out
            if pos.x.abs() > 1.2 || pos.y.abs() > 1.2 {
                commands.entity(entity).despawn();
            }
        }
    }
}
