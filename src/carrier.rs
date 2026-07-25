use bevy::prelude::*;
use bevy::text::TextBounds;
use rand::prelude::*;

use crate::divert::Divert;
use crate::gate::{Gate, blocks_circle};
use crate::simulation::SimulationState;
use crate::{WORK_AREA_BOTTOM, WORK_AREA_LEFT, WORK_AREA_RIGHT, WORK_AREA_TOP};

pub const BELT_SPEED: f32 = 100.0;
pub const CARRIER_DIVERT_SPEED: f32 = 50.0;
pub const CARRIER_RADIUS: f32 = 15.0;
pub const CARRIER_THICKNESS: f32 = 3.0;
/// Distanza minima fra i centri di due carrier: diametro piu' un po' di margine.
pub const CARRIER_SIZE: f32 = CARRIER_RADIUS * 2.0 + 4.0;

/// Lunghezza massima di un identificativo di campione.
pub const SAMPLE_ID_MAX_LEN: usize = 24;

#[derive(PartialEq)]
pub enum CarrierType {
    Empty,
    WithTube,
}

/// Identificativo del campione trasportato. Il limite di 24 caratteri sta nel
/// costruttore invece che in un commento: un valore piu' lungo non puo' esistere.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SampleId(String);

impl SampleId {
    pub fn new(text: &str) -> Option<Self> {
        // Si contano i caratteri, non i byte: un accento occupa due byte ma
        // resta un carattere solo.
        let length = text.chars().count();

        (1..=SAMPLE_ID_MAX_LEN)
            .contains(&length)
            .then(|| SampleId(text.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Il carrier non sa niente del percorso: sono i deviatori che incontra a dirgli
/// dove andare. Porta pero' la propria identita', che serve anche senza interfaccia.
#[derive(Component)]
pub struct Carrier {
    pub kind: CarrierType,
    pub carrier_id: u32,
    /// Uno e uno solo, quando c'e'.
    pub sample_id: Option<SampleId>,
}

/// Contatore progressivo dei carrier. Riparte da 1 a ogni avvio.
#[derive(Resource)]
pub struct NextCarrierId(u32);

impl Default for NextCarrierId {
    fn default() -> Self {
        NextCarrierId(1)
    }
}

impl NextCarrierId {
    fn take(&mut self) -> u32 {
        let id = self.0;
        self.0 = self.0.wrapping_add(1);
        id
    }
}

#[derive(Component)]
pub struct Tube;

/// La cinematica: nessun riferimento a mesh, materiali o camera, cosi' gira
/// anche senza interfaccia.
pub struct CarrierPlugin;

impl Plugin for CarrierPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<NextCarrierId>().add_systems(
            Update,
            (move_carrier, despawn_offscreen).run_if(in_state(SimulationState::Running)),
        );
    }
}

/// L'aspetto dei carrier: si monta solo quando c'e' l'interfaccia.
pub struct CarrierVisualsPlugin;

impl Plugin for CarrierVisualsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_carrier_assets)
            .add_systems(Update, attach_carrier_visuals);
    }
}

/// Mesh e materiali dei carrier, creati una volta sola: se venissero costruiti a
/// ogni spawn si accumulerebbero asset identici per tutta la durata del gioco.
#[derive(Resource)]
pub struct CarrierAssets {
    empty_mesh: Handle<Mesh>,
    empty_material: Handle<ColorMaterial>,
    with_tube_mesh: Handle<Mesh>,
    with_tube_material: Handle<ColorMaterial>,
}

fn setup_carrier_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    commands.insert_resource(CarrierAssets {
        // Il carrier vuoto e' un anello: cerchio senza riempimento.
        empty_mesh: meshes.add(Annulus::new(
            CARRIER_RADIUS - CARRIER_THICKNESS,
            CARRIER_RADIUS,
        )),
        empty_material: materials.add(Color::WHITE),
        with_tube_mesh: meshes.add(Circle::new(CARRIER_RADIUS)),
        with_tube_material: materials.add(Color::srgb(0.0, 0.8, 0.2)),
    });
}

/// Il testo viene rasterizzato a questa dimensione e poi rimpicciolito dalla
/// scala: se lo si disegnasse gia' a 3 px, zoomando resterebbe una macchia
/// sfocata, perche' lo zoom ingrandisce i pixel gia' prodotti. Cosi' invece la
/// nitidezza c'e' fino a un ingrandimento di 1/LABEL_SCALE.
const LABEL_FONT_SIZE: f32 = 32.0;
const LABEL_SCALE: f32 = 0.11;
/// Lato del quadrato inscritto nel cerchio: il testo non ne esce mai.
const LABEL_BOX: f32 = CARRIER_RADIUS * std::f32::consts::SQRT_2;

