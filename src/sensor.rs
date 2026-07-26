use bevy::prelude::*;

use crate::carrier::{COLLAR_RADIUS, Carrier, CarrierType, Heading};
use crate::editor::Tool;
use crate::engagement::Engaged;
use crate::grid::GRID_STEP;
use crate::piece::{self, PieceShapes};
use crate::switch::{Look, Switch};

/// Che cosa guarda il sensore. E' l'unica differenza fra i due: la zona che
/// sorvegliano e il modo in cui si accendono sono gli stessi.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SensorKind {
    /// Vede solo i carrier che portano una provetta.
    Tube,
    /// Vede qualunque carrier passi.
    Carrier,
}

/// Fotocellula montata su una parete della cella. Non tocca il flusso: guarda e
/// basta, e mentre vede qualcosa lo dice accendendo il proprio bordo.
#[derive(Component)]
pub struct Sensor {
    pub kind: SensorKind,
    /// Se in questo istante dichiara presenza: perche' un carrier gli passa
    /// davvero davanti, o perche' un tester lo ha forzato. Lo scrive la
    /// simulazione, lo legge l'interfaccia: cosi' il conto si fa anche senza
    /// finestra.
    pub seeing: bool,
}

impl SensorKind {
    pub fn tool(self) -> Tool {
        match self {
            SensorKind::Tube => Tool::TubeSensor,
            SensorKind::Carrier => Tool::CarrierSensor,
        }
    }
}

impl Sensor {
    fn watches(&self, kind: CarrierType) -> bool {
        match self.kind {
            SensorKind::Tube => kind == CarrierType::WithTube,
            SensorKind::Carrier => true,
        }
    }
}

impl Engaged for Sensor {
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

    fn reaches(&self, at: Vec3, facing: Heading, carrier: &Carrier, carrier_at: Vec3) -> bool {
        self.watches(carrier.kind) && in_beam(at, facing, carrier_at)
    }
}

/// Vero se il carrier sta interrompendo il fascio del sensore. Il fascio e' una
/// riga sottile che dalla parete taglia la corsia di traverso, come una
/// fotocellula vera: non e' il contatto con la sbarra, che non avverrebbe mai
/// visto che il carrier passa al centro e la sbarra sta sul bordo.
///
/// A interromperlo e' il collare, non la base: e' la ragione per cui due carrier
/// a contatto danno due letture distinte invece di una sola lunga. Le basi si
/// toccano, i collari no.
pub fn in_beam(sensor: Vec3, facing: Heading, carrier: Vec3) -> bool {
    let local = facing.rotation().inverse() * (carrier - sensor);

    local.x.abs() <= COLLAR_RADIUS && local.y.abs() <= GRID_STEP / 2.0
}

pub struct SensorVisualsPlugin;

impl Plugin for SensorVisualsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_sensor_assets)
            .add_systems(Update, (attach_sensor_visuals, refresh_sensor_look));
    }
}

#[derive(Resource)]
pub struct SensorAssets {
    tube: Look,
    carrier: Look,
}

fn setup_sensor_assets(mut commands: Commands, mut materials: ResMut<Assets<ColorMaterial>>) {
    commands.insert_resource(SensorAssets {
        tube: Look::new(&mut materials, Color::srgb(0.95, 0.80, 0.20)),
        carrier: Look::new(&mut materials, Color::srgb(0.10, 0.75, 0.80)),
    });
}

pub fn spawn_sensor(commands: &mut Commands, position: Vec3, kind: SensorKind) -> Entity {
    commands
        .spawn((
            Transform::from_translation(position),
            Sensor {
                kind,
                seeing: false,
            },
        ))
        .id()
}

fn material_for(assets: &SensorAssets, sensor: &Sensor, switch: Switch) -> Handle<ColorMaterial> {
    let look = match sensor.kind {
        SensorKind::Tube => &assets.tube,
        SensorKind::Carrier => &assets.carrier,
    };

    look.material(switch, sensor.seeing)
}

