use bevy::prelude::*;

use crate::carrier::{Carrier, Heading};
use crate::editor::Tool;
use crate::engagement::Engaged;
use crate::piece::{self, ANTENNA_OFFSET, ANTENNA_RADIUS, PieceShapes};
use crate::switch::{Look, Switch};

/// Antenna di lettura: sta sotto la linea e guarda passare i carrier sopra di
/// se'. Per ora e' solo un punto sulla mappa e non tocca il flusso in nessun
/// modo; cosa legga e a chi lo dica arrivera' con mqtt.
///
/// Vive su un piano suo (`Layer::Under`), quindi puo' condividere la cella con
/// un oggetto di linea: e' proprio dove un'antenna serve, sotto il punto in cui
/// il carrier si ferma o viene deviato.
#[derive(Component)]
pub struct Antenna {
    /// Se in questo istante sta leggendo: perche' un carrier le sta davvero
    /// sopra, o perche' un tester l'ha forzata. Lo scrive la simulazione, lo
    /// legge l'interfaccia.
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

impl Engaged for Antenna {
    fn engaged(&self) -> bool {
        self.seeing
    }

    fn set_engaged(&mut self, engaged: bool) {
        self.seeing = engaged;
    }

    /// Un tester puo' farlo dichiarare presenza anche a vuoto: e' proprio il
    /// caso che serve per provare gli scenari sbagliati.
    fn forced_by_switch(&self) -> bool {
        true
    }

    fn reaches(&self, at: Vec3, facing: Heading, _carrier: &Carrier, carrier_at: Vec3) -> bool {
        // L'antenna legge chiunque le stia sopra: il tubo non la riguarda.
        over(eye(at, facing), carrier_at)
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
    look: Look,
}

fn setup_antenna_assets(mut commands: Commands, mut materials: ResMut<Assets<ColorMaterial>>) {
    commands.insert_resource(AntennaAssets {
        // Un alone verde come quello di prima non servirebbe: finirebbe
        // accanto al verde del carrier che sta leggendo, e due verdi vicini non
        // dicono niente. Meglio lo stesso azzurro schiarito.
        look: Look::new(&mut materials, Color::srgb(0.30, 0.70, 0.95)),
    });
}

pub fn spawn_antenna(commands: &mut Commands, position: Vec3) -> Entity {
    commands
        .spawn((
            Transform::from_translation(position),
            Antenna { seeing: false },
        ))
        .id()
}

fn material_for(
    assets: &AntennaAssets,
    antenna: &Antenna,
    switch: Switch,
) -> Handle<ColorMaterial> {
    assets.look.material(switch, antenna.seeing)
}

/// Un cerchio piu' largo del quadrato degli oggetti di linea: cosi' quando ci
/// finisce sotto un gate le resta fuori una corona, che dice che l'antenna c'e'
/// e di che colore e'. I carrier che le passano sopra la coprono solo in parte,
/// per lo stesso motivo.
fn attach_antenna_visuals(
    mut commands: Commands,
    shapes: Res<PieceShapes>,
    assets: Res<AntennaAssets>,
    antennas: Query<(Entity, &Antenna, &Switch), Without<Mesh2d>>,
) {
    for (entity, antenna, switch) in antennas.iter() {
        let (shape, _) = piece::dressing(&shapes, Tool::Antenna);

        commands.entity(entity).insert((
            Mesh2d(shape),
            MeshMaterial2d(material_for(&assets, antenna, *switch)),
        ));
    }
}

/// Colore e lettura seguono lo stato: chi accende, spegne o ci passa sopra non
/// deve sapere niente di mesh.
fn refresh_antenna_look(
    assets: Res<AntennaAssets>,
    antennas: Query<
        (&Antenna, &Switch, &mut MeshMaterial2d<ColorMaterial>),
        Or<(Changed<Antenna>, Changed<Switch>)>,
    >,
) {
    for (antenna, switch, mut material) in antennas {
        material.0 = material_for(&assets, antenna, *switch);
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