/// Da' un corpo ai carrier appena nati. Senza questo sistema esistono lo stesso e
/// si muovono: semplicemente non li vede nessuno.
fn attach_carrier_visuals(
    mut commands: Commands,
    assets: Res<CarrierAssets>,
    carriers: Query<(Entity, &Carrier), Without<Mesh2d>>,
) {
    for (entity, carrier) in carriers.iter() {
        let (mesh, material) = match carrier.kind {
            CarrierType::WithTube => (
                assets.with_tube_mesh.clone(),
                assets.with_tube_material.clone(),
            ),
            CarrierType::Empty => (assets.empty_mesh.clone(), assets.empty_material.clone()),
        };

        commands
            .entity(entity)
            .insert((Mesh2d(mesh), MeshMaterial2d(material)))
            .with_child(carrier_label(carrier));
    }
}

/// Etichetta dentro al cerchio: carrier_id sulla prima riga, sample_id sotto se
/// c'e'. Va a capo su qualsiasi carattere, perche' un sample_id non ha spazi in
/// cui spezzarsi.
fn carrier_label(carrier: &Carrier) -> impl Bundle {
    let mut text = carrier.carrier_id.to_string();
    if let Some(sample_id) = &carrier.sample_id {
        text.push('\n');
        text.push_str(sample_id.as_str());
    }

    (
        Text2d::new(text),
        TextFont {
            font_size: LABEL_FONT_SIZE,
            ..default()
        },
        TextColor(Color::WHITE),
        TextLayout::new(Justify::Center, LineBreak::AnyCharacter),
        // I limiti sono nello spazio del testo, quindi prima della riduzione.
        TextBounds::new(LABEL_BOX / LABEL_SCALE, LABEL_BOX / LABEL_SCALE),
        Transform::from_xyz(0.0, 0.0, 0.1).with_scale(Vec3::splat(LABEL_SCALE)),
    )
}

/// Immette un carrier nel flusso; il tipo (vuoto o con tubo) e' casuale 50-50.
/// Il campione lo ha solo chi porta un tubo: e' il tubo il campione.
pub fn spawn_random_carrier(commands: &mut Commands, position: Vec3, ids: &mut NextCarrierId) {
    let mut rng = rand::rng();
    let carrier_id = ids.take();

    if rng.random::<u32>() > u32::MAX / 2 {
        commands.spawn((
            Transform::from_translation(position),
            Carrier {
                kind: CarrierType::WithTube,
                carrier_id,
                sample_id: placeholder_sample_id(carrier_id),
            },
            children![Tube],
        ));
    } else {
        commands.spawn((
            Transform::from_translation(position),
            Carrier {
                kind: CarrierType::Empty,
                carrier_id,
                sample_id: None,
            },
        ));
    }
}

/// Segnaposto in attesa che i campioni arrivino da fuori: e' derivato dal
/// carrier_id solo per avere qualcosa di stabile da guardare a schermo.
fn placeholder_sample_id(carrier_id: u32) -> Option<SampleId> {
    SampleId::new(&format!("SMP-{carrier_id:08}"))
}

/// Spostamento di un carrier in questo frame. Solo i carrier con tubo vengono
/// deviati; gli altri tirano dritto sulla corsia.
fn carrier_step(
    carrier: &Carrier,
    translation: Vec3,
    diverts: &[(&Divert, Vec3)],
    delta_secs: f32,
) -> Vec3 {
    let straight = Vec3::new(-BELT_SPEED * delta_secs, 0.0, 0.0);

    if carrier.kind != CarrierType::WithTube {
        return straight;
    }

    for (divert, position) in diverts {
        if !divert.catches(*position, translation) {
            continue;
        }

        // Il passo verticale non supera mai la quota di destinazione: e' questo
        // che fa arrivare il carrier esattamente sulla corsia voluta, invece di
        // lasciarlo dove capita quando esce dalla finestra del deviatore.
        let reach = BELT_SPEED * delta_secs;
        let lift = (divert.target_y(position.y) - translation.y).clamp(-reach, reach);
        // Gia' a destinazione: questo deviatore non ha niente da fare, ma un
        // altro nella stessa colonna potrebbe averne.
        if lift == 0.0 {
            continue;
        }

        return Vec3::new(-CARRIER_DIVERT_SPEED * delta_secs, lift, 0.0);
    }

    straight
}

