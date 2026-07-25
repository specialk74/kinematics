use bevy::prelude::*;

use crate::carrier::{CARRIER_SIZE, Carrier, NextCarrierId, spawn_random_carrier};
use crate::piece::{self, Arrow, Facing, PieceShapes};
use crate::simulation::SimulationState;

pub const CARRIER_SPAWN_TIME: f32 = 0.500;

/// Oggetto che immette carrier nel flusso. Ogni sorgente ha il proprio timer,
/// cosi' due sorgenti piazzate in momenti diversi restano indipendenti.
#[derive(Component)]
pub struct CarrierSource {
    timer: Timer,
    pub active: bool,
}

impl CarrierSource {
    /// Riazzera l'attesa: il prossimo carrier esce dopo un intervallo intero,
    /// non subito perche' il timer era gia' quasi scaduto.
    pub fn restart(&mut self) {
        self.timer.reset();
    }
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
            .add_systems(Update, (attach_source_visuals, refresh_source_colour));
    }
}

#[derive(Resource)]
pub struct SourceAssets {
    active_material: Handle<ColorMaterial>,
    idle_material: Handle<ColorMaterial>,
}

fn setup_source_assets(mut commands: Commands, mut materials: ResMut<Assets<ColorMaterial>>) {
    commands.insert_resource(SourceAssets {
        active_material: materials.add(Color::srgb(0.2, 0.5, 1.0)),
        idle_material: materials.add(Color::srgb(0.3, 0.3, 0.3)),
    });
}

fn material_for(assets: &SourceAssets, active: bool) -> Handle<ColorMaterial> {
    if active {
        assets.active_material.clone()
    } else {
        assets.idle_material.clone()
    }
}

fn attach_source_visuals(
    mut commands: Commands,
    shapes: Res<PieceShapes>,
    assets: Res<SourceAssets>,
    sources: Query<(Entity, &CarrierSource), Without<Mesh2d>>,
) {
    for (entity, source) in sources.iter() {
        piece::dress(
            &mut commands,
            entity,
            &shapes,
            material_for(&assets, source.active),
            Arrow::Straight,
        );
    }
}

fn refresh_source_colour(
    assets: Res<SourceAssets>,
    sources: Query<(&CarrierSource, &mut MeshMaterial2d<ColorMaterial>), Changed<CarrierSource>>,
) {
    for (source, mut material) in sources {
        material.0 = material_for(&assets, source.active);
    }
}

pub fn spawn_source(commands: &mut Commands, position: Vec3) -> Entity {
    commands
        .spawn((
            Transform::from_translation(position),
            CarrierSource {
                timer: Timer::from_seconds(CARRIER_SPAWN_TIME, TimerMode::Repeating),
                active: true,
            },
        ))
        .id()
}

fn spawn_from_sources(
    mut commands: Commands,
    time: Res<Time>,
    mut sources: Query<(&mut CarrierSource, &Facing, &Transform)>,
    carriers: Query<&Transform, With<Carrier>>,
    mut ids: ResMut<NextCarrierId>,
) {
    for (mut source, facing, transform) in sources.iter_mut() {
        // Da spenta non emette e non conta nemmeno il tempo: riaccesa riparte
        // da dov'era, invece di recuperare l'attesa in un colpo solo.
        if !source.active {
            continue;
        }

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

        spawn_random_carrier(&mut commands, position, facing.0, &mut ids);
    }
}
