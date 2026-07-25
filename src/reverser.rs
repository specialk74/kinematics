use std::f32::consts::PI;

use bevy::prelude::*;

use crate::carrier::{Heading, Motion};
use crate::grid::GRID_STEP;

pub const REVERSER_SIZE: f32 = 30.0;
/// Raggio della curva: mezza cella, cosi' la semicirconferenza porta il carrier
/// esattamente una cella piu' in basso.
pub const TURN_RADIUS: f32 = GRID_STEP / 2.0;

/// Inversione di marcia: il carrier fa mezzo giro e riparte in senso opposto su
/// una linea parallela, una cella piu' in la'. Non ripercorre la linea di
/// andata, che e' il motivo per cui la curva c'e'.
///
/// La curva gira sempre verso la sinistra del carrier, e questo vale per
/// qualunque marcia: chi va a sinistra esce una cella piu' in basso, chi sale
/// esce una cella piu' a sinistra.
#[derive(Component)]
pub struct Reverser {
    pub active: bool,
}

impl Reverser {
    /// Il moto circolare che questo oggetto, piazzato in `position`, impone a un
    /// carrier che arriva marciando in `heading`. Il centro sta mezza cella di
    /// lato, quindi il mezzo giro copre esattamente una cella.
    pub fn turn(&self, position: Vec3, heading: Heading) -> Motion {
        // Il centro sta alla sinistra del carrier: la curva e' antioraria.
        let left = -heading.turn_right().as_vec();

        Motion::Turning {
            centre: position.truncate() + left * TURN_RADIUS,
            remaining: PI,
            exit: heading.opposite(),
        }
    }
}

pub struct ReverserVisualsPlugin;

impl Plugin for ReverserVisualsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_reverser_assets)
            .add_systems(Update, (attach_reverser_visuals, refresh_reverser_colour));
    }
}

#[derive(Resource)]
pub struct ReverserAssets {
    mesh: Handle<Mesh>,
    active_material: Handle<ColorMaterial>,
    idle_material: Handle<ColorMaterial>,
}

fn setup_reverser_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    commands.insert_resource(ReverserAssets {
        // Rombo: distinto da triangoli, quadrati, sbarre e cerchi.
        mesh: meshes.add(Rhombus::new(REVERSER_SIZE, REVERSER_SIZE)),
        active_material: materials.add(Color::srgb(0.65, 0.30, 0.95)),
        idle_material: materials.add(Color::srgb(0.3, 0.3, 0.3)),
    });
}

/// Mesh e orientamento, condivisi con l'anteprima dell'editor.
pub fn shape(assets: &ReverserAssets) -> (Handle<Mesh>, Quat) {
    (assets.mesh.clone(), Quat::IDENTITY)
}

pub fn spawn_reverser(commands: &mut Commands, position: Vec3) -> Entity {
    commands
        .spawn((
            Transform::from_translation(position),
            Reverser { active: true },
        ))
        .id()
}

fn material_for(assets: &ReverserAssets, reverser: &Reverser) -> Handle<ColorMaterial> {
    if reverser.active {
        assets.active_material.clone()
    } else {
        assets.idle_material.clone()
    }
}

fn attach_reverser_visuals(
    mut commands: Commands,
    assets: Res<ReverserAssets>,
    reversers: Query<(Entity, &Reverser), Without<Mesh2d>>,
) {
    let (mesh, _) = shape(&assets);

    for (entity, reverser) in reversers.iter() {
        commands.entity(entity).insert((
            Mesh2d(mesh.clone()),
            MeshMaterial2d(material_for(&assets, reverser)),
        ));
    }
}

fn refresh_reverser_colour(
    assets: Res<ReverserAssets>,
    reversers: Query<(&Reverser, &mut MeshMaterial2d<ColorMaterial>), Changed<Reverser>>,
) {
    for (reverser, mut material) in reversers {
        material.0 = material_for(&assets, reverser);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reverser() -> Reverser {
        Reverser { active: true }
    }

    /// Il caso di partenza: chi va a sinistra e curva in antiorario esce verso
    /// destra una cella piu' in basso.
    #[test]
    fn a_leftward_carrier_turning_counter_clockwise_comes_out_one_cell_below() {
        let Motion::Turning { centre, exit, .. } = reverser().turn(Vec3::ZERO, Heading::Left)
        else {
            panic!("l'inversione deve produrre una curva");
        };

        assert_eq!(centre, Vec2::new(0.0, -TURN_RADIUS));
        assert_eq!(centre.y - TURN_RADIUS, -GRID_STEP, "esce una cella sotto");
        assert_eq!(exit, Heading::Right);
    }

    /// La stessa inversione su un flusso verticale: chi sale torna giu' una
    /// cella di lato, senza ripassare dalla colonna di salita.
    #[test]
    fn a_rising_carrier_is_turned_sideways_and_sent_down() {
        let Motion::Turning { centre, exit, .. } = reverser().turn(Vec3::ZERO, Heading::Up) else {
            panic!("l'inversione deve produrre una curva");
        };

        assert_eq!(centre, Vec2::new(-TURN_RADIUS, 0.0));
        assert_eq!(
            centre.x - TURN_RADIUS,
            -GRID_STEP,
            "esce una cella a sinistra"
        );
        assert_eq!(exit, Heading::Down);
    }
}
