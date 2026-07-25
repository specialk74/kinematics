use bevy::prelude::*;
use rand::prelude::*;

pub const CARRIER_SPAWN_TIME: f32 = 0.500;
pub const BELT_SPEED: f32 = 100.0;
pub const CARRIER_DIVERT_SPEED: f32 = 50.0;
pub const CARRIER_RADIUS: f32 = 15.0;
pub const CARRIER_THICKNESS: f32 = 3.0;
pub const CARRIER_SIZE: f32 = CARRIER_RADIUS * 2.0 + 4.0;
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

/// Mesh e materiali dei carrier, creati una volta sola: se venissero costruiti a
/// ogni spawn si accumulerebbero asset identici per tutta la durata del gioco.
#[derive(Resource)]
struct CarrierAssets {
    empty_mesh: Handle<Mesh>,
    empty_material: Handle<ColorMaterial>,
    with_tube_mesh: Handle<Mesh>,
    with_tube_material: Handle<ColorMaterial>,
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    commands.spawn(Camera2d);

    commands.insert_resource(CarrierAssets {
        // Il carrier vuoto e' un anello: cerchio senza riempimento.
        empty_mesh: meshes.add(Annulus::new(
            CARRIER_RADIUS - CARRIER_THICKNESS,
            CARRIER_RADIUS,
        )),
        empty_material: materials.add(Color::WHITE),
        with_tube_mesh: meshes.add(Circle::new(CARRIER_RADIUS)),
        with_tube_material: materials.add(Color::srgb(0.0, 0.8, 0.2)),
    });

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

fn spawn_carrier(
    mut commands: Commands,
    timer: Res<CarrierSpawnTimer>,
    carriers: Query<&Transform, With<Carrier>>,
    carrier_assets: Res<CarrierAssets>,
) {
    if !timer.is_finished() {
        return;
    }

    let spawn = Vec3 {
        x: WIDTH as f32 / 2.0 - 50.0,
        y: 0.0,
        z: 0.0,
    };

    // Non far entrare un carrier sopra a uno ancora fermo in coda all'ingresso.
    if carriers
        .iter()
        .any(|transform| transform.translation.distance(spawn) < CARRIER_SIZE)
    {
        return;
    }

    let mut rng = rand::rng();
    if rng.random::<u32>() > u32::MAX / 2 {
        commands.spawn((
            Mesh2d(carrier_assets.with_tube_mesh.clone()),
            MeshMaterial2d(carrier_assets.with_tube_material.clone()),
            Transform::from_translation(spawn),
            Carrier(CarrierType::WithTube),
            children![Tube],
        ));
    } else {
        commands.spawn((
            Mesh2d(carrier_assets.empty_mesh.clone()),
            MeshMaterial2d(carrier_assets.empty_material.clone()),
            Transform::from_translation(spawn),
            Carrier(CarrierType::Empty),
        ));
    }
}

fn carrier_velocity(carrier: &Carrier, translation: Vec3) -> Vec3 {
    if carrier.0 == CarrierType::WithTube {
        let pos = translation.x.abs();
        if pos < 16.0 {
            Vec3::new(-CARRIER_DIVERT_SPEED, BELT_SPEED, 0.0)
        } else if translation.x < -300.0 && translation.x > -(300.0 + 32.0) {
            Vec3::new(-CARRIER_DIVERT_SPEED, -BELT_SPEED, 0.0)
        } else {
            Vec3::new(-BELT_SPEED, 0.0, 0.0)
        }
    } else {
        Vec3::new(-BELT_SPEED, 0.0, 0.0)
    }
}

fn move_carrier(time: Res<Time>, mut query: Query<(Entity, &Carrier, &mut Transform)>) {
    let delta_secs = time.delta_secs();

    // Si risolve un carrier alla volta partendo da quello piu' avanti sul nastro:
    // chi si ferma deve bloccare a cascata tutti quelli che ha dietro.
    let mut belt: Vec<(Entity, Vec3, Vec3)> = query
        .iter()
        .map(|(entity, carrier, transform)| {
            let translation = transform.translation;
            (
                entity,
                translation,
                carrier_velocity(carrier, translation) * delta_secs,
            )
        })
        .collect();
    belt.sort_by(|a, b| a.1.x.total_cmp(&b.1.x));

    let mut resolved: Vec<(Entity, Vec3)> = Vec::with_capacity(belt.len());
    for (entity, translation, step) in belt {
        let candidate = translation + step;
        // Il passo viene annullato solo se avvicina il carrier a uno gia' troppo
        // vicino: cosi' chi e' sovrapposto puo' comunque allontanarsi.
        let blocked = resolved.iter().any(|(_, ahead)| {
            let gap = candidate.distance(*ahead);
            gap < CARRIER_SIZE && gap < translation.distance(*ahead)
        });
        resolved.push((entity, if blocked { translation } else { candidate }));
    }

    for (entity, translation) in resolved {
        if let Ok((_, _, mut transform)) = query.get_mut(entity) {
            transform.translation = translation;
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
