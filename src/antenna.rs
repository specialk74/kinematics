use bevy::prelude::*;

use crate::piece::{self, PieceShapes};

/// Antenna di lettura: sta sotto la linea e guarda passare i carrier sopra di
/// se'. Per ora e' solo un punto sulla mappa e non tocca il flusso in nessun
/// modo; cosa legga e a chi lo dica arrivera' con mqtt.
///
/// Vive su un piano suo (`Layer::Under`), quindi puo' condividere la cella con
/// un oggetto di linea: e' proprio dove un'antenna serve, sotto il punto in cui
/// il carrier si ferma o viene deviato.
#[derive(Component)]
pub struct Antenna {
    /// Antenna spenta: resta dov'e' ma diventa grigia. Cosa smetta di fare
    /// esattamente lo dira' mqtt; per ora e' un interruttore che si vede.
    pub active: bool,
}

pub struct AntennaVisualsPlugin;

impl Plugin for AntennaVisualsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_antenna_assets)
            .add_systems(Update, (attach_antenna_visuals, refresh_antenna_colour));
    }
}

#[derive(Resource)]
pub struct AntennaAssets {
    active_material: Handle<ColorMaterial>,
    idle_material: Handle<ColorMaterial>,
}

fn setup_antenna_assets(mut commands: Commands, mut materials: ResMut<Assets<ColorMaterial>>) {
    commands.insert_resource(AntennaAssets {
        active_material: materials.add(Color::srgb(0.30, 0.70, 0.95)),
        // Lo stesso grigio degli altri oggetti spenti: spento si legge allo
        // stesso modo in tutta la scena.
        idle_material: materials.add(Color::srgb(0.3, 0.3, 0.3)),
    });
}

pub fn spawn_antenna(commands: &mut Commands, position: Vec3) -> Entity {
    commands
        .spawn((
            Transform::from_translation(position),
            Antenna { active: true },
        ))
        .id()
}

fn material_for(assets: &AntennaAssets, antenna: &Antenna) -> Handle<ColorMaterial> {
    if antenna.active {
        assets.active_material.clone()
    } else {
        assets.idle_material.clone()
    }
}

/// Un cerchio piu' largo del quadrato degli oggetti di linea: cosi' quando ci
/// finisce sotto un gate le resta fuori una corona, che dice che l'antenna c'e'
/// e di che colore e'. I carrier che le passano sopra la coprono solo in parte,
/// per lo stesso motivo.
fn attach_antenna_visuals(
    mut commands: Commands,
    shapes: Res<PieceShapes>,
    assets: Res<AntennaAssets>,
    antennas: Query<(Entity, &Antenna), Without<Mesh2d>>,
) {
    for (entity, antenna) in antennas.iter() {
        commands.entity(entity).insert((
            Mesh2d(piece::circle(&shapes)),
            MeshMaterial2d(material_for(&assets, antenna)),
        ));
    }
}

fn refresh_antenna_colour(
    assets: Res<AntennaAssets>,
    antennas: Query<(&Antenna, &mut MeshMaterial2d<ColorMaterial>), Changed<Antenna>>,
) {
    for (antenna, mut material) in antennas {
        material.0 = material_for(&assets, antenna);
    }
}
