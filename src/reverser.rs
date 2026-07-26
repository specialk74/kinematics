use std::f32::consts::PI;

use bevy::prelude::*;

use crate::carrier::{Carrier, Heading, Motion};
use crate::editor::Tool;
use crate::engagement::{Engaged, in_the_same_cell};
use crate::grid::GRID_STEP;
use crate::piece::{self, PieceShapes};
use crate::switch::{Look, Switch};

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
    /// Se in questo istante ha un carrier fra le mani. Lo scrive la
    /// simulazione, lo legge il colore.
    pub engaged: bool,
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

impl Engaged for Reverser {
    fn engaged(&self) -> bool {
        self.engaged
    }

    fn set_engaged(&mut self, engaged: bool) {
        self.engaged = engaged;
    }

    fn reaches(&self, at: Vec3, _facing: Heading, _carrier: &Carrier, carrier_at: Vec3) -> bool {
        // Per ora: "ho un carrier nella mia cella". La condizione vera, quella
        // che dira' a mqtt "sto agendo su questo carrier", e' diversa per
        // ciascuno e verra' quando serviranno i messaggi e non il colore.
        in_the_same_cell(at, carrier_at)
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
    look: Look,
}

fn setup_reverser_assets(mut commands: Commands, mut materials: ResMut<Assets<ColorMaterial>>) {
    commands.insert_resource(ReverserAssets {
        look: Look::new(&mut materials, Color::srgb(0.65, 0.30, 0.95)),
    });
}

pub fn spawn_reverser(commands: &mut Commands, position: Vec3) -> Entity {
    commands
        .spawn((
            Transform::from_translation(position),
            Reverser { engaged: false },
        ))
        .id()
}

fn material_for(
    assets: &ReverserAssets,
    reverser: &Reverser,
    switch: Switch,
) -> Handle<ColorMaterial> {
    assets.look.material(switch, reverser.engaged)
}

fn attach_reverser_visuals(
    mut commands: Commands,
    shapes: Res<PieceShapes>,
    assets: Res<ReverserAssets>,
    reversers: Query<(Entity, &Reverser, &Switch), Without<Mesh2d>>,
) {
    for (entity, reverser, switch) in reversers.iter() {
        let (shape, arrow) = piece::dressing(&shapes, Tool::Reverser);

        piece::dress_shape(
            &mut commands,
            entity,
            &shapes,
            shape,
            material_for(&assets, reverser, *switch),
            arrow,
        );
    }
}

fn refresh_reverser_colour(
    assets: Res<ReverserAssets>,
    reversers: Query<
        (&Reverser, &Switch, &mut MeshMaterial2d<ColorMaterial>),
        Or<(Changed<Reverser>, Changed<Switch>)>,
    >,
) {
    for (reverser, switch, mut material) in reversers {
        material.0 = material_for(&assets, reverser, *switch);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reverser() -> Reverser {
        Reverser { engaged: false }
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