fn attach_sensor_visuals(
    mut commands: Commands,
    shapes: Res<PieceShapes>,
    assets: Res<SensorAssets>,
    sensors: Query<(Entity, &Sensor, &Switch), Without<Mesh2d>>,
) {
    for (entity, sensor, switch) in sensors.iter() {
        let (shape, arrow) = piece::dressing(&shapes, sensor.kind.tool());

        piece::dress_shape(
            &mut commands,
            entity,
            &shapes,
            shape,
            material_for(&assets, sensor, *switch),
            arrow,
        );
    }
}

/// Il colore segue lo stato e quello che il sensore vede: chi accende, spegne
/// o passa davanti non deve sapere niente di mesh.
fn refresh_sensor_look(
    assets: Res<SensorAssets>,
    sensors: Query<
        (&Sensor, &Switch, &mut MeshMaterial2d<ColorMaterial>),
        Or<(Changed<Sensor>, Changed<Switch>)>,
    >,
) {
    for (sensor, switch, mut material) in sensors {
        material.0 = material_for(&assets, sensor, *switch);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::carrier::{CARRIER_SIZE, Motion};
    use crate::piece::Facing;

    /// Fa girare il sistema vero su una scena minima: un sensore e un carrier
    /// nella stessa cella. E' l'unico modo di verificare quello che conta
    /// davvero, cioe' che il sensore si accenda per chi deve e per nessun altro.
    fn sees_with(kind: SensorKind, switch: Switch, carrier: Option<CarrierType>) -> bool {
        let mut app = App::new();
        app.add_plugins(crate::engagement::EngagementPlugin);

        let sensor = app
            .world_mut()
            .spawn((
                Transform::default(),
                Facing(Heading::Up),
                switch,
                Sensor {
                    kind,
                    seeing: false,
                },
            ))
            .id();
        if let Some(kind) = carrier {
            app.world_mut().spawn((
                Transform::default(),
                Carrier {
                    kind,
                    carrier_id: 1,
                    sample_id: None,
                    motion: Motion::Straight(Heading::Left),
                },
            ));
        }

        app.update();

        app.world().get::<Sensor>(sensor).expect("sensore").seeing
    }

    fn sees(kind: SensorKind, enabled: bool, passing: CarrierType) -> bool {
        let mut app = App::new();
        app.add_plugins(crate::engagement::EngagementPlugin);

        let sensor = app
            .world_mut()
            .spawn((
                Transform::default(),
                Facing(Heading::Up),
                Switch {
                    enabled,
                    active: false,
                },
                Sensor {
                    kind,
                    seeing: false,
                },
            ))
            .id();
        app.world_mut().spawn((
            Transform::default(),
            Carrier {
                kind: passing,
                carrier_id: 1,
                sample_id: None,
                motion: Motion::Straight(Heading::Left),
            },
        ));

        app.update();

        app.world().get::<Sensor>(sensor).expect("sensore").seeing
    }

    /// Il sensore tubo guarda solo le provette; quello dei carrier tutto.
    #[test]
    fn only_the_right_carrier_lights_the_sensor_up() {
        assert!(sees(SensorKind::Tube, true, CarrierType::WithTube));
        assert!(
            !sees(SensorKind::Tube, true, CarrierType::Empty),
            "un carrier vuoto non accende il sensore delle provette"
        );

        assert!(sees(SensorKind::Carrier, true, CarrierType::WithTube));
        assert!(sees(SensorKind::Carrier, true, CarrierType::Empty));
    }

    /// Il punto di tutto il simulatore: un tester puo' far dichiarare al
    /// sensore una presenza che non c'e', per vedere come reagisce il programma
    /// che sta collaudando. E un sensore fuori servizio non dichiara niente
    /// nemmeno se lo si forza.
    #[test]
    fn a_forced_sensor_reports_a_presence_that_is_not_there() {
        let forced = Switch {
            enabled: true,
            active: true,
        };
        let honest = Switch {
            enabled: true,
            active: false,
        };
        let out_of_service = Switch {
            enabled: false,
            active: true,
        };

        assert!(
            sees_with(SensorKind::Tube, forced, None),
            "forzato dichiara presenza a vuoto"
        );
        assert!(
            !sees_with(SensorKind::Tube, honest, None),
            "non forzato dice la verita': non c'e' nessuno"
        );
        assert!(
            !sees_with(SensorKind::Tube, out_of_service, None),
            "fuori servizio non dichiara niente, nemmeno forzato"
        );
        assert!(
            !sees_with(
                SensorKind::Tube,
                out_of_service,
                Some(CarrierType::WithTube)
            ),
            "e nemmeno quando davanti ci passa davvero qualcuno"
        );
    }

    /// Spento vuol dire fuori servizio: non vede niente, nemmeno quello che
    /// gli passa davanti.
    #[test]
    fn a_switched_off_sensor_stays_blind() {
        assert!(!sees(SensorKind::Tube, false, CarrierType::WithTube));
        assert!(!sees(SensorKind::Carrier, false, CarrierType::Empty));
    }

    fn sensor(kind: SensorKind) -> Sensor {
        Sensor {
            kind,
            seeing: false,
        }
    }

    /// Il sensore guarda la striscia di cella davanti alla propria parete.
    #[test]
    fn a_carrier_crossing_the_cell_is_seen() {
        let at = Vec3::ZERO;

        assert!(in_beam(at, Heading::Up, Vec3::ZERO), "in mezzo alla cella");
        assert!(
            in_beam(at, Heading::Up, Vec3::new(0.0, GRID_STEP / 2.0 - 1.0, 0.0)),
            "ancora dentro la cella, verso la sbarra"
        );
        assert!(
            !in_beam(at, Heading::Up, Vec3::new(0.0, GRID_STEP, 0.0)),
            "una cella piu' in la' non lo riguarda"
        );
        assert!(
            !in_beam(at, Heading::Up, Vec3::new(GRID_STEP, 0.0, 0.0)),
            "nemmeno la corsia accanto"
        );
    }

    /// Il motivo del collare: due carrier a contatto sono due letture, non una.
    /// A meta' strada fra i due il fascio e' libero, quindi il sensore si spegne
    /// e si riaccende invece di restare acceso per tutta la coppia.
    #[test]
    fn two_touching_carriers_break_the_beam_one_at_a_time() {
        let first = Vec3::ZERO;
        let second = Vec3::new(CARRIER_SIZE, 0.0, 0.0);
        let between = Vec3::new(CARRIER_SIZE / 2.0, 0.0, 0.0);

        // Il sensore che guarda esattamente in mezzo ai due non vede nessuno.
        assert!(!in_beam(between, Heading::Up, first));
        assert!(!in_beam(between, Heading::Up, second));

        // Ma davanti a ciascuno dei due si accende.
        assert!(in_beam(first, Heading::Up, first));
        assert!(in_beam(second, Heading::Up, second));
    }

    /// La zona si gira con il sensore: montato su un'altra parete guarda lungo
    /// l'altro asse.
    #[test]
    fn the_beam_turns_with_the_sensor() {
        let at = Vec3::ZERO;
        let along_the_lane = Vec3::new(GRID_STEP / 2.0 - 1.0, 0.0, 0.0);

        assert!(!in_beam(at, Heading::Up, along_the_lane));
        assert!(in_beam(at, Heading::Left, along_the_lane));
    }

    /// L'unica differenza fra i due sensori: il tubo si accende solo per chi ne
    /// porta uno, il carrier per chiunque passi.
    #[test]
    fn each_sensor_watches_its_own_thing() {
        assert!(sensor(SensorKind::Tube).watches(CarrierType::WithTube));
        assert!(!sensor(SensorKind::Tube).watches(CarrierType::Empty));

        assert!(sensor(SensorKind::Carrier).watches(CarrierType::WithTube));
        assert!(sensor(SensorKind::Carrier).watches(CarrierType::Empty));
    }
}
