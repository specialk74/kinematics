use std::f32::consts::FRAC_PI_2;

use bevy::prelude::*;

use crate::carrier::{CARRIER_SIZE, Carrier, spawn_random_carrier};
use crate::simulation::SimulationState;

pub const CARRIER_SPAWN_TIME: f32 = 0.500;
pub const SOURCE_SIZE: f32 = 34.0;

/// Oggetto che immette carrier nel flusso. Ogni sorgente ha il proprio timer,
/// cosi' due sorgenti piazzate in momenti diversi restano indipendenti.
#[derive(Component)]
pub struct CarrierSource {
    timer: Timer,
}

pub struct SourcePlugin;

impl Plugin for SourcePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            // In pausa i timer non avanzano nemmeno: al play il flusso riprende
            // da dov'era invece di recuperare tutto il tempo fermo.
            spawn_from_sources.run_if(in_state(SimulationState::Running)),
        );
    }
}

pub struct SourceVisualsPlugin;

impl Plugin for SourceVisualsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_source_assets)
            .add_systems(Update, attach_source_visuals);
    }
}

#[derive(Resource)]
pub struct SourceAssets {
    mesh: Handle<Mesh>,
    material: Handle<ColorMaterial>,
}

fn setup_source_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    commands.insert_resource(SourceAssets {
        mesh: meshes.add(RegularPolygon::new(SOURCE_SIZE / 2.0, 3)),
        material: materials.add(Color::srgb(0.2, 0.5, 1.0)),
    });
}

/// Mesh e orientamento della sorgente: il triangolo punta a sinistra, nel verso
/// in cui partono i carrier. La usa anche l'anteprima dell'editor, cosi' quello
/// che si vede prima del clic non puo' discostarsi da quello che viene piazzato.
pub fn shape(assets: &SourceAssets) -> (Handle<Mesh>, Quat) {
    (assets.mesh.clone(), Quat::from_rotation_z(FRAC_PI_2))
}

fn attach_source_visuals(
    mut commands: Commands,
    assets: Res<SourceAssets>,
    sources: Query<(Entity, &mut Transform), (With<CarrierSource>, Without<Mesh2d>)>,
) {
    let (mesh, rotation) = shape(&assets);

    for (entity, mut transform) in sources {
        transform.rotation = rotation;
        commands.entity(entity).insert((
            Mesh2d(mesh.clone()),
            MeshMaterial2d(assets.material.clone()),
        ));
    }
}

pub fn spawn_source(commands: &mut Commands, position: Vec3) -> Entity {
    commands
        .spawn((
            Transform::from_translation(position),
            CarrierSource {
                timer: Timer::from_seconds(CARRIER_SPAWN_TIME, TimerMode::Repeating),
            },
        ))
        .id()
}

fn spawn_from_sources(
    mut commands: Commands,
    time: Res<Time>,
    mut sources: Query<(&mut CarrierSource, &Transform)>,
    carriers: Query<&Transform, With<Carrier>>,
) {
    for (mut source, transform) in sources.iter_mut() {
        source.timer.tick(time.delta());
        if !source.timer.is_finished() {
            continue;
        }

        // I carrier vivono sul piano z = 0, la sorgente e' disegnata piu' avanti.
        let position = transform.translation.with_z(0.0);

        // Non far entrare un carrier sopra a uno ancora fermo davanti alla sorgente.
        if carriers
            .iter()
            .any(|carrier| carrier.translation.distance(position) < CARRIER_SIZE)
        {
            continue;
        }

        spawn_random_carrier(&mut commands, position);
    }
}
