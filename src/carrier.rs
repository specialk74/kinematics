use bevy::prelude::*;
use rand::prelude::*;

use crate::WORK_AREA_LEFT;
use crate::gate::{Gate, blocks_circle};

pub const BELT_SPEED: f32 = 100.0;
pub const CARRIER_DIVERT_SPEED: f32 = 50.0;
pub const CARRIER_RADIUS: f32 = 15.0;
pub const CARRIER_THICKNESS: f32 = 3.0;
/// Distanza minima fra i centri di due carrier: diametro piu' un po' di margine.
pub const CARRIER_SIZE: f32 = CARRIER_RADIUS * 2.0 + 4.0;

#[derive(PartialEq)]
pub enum CarrierType {
    Empty,
    WithTube,
}

#[derive(Component)]
pub struct Carrier(pub CarrierType);

#[derive(Component)]
pub struct Tube;

pub struct CarrierPlugin;

impl Plugin for CarrierPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_carrier_assets)
            .add_systems(Update, (move_carrier, despawn_offscreen));
    }
}

/// Mesh e materiali dei carrier, creati una volta sola: se venissero costruiti a
/// ogni spawn si accumulerebbero asset identici per tutta la durata del gioco.
#[derive(Resource)]
pub struct CarrierAssets {
    empty_mesh: Handle<Mesh>,
    empty_material: Handle<ColorMaterial>,
    with_tube_mesh: Handle<Mesh>,
    with_tube_material: Handle<ColorMaterial>,
}

fn setup_carrier_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
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
}

/// Immette un carrier nel flusso; il tipo (vuoto o con tubo) e' casuale 50-50.
pub fn spawn_random_carrier(commands: &mut Commands, assets: &CarrierAssets, position: Vec3) {
    let mut rng = rand::rng();
    if rng.random::<u32>() > u32::MAX / 2 {
        commands.spawn((
            Mesh2d(assets.with_tube_mesh.clone()),
            MeshMaterial2d(assets.with_tube_material.clone()),
            Transform::from_translation(position),
            Carrier(CarrierType::WithTube),
            children![Tube],
        ));
    } else {
        commands.spawn((
            Mesh2d(assets.empty_mesh.clone()),
            MeshMaterial2d(assets.empty_material.clone()),
            Transform::from_translation(position),
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

fn move_carrier(
    time: Res<Time>,
    mut query: Query<(Entity, &Carrier, &mut Transform)>,
    gates: Query<(&Gate, &Transform), Without<Carrier>>,
) {
    let delta_secs = time.delta_secs();

    let active_gates: Vec<Vec3> = gates
        .iter()
        .filter(|(gate, _)| gate.active)
        .map(|(_, transform)| transform.translation)
        .collect();

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
        })
        // Un gate attivo ferma il carrier subito prima di toccarlo. Chi lo stava
        // gia' attraversando quando il gate e' stato attivato finisce il transito
        // invece di restare incastrato dentro la sbarra.
            || active_gates.iter().any(|gate| {
                blocks_circle(*gate, candidate, CARRIER_RADIUS)
                    && !blocks_circle(*gate, translation, CARRIER_RADIUS)
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
        // Fine corsa: il carrier e' scomparso sotto la barra degli strumenti.
        if transform.translation.x + CARRIER_RADIUS < WORK_AREA_LEFT {
            commands.entity(entity).despawn();
            continue;
        }

        if let Some(pos) = camera.world_to_ndc(camera_transform, transform.translation) {
            // Add a buffer (e.g., 1.2 instead of 1.0) to hide things before they pop out
            if pos.x.abs() > 1.2 || pos.y.abs() > 1.2 {
                commands.entity(entity).despawn();
            }
        }
    }
}
