use bevy::prelude::*;

use crate::carrier::{Carrier, Heading};
use crate::engagement::{Engaged, in_the_same_cell};
use crate::piece::{self, Arrow, PieceShapes};

/// Fa svoltare il carrier a destra rispetto alla sua marcia: chi va a sinistra
/// prosegue verso l'alto, chi sale prosegue verso destra, e cosi' via. Definirla
/// rispetto al carrier e non rispetto agli assi e' quello che la fa funzionare
/// in qualunque direzione. Agisce su tutti i carrier, vuoti compresi: e' un
/// pezzo di percorso, non uno smistamento.
#[derive(Component)]
pub struct Turner {
    pub active: bool,
    /// Se in questo istante ha un carrier fra le mani. Lo scrive la simulazione, lo legge il colore.
    pub engaged: bool,
}

/// Come il gate: a muovere i carrier ci pensa il loro movimento, qui c'e' solo
/// l'aspetto.

impl Engaged for Turner {
    fn active(&self) -> bool {
        self.active
    }

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

pub struct TurnerVisualsPlugin;

impl Plugin for TurnerVisualsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_turner_assets)
            .add_systems(Update, (attach_turner_visuals, refresh_turner_colour));
    }
}

#[derive(Resource)]
pub struct TurnerAssets {
    active_material: Handle<ColorMaterial>,
    /// Lo stesso colore schiarito, per quando c'e' un carrier in mezzo.
    busy_material: Handle<ColorMaterial>,
    idle_material: Handle<ColorMaterial>,
}

fn setup_turner_assets(mut commands: Commands, mut materials: ResMut<Assets<ColorMaterial>>) {
    commands.insert_resource(TurnerAssets {
        // Esagono: nessun altro oggetto ha questa forma.
        active_material: materials.add(Color::srgb(0.10, 0.70, 0.70)),
        busy_material: materials.add(Color::srgb(0.48, 0.97, 0.97)),
        idle_material: materials.add(Color::srgb(0.3, 0.3, 0.3)),
    });
}

pub fn spawn_turner(commands: &mut Commands, position: Vec3) -> Entity {
    commands
        .spawn((
            Transform::from_translation(position),
            Turner {
                active: true,
                engaged: false,
            },
        ))
        .id()
}

fn material_for(assets: &TurnerAssets, turner: &Turner) -> Handle<ColorMaterial> {
    match (turner.active, turner.engaged) {
        (false, _) => assets.idle_material.clone(),
        (true, false) => assets.active_material.clone(),
        (true, true) => assets.busy_material.clone(),
    }
}

fn attach_turner_visuals(
    mut commands: Commands,
    shapes: Res<PieceShapes>,
    assets: Res<TurnerAssets>,
    turners: Query<(Entity, &Turner), Without<Mesh2d>>,
) {
    for (entity, turner) in turners.iter() {
        piece::dress(
            &mut commands,
            entity,
            &shapes,
            material_for(&assets, turner),
            Arrow::Straight,
        );
    }
}

fn refresh_turner_colour(
    assets: Res<TurnerAssets>,
    turners: Query<(&Turner, &mut MeshMaterial2d<ColorMaterial>), Changed<Turner>>,
) {
    for (turner, mut material) in turners {
        material.0 = material_for(&assets, turner);
    }
}
