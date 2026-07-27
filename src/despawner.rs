use bevy::prelude::*;

use crate::carrier::{CARRIER_RADIUS, Carrier};
use crate::geometry::circle_touches_box;
use crate::piece::{self, PIECE_SIZE, PieceShapes, Tool};
use crate::simulation::SimulationState;
use crate::switch::Look;
use crate::switch::Switch;

/// Uscita dal sistema: il carrier che la tocca smette di esistere. Senza
/// nessuna uscita piazzata i carrier proseguono verso sinistra all'infinito.
#[derive(Component)]
pub struct Despawner;

pub struct DespawnerPlugin;

impl Plugin for DespawnerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            // In pausa non si distrugge niente: il mondo e' fermo del tutto.
            despawn_on_contact.run_if(in_state(SimulationState::Running)),
        );
    }
}

pub struct DespawnerVisualsPlugin;

impl Plugin for DespawnerVisualsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_despawner_assets)
            .add_systems(Update, (attach_despawner_visuals, refresh_despawner_colour));
    }
}

#[derive(Resource)]
pub struct DespawnerAssets {
    look: Look,
}

fn setup_despawner_assets(mut commands: Commands, mut materials: ResMut<Assets<ColorMaterial>>) {
    commands.insert_resource(DespawnerAssets {
        look: Look::new(&mut materials, Color::srgb(0.55, 0.10, 0.10)),
    });
}

pub fn spawn_despawner(commands: &mut Commands, position: Vec3) -> Entity {
    commands
        .spawn((Transform::from_translation(position), Despawner))
        .id()
}

fn attach_despawner_visuals(
    mut commands: Commands,
    shapes: Res<PieceShapes>,
    assets: Res<DespawnerAssets>,
    despawners: Query<(Entity, &Switch), (With<Despawner>, Without<Mesh2d>)>,
) {
    for (entity, switch) in despawners.iter() {
        let (shape, arrow) = piece::dressing(&shapes, Tool::Despawner);

        piece::dress_shape(
            &mut commands,
            entity,
            &shapes,
            shape,
            assets.look.material(*switch, false),
            arrow,
        );
    }
}

fn refresh_despawner_colour(
    assets: Res<DespawnerAssets>,
    despawners: Query<
        (&Switch, &mut MeshMaterial2d<ColorMaterial>),
        (With<Despawner>, Changed<Switch>),
    >,
) {
    for (switch, mut material) in despawners {
        material.0 = assets.look.material(*switch, false);
    }
}

/// Vero se il carrier sta toccando l'uscita.
fn swallows(despawner: Vec3, carrier: Vec3) -> bool {
    circle_touches_box(
        despawner,
        Vec2::splat(PIECE_SIZE / 2.0),
        carrier,
        CARRIER_RADIUS,
    )
}

fn despawn_on_contact(
    mut commands: Commands,
    carriers: Query<(Entity, &Transform), With<Carrier>>,
    despawners: Query<(&Switch, &Transform), (With<Despawner>, Without<Carrier>)>,
) {
    for (entity, carrier) in carriers.iter() {
        // Un'uscita fuori servizio o non comandata lascia proseguire: e' il modo
        // di provare cosa fa il programma di comando quando l'impianto non
        // smaltisce piu'.
        let swallowed = despawners
            .iter()
            .filter(|(switch, _)| switch.working())
            .any(|(_, despawner)| swallows(despawner.translation, carrier.translation));

        if swallowed {
            commands.entity(entity).despawn();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::GRID_STEP;

    #[test]
    fn a_carrier_arriving_on_the_exit_is_swallowed() {
        assert!(swallows(Vec3::ZERO, Vec3::new(20.0, 0.0, 0.0)));
        assert!(swallows(Vec3::ZERO, Vec3::ZERO));
    }

    #[test]
    fn a_carrier_still_far_away_survives() {
        assert!(!swallows(Vec3::ZERO, Vec3::new(60.0, 0.0, 0.0)));
    }

    /// Un'uscita su una corsia non tocca il flusso di quella accanto.
    #[test]
    fn a_carrier_on_another_lane_is_not_touched() {
        assert!(!swallows(Vec3::ZERO, Vec3::new(0.0, GRID_STEP, 0.0)));
    }
}