fn move_carrier(
    time: Res<Time>,
    mut query: Query<(Entity, &Carrier, &mut Transform)>,
    gates: Query<(&Gate, &Transform), Without<Carrier>>,
    diverts: Query<(&Divert, &Transform), Without<Carrier>>,
) {
    let delta_secs = time.delta_secs();

    let active_gates: Vec<Vec3> = gates
        .iter()
        .filter(|(gate, _)| gate.active)
        .map(|(_, transform)| transform.translation)
        .collect();

    let diverts: Vec<(&Divert, Vec3)> = diverts
        .iter()
        .map(|(divert, transform)| (divert, transform.translation))
        .collect();

    // Si risolve un carrier alla volta partendo da quello piu' avanti sul nastro:
    // chi si ferma deve bloccare a cascata tutti quelli che ha dietro.
    let mut belt: Vec<(Entity, Vec3, Vec3)> = query
        .iter()
        .map(|(entity, carrier, transform)| {
            let translation = transform.translation;
            (
                entity,
                translation,
                carrier_step(carrier, translation, &diverts, delta_secs),
            )
        })
        .collect();
    belt.sort_by(|a, b| a.1.x.total_cmp(&b.1.x));

    let mut resolved: Vec<(Entity, Vec3)> = Vec::with_capacity(belt.len());
    for (entity, translation, step) in belt {
        let candidate = translation + step;
        // Il passo viene annullato solo se avvicina il carrier a uno gia' troppo
        // vicino: cosi' chi e' sovrapposto puo' comunque allontanarsi.
        let blocked = resolved.iter().any(|(_, ahead)| {
            let gap = candidate.distance(*ahead);
            gap < CARRIER_SIZE && gap < translation.distance(*ahead)
        })
        // Un gate attivo ferma il carrier subito prima di toccarlo. Chi lo stava
        // gia' attraversando quando il gate e' stato attivato finisce il transito
        // invece di restare incastrato dentro la sbarra.
            || active_gates.iter().any(|gate| {
                blocks_circle(*gate, candidate, CARRIER_RADIUS)
                    && !blocks_circle(*gate, translation, CARRIER_RADIUS)
            });
        resolved.push((entity, if blocked { translation } else { candidate }));
    }

    for (entity, translation) in resolved {
        if let Ok((_, _, mut transform)) = query.get_mut(entity) {
            transform.translation = translation;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::divert::{DIVERT_ZONE_HALF_WIDTH, DivertKind, LANE_HEIGHT};
    use crate::grid::GRID_STEP;

    const DELTA: f32 = 1.0 / 60.0;

    /// La corsia principale su cui stanno le sorgenti, in coordinate mondo.
    const MAIN_LANE: f32 = -120.0;

    fn atr() -> Divert {
        Divert {
            kind: DivertKind::Atr,
            active: true,
        }
    }

    fn carrier(kind: CarrierType) -> Carrier {
        Carrier {
            kind,
            carrier_id: 1,
            sample_id: None,
        }
    }

    /// Il caso completo: divert sulla corsia principale, ATR una cella piu' su.
    /// Il carrier deve salire di una corsia esatta e tornare esattamente da dove
    /// era partito, senza portarsi dietro nessuna informazione.
    #[test]
    fn divert_and_atr_hand_the_carrier_back_to_the_main_lane() {
        let divert = Divert {
            kind: DivertKind::Divert,
            active: true,
        };
        let deviators = [
            (&divert, Vec3::new(0.0, MAIN_LANE, 0.0)),
            (
                &atr(),
                Vec3::new(-3.0 * GRID_STEP, MAIN_LANE + GRID_STEP, 0.0),
            ),
        ];

        let carrier = carrier(CarrierType::WithTube);
        let mut position = Vec3::new(4.0 * GRID_STEP, MAIN_LANE, 0.0);
        let mut highest = MAIN_LANE;

        for _ in 0..1000 {
            position += carrier_step(&carrier, position, &deviators, DELTA);
            highest = highest.max(position.y);
        }

        assert_eq!(
            highest,
            MAIN_LANE + LANE_HEIGHT,
            "il divert alza di una corsia esatta"
        );
        assert_eq!(
            position.y, MAIN_LANE,
            "l'ATR riporta il carrier sulla corsia principale"
        );
    }

    /// L'ATR da solo: scende di una corsia rispetto a dove e' piazzato.
    #[test]
    fn atr_lowers_the_carrier_by_exactly_one_lane() {
        let atr = atr();
        let atr_position = Vec3::new(0.0, MAIN_LANE + LANE_HEIGHT, 0.0);
        let carrier = carrier(CarrierType::WithTube);
        let mut position = Vec3::new(DIVERT_ZONE_HALF_WIDTH, MAIN_LANE + LANE_HEIGHT, 0.0);

        for _ in 0..200 {
            position += carrier_step(&carrier, position, &[(&atr, atr_position)], DELTA);
        }

        assert_eq!(position.y, MAIN_LANE);
    }

    /// Il passo verticale si accorcia sull'ultimo tratto invece di scavalcare la quota.
    #[test]
    fn the_last_step_does_not_overshoot() {
        let atr = atr();
        let carrier = carrier(CarrierType::WithTube);
        let almost_there = Vec3::new(0.0, MAIN_LANE + 1.0, 0.0);

        let atr_position = Vec3::new(0.0, MAIN_LANE + LANE_HEIGHT, 0.0);
        let step = carrier_step(&carrier, almost_there, &[(&atr, atr_position)], DELTA);

        assert_eq!(step.y, -1.0, "scende solo il px che manca");
        assert!(
            BELT_SPEED * DELTA > 1.0,
            "senza il limite avrebbe scavalcato la quota"
        );
    }

    /// Un divert spento non deve deviare niente, nemmeno con un ATR acceso poco
    /// piu' avanti che potrebbe agganciare lo stesso carrier.
    #[test]
    fn a_switched_off_divert_leaves_the_flow_alone() {
        let off_divert = Divert {
            kind: DivertKind::Divert,
            active: false,
        };
        let live_atr = atr();
        let carrier = carrier(CarrierType::WithTube);
        let deviators = [
            (&off_divert, Vec3::new(0.0, MAIN_LANE, 0.0)),
            (
                &live_atr,
                Vec3::new(-2.0 * GRID_STEP, MAIN_LANE + GRID_STEP, 0.0),
            ),
        ];

        let mut position = Vec3::new(200.0, MAIN_LANE, 0.0);
        for _ in 0..400 {
            position += carrier_step(&carrier, position, &deviators, DELTA);
            assert_eq!(
                position.y, MAIN_LANE,
                "il carrier non deve lasciare la corsia a x = {}",
                position.x
            );
        }
    }

    #[test]
    fn the_counter_hands_out_one_id_per_carrier() {
        let mut ids = NextCarrierId::default();

        assert_eq!(ids.take(), 1, "si parte da 1, non da 0");
        assert_eq!(ids.take(), 2);
        assert_eq!(ids.take(), 3);
    }

    #[test]
    fn a_sample_id_longer_than_the_limit_cannot_exist() {
        let limit = "x".repeat(SAMPLE_ID_MAX_LEN);
        let too_long = "x".repeat(SAMPLE_ID_MAX_LEN + 1);

        assert!(SampleId::new(&limit).is_some(), "24 caratteri sono ammessi");
        assert!(SampleId::new(&too_long).is_none());
        assert!(SampleId::new("").is_none(), "un id vuoto e' assenza di id");
    }

    /// Il limite e' sui caratteri: contare i byte scarterebbe id legittimi.
    #[test]
    fn accented_characters_count_as_one() {
        let accented = "à".repeat(SAMPLE_ID_MAX_LEN);

        assert_eq!(accented.len(), SAMPLE_ID_MAX_LEN * 2, "sono 48 byte");
        assert!(SampleId::new(&accented).is_some());
    }

    #[test]
    fn only_carriers_with_a_tube_carry_a_sample() {
        assert!(placeholder_sample_id(42).is_some());
        assert_eq!(
            placeholder_sample_id(42).unwrap().as_str(),
            "SMP-00000042",
            "sta comodamente nei 24 caratteri"
        );
    }

    #[test]
    fn empty_carriers_are_never_diverted() {
        let carrier = carrier(CarrierType::Empty);
        let position = Vec3::new(0.0, MAIN_LANE + LANE_HEIGHT, 0.0);
        let step = carrier_step(&carrier, position, &[(&atr(), position)], DELTA);

        assert_eq!(step.y, 0.0);
    }
}

/// Vero se il carrier ha lasciato del tutto l'area di lavoro. Il conto e' sui
/// confini noti dell'area, non sulla camera: cosi' vale anche senza interfaccia.
fn outside_work_area(translation: Vec3) -> bool {
    translation.x + CARRIER_RADIUS < WORK_AREA_LEFT
        || translation.x - CARRIER_RADIUS > WORK_AREA_RIGHT
        || translation.y + CARRIER_RADIUS < WORK_AREA_BOTTOM
        || translation.y - CARRIER_RADIUS > WORK_AREA_TOP
}

fn despawn_offscreen(mut commands: Commands, query: Query<(Entity, &Transform), With<Carrier>>) {
    for (entity, transform) in query.iter() {
        if outside_work_area(transform.translation) {
            commands.entity(entity).despawn();
        }
    }
}
