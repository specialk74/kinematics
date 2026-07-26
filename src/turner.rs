use bevy::prelude::*;

use crate::carrier::{Carrier, Heading};
use crate::editor::Tool;
use crate::engagement::{Engaged, in_the_same_cell};
use crate::piece::{self, PieceShapes};
use crate::switch::{Look, Switch};

/// Fa svoltare il carrier a destra rispetto alla sua marcia: chi va a sinistra
/// prosegue verso l'alto, chi sale prosegue verso destra, e cosi' via. Definirla
/// rispetto al carrier e non rispetto agli assi e' quello che la fa funzionare
/// in qualunque direzione. Agisce su tutti i carrier, vuoti compresi: e' un
/// pezzo di percorso, non uno smistamento.
#[derive(Component)]
pub struct Turner {
    /// Se in questo istante ha un carrier fra le mani. Lo scrive la
    /// simulazione, lo legge il colore.
    pub engaged: bool,
}

/// Come il gate: a muovere i carrier ci pensa il loro movimento, qui c'e' solo
/// l'aspetto.

impl Engaged for Turner {
    fn engaged(&self) -> bool {
        self.engaged
    }

    fn set_engaged(&mut self, engaged: bool) {
        self.engaged = engaged;
    }

    fn reaches(
        &self,
        _switch: Switch,
        at: Vec3,
        _facing: Heading,
        _carrier: &Carrier,
        carrier_at: Vec3,
    ) -> bool {
        // L'azione dura un frame solo - l'istante in cui la marcia cambia -
        // quindi qui "ce l'ho fra le mani" resta la risposta giusta.
        in_the_same_cell(at, carrier_at)
    }
}

pub struct TurnerVisualsPlugin;

impl Plugin for TurnerVisualsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_turner_assets)
            .add_systems(Update, (attach_turner_visuals, refresh_turner_colour));
    }
}

#[derive(Resource)]
pub struct TurnerAssets {
    look: Look,
}

fn setup_turner_assets(mut commands: Commands, mut materials: ResMut<Assets<ColorMaterial>>) {
    commands.insert_resource(TurnerAssets {
        look: Look::new(&mut materials, Color::srgb(0.10, 0.70, 0.70)),
    });
}

pub fn spawn_turner(commands: &mut Commands, position: Vec3) -> Entity {
    commands
        .spawn((
            Transform::from_translation(position),
            Turner { engaged: false },
        ))
        .id()
}

fn material_for(assets: &TurnerAssets, turner: &Turner, switch: Switch) -> Handle<ColorMaterial> {
    assets.look.material(switch, turner.engaged)
}

fn attach_turner_visuals(
    mut commands: Commands,
    shapes: Res<PieceShapes>,
    assets: Res<TurnerAssets>,
    turners: Query<(Entity, &Turner, &Switch), Without<Mesh2d>>,
) {
    for (entity, turner, switch) in turners.iter() {
        let (shape, arrow) = piece::dressing(&shapes, Tool::Turner);

        piece::dress_shape(
            &mut commands,
            entity,
            &shapes,
            shape,
            material_for(&assets, turner, *switch),
            arrow,
        );
    }
}

fn refresh_turner_colour(
    assets: Res<TurnerAssets>,
    turners: Query<
        (&Turner, &Switch, &mut MeshMaterial2d<ColorMaterial>),
        Or<(Changed<Turner>, Changed<Switch>)>,
    >,
) {
    for (turner, switch, mut material) in turners {
        material.0 = material_for(&assets, turner, *switch);
    }
}
