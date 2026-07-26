use bevy::prelude::*;

use crate::carrier::{Carrier, CarrierType, Heading};
use crate::grid::GRID_STEP;
use crate::piece::{self, Arrow, BAR_LENGTH, Facing, PieceShapes};

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
    /// Spento vuol dire fuori servizio: resta grigio e non vede niente.
    pub active: bool,
    /// Se in questo istante ha un carrier davanti. Lo scrive la simulazione,
    /// lo legge l'interfaccia: cosi' il conto si fa anche senza finestra.
    pub seeing: bool,
}

impl Sensor {
    fn watches(&self, kind: CarrierType) -> bool {
        match self.kind {
            SensorKind::Tube => kind == CarrierType::WithTube,
            SensorKind::Carrier => true,
        }
    }
}

/// Vero se il carrier sta passando davanti al sensore. Il sensore guarda di
/// traverso alla propria parete, come una fotocellula: conta dove si trova il
/// carrier nella cella, non il contatto con la sbarra, che non avverrebbe mai
/// visto che il carrier passa al centro e la sbarra sta sul bordo.
pub fn in_beam(sensor: Vec3, facing: Heading, carrier: Vec3) -> bool {
    let local = facing.rotation().inverse() * (carrier - sensor);

    local.x.abs() <= BAR_LENGTH / 2.0 && local.y.abs() <= GRID_STEP / 2.0
}

/// La parte che conta anche senza interfaccia: chi vede chi.
pub struct SensorPlugin;

impl Plugin for SensorPlugin {
    fn build(&self, app: &mut App) {
        // Senza `run_if`: anche in pausa e durante una riproduzione un carrier
        // fermo davanti al sensore e' comunque davanti al sensore.
        app.add_systems(Update, sense_carriers);
    }
}

fn sense_carriers(
    carriers: Query<(&Carrier, &Transform)>,
    sensors: Query<(&mut Sensor, &Facing, &Transform), Without<Carrier>>,
) {
    for (mut sensor, facing, transform) in sensors {
        let seeing = sensor.active
            && carriers.iter().any(|(carrier, position)| {
                sensor.watches(carrier.kind)
                    && in_beam(transform.translation, facing.0, position.translation)
            });

        // Si scrive solo quando cambia davvero: il colore del bordo si aggiorna
        // guardando `Changed<Sensor>`, e riscriverlo ogni frame lo sveglierebbe
        // per niente a ogni frame.
        if sensor.seeing != seeing {
            sensor.seeing = seeing;
        }
    }
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
    tube_material: Handle<ColorMaterial>,
    carrier_material: Handle<ColorMaterial>,
    idle_material: Handle<ColorMaterial>,
}

/// Il bordo che si accende. Sta su un figlio e non sulla sbarra stessa perche'
/// il colore della sbarra dice che sensore e', e quello non deve cambiare
/// mentre lavora.
#[derive(Component)]
struct SensorGlow;

fn setup_sensor_assets(mut commands: Commands, mut materials: ResMut<Assets<ColorMaterial>>) {
    commands.insert_resource(SensorAssets {
        tube_material: materials.add(Color::srgb(0.95, 0.80, 0.20)),
        carrier_material: materials.add(Color::srgb(0.10, 0.75, 0.80)),
        idle_material: materials.add(Color::srgb(0.3, 0.3, 0.3)),
    });
}

pub fn spawn_sensor(commands: &mut Commands, position: Vec3, kind: SensorKind) -> Entity {
    commands
        .spawn((
            Transform::from_translation(position),
            Sensor {
                kind,
                active: true,
                seeing: false,
            },
        ))
        .id()
}

fn material_for(assets: &SensorAssets, sensor: &Sensor) -> Handle<ColorMaterial> {
    if !sensor.active {
        return assets.idle_material.clone();
    }

    match sensor.kind {
        SensorKind::Tube => assets.tube_material.clone(),
        SensorKind::Carrier => assets.carrier_material.clone(),
    }
}

fn attach_sensor_visuals(
    mut commands: Commands,
    shapes: Res<PieceShapes>,
    assets: Res<SensorAssets>,
    sensors: Query<(Entity, &Sensor), Without<Mesh2d>>,
) {
    for (entity, sensor) in sensors.iter() {
        piece::dress_shape(
            &mut commands,
            entity,
            &shapes,
            piece::bar(&shapes),
            material_for(&assets, sensor),
            Arrow::None,
        );

        commands.entity(entity).with_child((
            Mesh2d(piece::glow(&shapes)),
            MeshMaterial2d(piece::glow_material(&shapes)),
            // Dietro alla sbarra, cosi' sporge tutt'attorno come un bordo.
            Transform::from_xyz(0.0, 0.0, -0.01),
            Visibility::Hidden,
            SensorGlow,
        ));
    }
}

/// Il colore segue lo stato e il bordo segue quello che il sensore vede: chi
/// accende, spegne o passa davanti non deve sapere niente di mesh.
fn refresh_sensor_look(
    assets: Res<SensorAssets>,
    sensors: Query<(&Sensor, &Children, &mut MeshMaterial2d<ColorMaterial>), Changed<Sensor>>,
    mut glows: Query<&mut Visibility, With<SensorGlow>>,
) {
    for (sensor, children, mut material) in sensors {
        material.0 = material_for(&assets, sensor);

        for child in children.iter() {
            if let Ok(mut visibility) = glows.get_mut(child) {
                *visibility = if sensor.seeing {
                    Visibility::Visible
                } else {
                    Visibility::Hidden
                };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::carrier::Motion;

    /// Fa girare il sistema vero su una scena minima: un sensore e un carrier
    /// nella stessa cella. E' l'unico modo di verificare quello che conta
    /// davvero, cioe' che il sensore si accenda per chi deve e per nessun altro.
    fn sees(kind: SensorKind, active: bool, passing: CarrierType) -> bool {
        let mut app = App::new();
        app.add_plugins(SensorPlugin);

        let sensor = app
            .world_mut()
            .spawn((
                Transform::default(),
                Facing(Heading::Up),
                Sensor {
                    kind,
                    active,
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
            active: true,
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
