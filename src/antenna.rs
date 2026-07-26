use bevy::prelude::*;

use crate::carrier::{Carrier, Heading};
use crate::piece::{self, ANTENNA_OFFSET, ANTENNA_RADIUS, Facing, PieceShapes};

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
    /// Se in questo istante ha un carrier sopra. Lo scrive la simulazione, lo
    /// legge l'interfaccia accendendole il bordo: cosi' il conto si fa anche
    /// senza finestra.
    pub seeing: bool,
}

/// Dove guarda l'antenna: non il centro della cella ma il punto scostato verso
/// il lato, che e' dove si ferma il carrier che il gate blocca.
pub fn eye(position: Vec3, facing: Heading) -> Vec3 {
    position + (facing.as_vec() * ANTENNA_OFFSET).extend(0.0)
}

/// Vero se il carrier sta sopra l'antenna. Basta il centro del carrier dentro
/// il cerchio dell'antenna: due carrier in coda distano 34, quindi non possono
/// esserci sopra tutti e due.
pub fn over(eye: Vec3, carrier: Vec3) -> bool {
    eye.truncate().distance(carrier.truncate()) <= ANTENNA_RADIUS
}

/// La lettura vera e propria, che serve anche senza finestra.
pub struct AntennaPlugin;

impl Plugin for AntennaPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, sense_carriers);
    }
}

fn sense_carriers(
    carriers: Query<&Transform, With<Carrier>>,
    antennas: Query<(&mut Antenna, &Facing, &Transform), Without<Carrier>>,
) {
    for (mut antenna, facing, transform) in antennas {
        let eye = eye(transform.translation, facing.0);
        let seeing = antenna.active
            && carriers
                .iter()
                .any(|carrier| over(eye, carrier.translation));

        if antenna.seeing != seeing {
            antenna.seeing = seeing;
        }
    }
}

pub struct AntennaVisualsPlugin;

impl Plugin for AntennaVisualsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_antenna_assets)
            .add_systems(Update, (attach_antenna_visuals, refresh_antenna_look));
    }
}

#[derive(Resource)]
pub struct AntennaAssets {
    active_material: Handle<ColorMaterial>,
    reading_material: Handle<ColorMaterial>,
    idle_material: Handle<ColorMaterial>,
}

fn setup_antenna_assets(mut commands: Commands, mut materials: ResMut<Assets<ColorMaterial>>) {
    commands.insert_resource(AntennaAssets {
        active_material: materials.add(Color::srgb(0.30, 0.70, 0.95)),
        // Lo stesso azzurro schiarito. Un alone verde come quello dei sensori
        // qui non servirebbe: finirebbe accanto al verde del carrier che sta
        // leggendo, e due verdi vicini non dicono niente.
        reading_material: materials.add(Color::srgb(0.62, 0.92, 1.0)),
        // Lo stesso grigio degli altri oggetti spenti: spento si legge allo
        // stesso modo in tutta la scena.
        idle_material: materials.add(Color::srgb(0.3, 0.3, 0.3)),
    });
}

pub fn spawn_antenna(commands: &mut Commands, position: Vec3) -> Entity {
    commands
        .spawn((
            Transform::from_translation(position),
            Antenna {
                active: true,
                seeing: false,
            },
        ))
        .id()
}

fn material_for(assets: &AntennaAssets, antenna: &Antenna) -> Handle<ColorMaterial> {
    match (antenna.active, antenna.seeing) {
        (false, _) => assets.idle_material.clone(),
        (true, false) => assets.active_material.clone(),
        (true, true) => assets.reading_material.clone(),
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

/// Colore e lettura seguono lo stato: chi accende, spegne o ci passa sopra non
/// deve sapere niente di mesh.
fn refresh_antenna_look(
    assets: Res<AntennaAssets>,
    antennas: Query<(&Antenna, &mut MeshMaterial2d<ColorMaterial>), Changed<Antenna>>,
) {
    for (antenna, mut material) in antennas {
        material.0 = material_for(&assets, antenna);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::carrier::CARRIER_SIZE;

    /// L'antenna guarda il punto in cui si ferma il carrier, non il centro
    /// della cella, e si gira con l'oggetto che le sta sopra.
    #[test]
    fn the_eye_follows_the_facing() {
        assert_eq!(
            eye(Vec3::ZERO, Heading::Left),
            Vec3::new(-ANTENNA_OFFSET, 0.0, 0.0)
        );
        assert_eq!(
            eye(Vec3::ZERO, Heading::Up),
            Vec3::new(0.0, ANTENNA_OFFSET, 0.0)
        );
    }

    /// Legge chi le sta sopra, e uno solo: due carrier in coda distano piu' del
    /// suo raggio, quindi non c'e' modo che li prenda tutti e due.
    #[test]
    fn only_the_carrier_standing_on_it_is_read() {
        let eye = Vec3::ZERO;

        assert!(over(eye, Vec3::ZERO));
        assert!(over(eye, Vec3::new(ANTENNA_RADIUS - 1.0, 0.0, 0.0)));
        assert!(!over(eye, Vec3::new(ANTENNA_RADIUS + 1.0, 0.0, 0.0)));
        assert!(
            !over(eye, Vec3::new(CARRIER_SIZE, 0.0, 0.0)),
            "quello in coda dietro non la riguarda"
        );
    }
}
