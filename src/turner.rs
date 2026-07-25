use bevy::prelude::*;

use crate::piece::{self, Arrow, PieceShapes};

/// Fa svoltare il carrier a destra rispetto alla sua marcia: chi va a sinistra
/// prosegue verso l'alto, chi sale prosegue verso destra, e cosi' via. Definirla
/// rispetto al carrier e non rispetto agli assi e' quello che la fa funzionare
/// in qualunque direzione. Agisce su tutti i carrier, vuoti compresi: e' un
/// pezzo di percorso, non uno smistamento.
#[derive(Component)]
pub struct Turner {
    pub active: bool,
}

/// Come il gate: a muovere i carrier ci pensa il loro movimento, qui c'e' solo
/// l'aspetto.
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
    idle_material: Handle<ColorMaterial>,
}

fn setup_turner_assets(mut commands: Commands, mut materials: ResMut<Assets<ColorMaterial>>) {
    commands.insert_resource(TurnerAssets {
        // Esagono: nessun altro oggetto ha questa forma.
        active_material: materials.add(Color::srgb(0.10, 0.70, 0.70)),
        idle_material: materials.add(Color::srgb(0.3, 0.3, 0.3)),
    });
}

pub fn spawn_turner(commands: &mut Commands, position: Vec3) -> Entity {
    commands
        .spawn((
            Transform::from_translation(position),
            Turner { active: true },
        ))
        .id()
}

fn material_for(assets: &TurnerAssets, active: bool) -> Handle<ColorMaterial> {
    if active {
        assets.active_material.clone()
    } else {
        assets.idle_material.clone()
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
            material_for(&assets, turner.active),
            Arrow::Straight,
        );
    }
}

fn refresh_turner_colour(
    assets: Res<TurnerAssets>,
    turners: Query<(&Turner, &mut MeshMaterial2d<ColorMaterial>), Changed<Turner>>,
) {
    for (turner, mut material) in turners {
        material.0 = material_for(&assets, turner.active);
    }
}
