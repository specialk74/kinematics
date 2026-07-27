use bevy::prelude::*;

use crate::carrier::{CARRIER_SIZE, Carrier, NextCarrierId, spawn_random_carrier};
use crate::piece::{self, Facing, PieceShapes, Tool};
use crate::simulation::SimulationState;
use crate::switch::{Look, Switch};

pub const CARRIER_SPAWN_TIME: f32 = 0.500;

/// Oggetto che immette carrier nel flusso. Ogni sorgente ha il proprio timer,
/// cosi' due sorgenti piazzate in momenti diversi restano indipendenti.
#[derive(Component)]
pub struct CarrierSource {
    timer: Timer,
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
    look: Look,
}

fn setup_source_assets(mut commands: Commands, mut materials: ResMut<Assets<ColorMaterial>>) {
    commands.insert_resource(SourceAssets {
        look: Look::new(&mut materials, Color::srgb(0.2, 0.5, 1.0)),
    });
}

fn material_for(assets: &SourceAssets, switch: Switch) -> Handle<ColorMaterial> {
    assets.look.material(switch, false)
}

fn attach_source_visuals(
    mut commands: Commands,
    shapes: Res<PieceShapes>,
    assets: Res<SourceAssets>,
    sources: Query<(Entity, &Switch), (With<CarrierSource>, Without<Mesh2d>)>,
) {
    for (entity, switch) in sources.iter() {
        let (shape, arrow) = piece::dressing(&shapes, Tool::CarrierSource);

        piece::dress_shape(
            &mut commands,
            entity,
            &shapes,
            shape,
            material_for(&assets, *switch),
            arrow,
        );
    }
}

fn refresh_source_colour(
    assets: Res<SourceAssets>,
    sources: Query<
        (&Switch, &mut MeshMaterial2d<ColorMaterial>),
        (With<CarrierSource>, Changed<Switch>),
    >,
) {
    for (switch, mut material) in sources {
        material.0 = material_for(&assets, *switch);
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
    mut sources: Query<(&mut CarrierSource, &Switch, &Facing, &Transform)>,
    carriers: Query<&Transform, With<Carrier>>,
    mut ids: ResMut<NextCarrierId>,
) {
    for (mut source, switch, facing, transform) in sources.iter_mut() {
        // Ferma non emette e non conta nemmeno il tempo: ripartendo riprende da
        // dov'era, invece di recuperare l'attesa in un colpo solo.
        if !switch.working() {
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
