use bevy::prelude::*;
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

#[derive(PartialEq)]
pub enum CarrierType {
    Empty,
    WithTube,
}

/// Il carrier non sa niente del percorso: sono i deviatori che incontra a dirgli
/// dove andare.
#[derive(Component)]
pub struct Carrier {
    pub kind: CarrierType,
}

#[derive(Component)]
pub struct Tube;

/// La cinematica: nessun riferimento a mesh, materiali o camera, cosi' gira
/// anche senza interfaccia.
pub struct CarrierPlugin;

impl Plugin for CarrierPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
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
            .insert((Mesh2d(mesh), MeshMaterial2d(material)));
    }
}

/// Immette un carrier nel flusso; il tipo (vuoto o con tubo) e' casuale 50-50.
pub fn spawn_random_carrier(commands: &mut Commands, position: Vec3) {
    let mut rng = rand::rng();

    if rng.random::<u32>() > u32::MAX / 2 {
        commands.spawn((
            Transform::from_translation(position),
            Carrier {
                kind: CarrierType::WithTube,
            },
            children![Tube],
        ));
    } else {
        commands.spawn((
            Transform::from_translation(position),
            Carrier {
                kind: CarrierType::Empty,
            },
        ));
    }
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
        Carrier { kind }
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
