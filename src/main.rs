use bevy::prelude::*;
use rand::prelude::*;

pub const CARRIER_SPAWN_TIME: f32 = 0.500;
pub const BELT_SPEED: f32 = 100.0;
pub const CARRIER_DIVERT_SPEED: f32 = 50.0;
pub const CARRIER_RADIUS: f32 = 15.0;
pub const CARRIER_THICKNESS: f32 = 3.0;
pub const CARRIER_SIZE: f32 = CARRIER_RADIUS * 2.0 + 4.0;
pub const GATE_WIDTH: f32 = 8.0;
pub const GATE_HEIGHT: f32 = 44.0;
pub const WIDTH: u32 = 1024;
pub const HEIGTH: u32 = 768;

#[derive(PartialEq)]
pub enum CarrierType {
    Empty,
    WithTube,
}

#[derive(Component)]
struct Carrier(CarrierType);

#[derive(Component)]
struct Tube;

#[derive(PartialEq)]
pub enum DivertType {
    Std,
    Nsd,
}

#[derive(Component)]
struct Divert(DivertType);

/// Sbarra piazzabile sul percorso: quando e' attiva i carrier si fermano davanti,
/// quando e' spenta li lascia passare. Se e' fuori dal flusso non blocca nessuno,
/// perche' il controllo e' puramente geometrico.
#[derive(Component)]
struct Gate {
    active: bool,
}

fn main() {
    App::new()
        .init_resource::<CarrierSpawnTimer>()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Carrier Flow".to_string(),
                resolution: (WIDTH, HEIGTH).into(),
                ..Default::default()
            }),
            ..Default::default()
        }))
        .add_systems(Startup, setup)
        .add_systems(Update, move_carrier)
        .add_systems(Update, tick_carrier_spawn_timer)
        .add_systems(Update, spawn_carrier)
        .add_systems(Update, despawn_offscreen)
        .add_systems(Update, place_gate)
        .run();
}

/// Mesh e materiali dei carrier, creati una volta sola: se venissero costruiti a
/// ogni spawn si accumulerebbero asset identici per tutta la durata del gioco.
#[derive(Resource)]
struct CarrierAssets {
    empty_mesh: Handle<Mesh>,
    empty_material: Handle<ColorMaterial>,
    with_tube_mesh: Handle<Mesh>,
    with_tube_material: Handle<ColorMaterial>,
}

#[derive(Resource)]
struct GateAssets {
    mesh: Handle<Mesh>,
    active_material: Handle<ColorMaterial>,
    idle_material: Handle<ColorMaterial>,
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    commands.spawn(Camera2d);

    commands.insert_resource(GateAssets {
        mesh: meshes.add(Rectangle::new(GATE_WIDTH, GATE_HEIGHT)),
        active_material: materials.add(Color::srgb(0.9, 0.1, 0.1)),
        idle_material: materials.add(Color::srgb(0.3, 0.3, 0.3)),
    });

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

    commands.spawn((
        Text2d::new("@"),
        TextFont {
            font_size: 12.0,
            font: default(),
            ..default()
        },
        TextColor(Color::WHITE),
        Transform::from_translation(Vec3::ZERO),
        Divert(DivertType::Nsd),
    ));
}

#[derive(Resource, Deref, DerefMut)]
pub struct CarrierSpawnTimer(Timer);

impl Default for CarrierSpawnTimer {
    fn default() -> Self {
        CarrierSpawnTimer(Timer::from_seconds(
            CARRIER_SPAWN_TIME,
            TimerMode::Repeating,
        ))
    }
}

pub fn tick_carrier_spawn_timer(mut timer: ResMut<CarrierSpawnTimer>, time: Res<Time>) {
    timer.tick(time.delta());
}

fn spawn_carrier(
    mut commands: Commands,
    timer: Res<CarrierSpawnTimer>,
    carriers: Query<&Transform, With<Carrier>>,
    carrier_assets: Res<CarrierAssets>,
) {
    if !timer.is_finished() {
        return;
    }

    let spawn = Vec3 {
        x: WIDTH as f32 / 2.0 - 50.0,
        y: 0.0,
        z: 0.0,
    };

    // Non far entrare un carrier sopra a uno ancora fermo in coda all'ingresso.
    if carriers
        .iter()
        .any(|transform| transform.translation.distance(spawn) < CARRIER_SIZE)
    {
        return;
    }

    let mut rng = rand::rng();
    if rng.random::<u32>() > u32::MAX / 2 {
        commands.spawn((
            Mesh2d(carrier_assets.with_tube_mesh.clone()),
            MeshMaterial2d(carrier_assets.with_tube_material.clone()),
            Transform::from_translation(spawn),
            Carrier(CarrierType::WithTube),
            children![Tube],
        ));
    } else {
        commands.spawn((
            Mesh2d(carrier_assets.empty_mesh.clone()),
            MeshMaterial2d(carrier_assets.empty_material.clone()),
            Transform::from_translation(spawn),
            Carrier(CarrierType::Empty),
        ));
    }
}

/// Click sinistro: piazza un gate attivo sotto al mouse. Se il click cade su un
/// gate gia' esistente ne commuta lo stato, cosi' si puo' aprire e chiudere il
/// flusso restando sullo stesso pulsante.
fn place_gate(
    mut commands: Commands,
    buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform)>,
    mut gates: Query<(&mut Gate, &Transform, &mut MeshMaterial2d<ColorMaterial>)>,
    gate_assets: Res<GateAssets>,
) {
    if !buttons.just_pressed(MouseButton::Left) {
        return;
    }

    let Ok(window) = windows.single() else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    let Ok((camera, camera_transform)) = camera_query.single() else {
        return;
    };
    let Ok(position) = camera.viewport_to_world_2d(camera_transform, cursor) else {
        return;
    };

    for (mut gate, transform, mut material) in gates.iter_mut() {
        let half_gate = Vec2::new(GATE_WIDTH, GATE_HEIGHT) / 2.0;
        if (position - transform.translation.truncate())
            .abs()
            .cmple(half_gate)
            .all()
        {
            gate.active = !gate.active;
            material.0 = if gate.active {
                gate_assets.active_material.clone()
            } else {
                gate_assets.idle_material.clone()
            };
            return;
        }
    }

    commands.spawn((
        Mesh2d(gate_assets.mesh.clone()),
        MeshMaterial2d(gate_assets.active_material.clone()),
        // z davanti ai carrier, cosi' la sbarra resta visibile quando si accodano.
        Transform::from_translation(position.extend(1.0)),
        Gate { active: true },
    ));
}

fn carrier_velocity(carrier: &Carrier, translation: Vec3) -> Vec3 {
    if carrier.0 == CarrierType::WithTube {
        let pos = translation.x.abs();
        if pos < 16.0 {
            Vec3::new(-CARRIER_DIVERT_SPEED, BELT_SPEED, 0.0)
        } else if translation.x < -300.0 && translation.x > -(300.0 + 32.0) {
            Vec3::new(-CARRIER_DIVERT_SPEED, -BELT_SPEED, 0.0)
        } else {
            Vec3::new(-BELT_SPEED, 0.0, 0.0)
        }
    } else {
        Vec3::new(-BELT_SPEED, 0.0, 0.0)
    }
}

/// Il carrier e' un cerchio, il gate un rettangolo: si misura la distanza dal
/// punto del rettangolo piu' vicino al centro del carrier.
fn carrier_touches_gate(carrier: Vec3, gate: Vec3) -> bool {
    let half_gate = Vec2::new(GATE_WIDTH, GATE_HEIGHT) / 2.0;
    let distance = (carrier.truncate() - gate.truncate()).abs() - half_gate;
    distance.max(Vec2::ZERO).length() < CARRIER_RADIUS
}

fn move_carrier(
    time: Res<Time>,
    mut query: Query<(Entity, &Carrier, &mut Transform)>,
    gates: Query<(&Gate, &Transform), Without<Carrier>>,
) {
    let delta_secs = time.delta_secs();

    let active_gates: Vec<Vec3> = gates
        .iter()
        .filter(|(gate, _)| gate.active)
        .map(|(_, transform)| transform.translation)
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
                carrier_velocity(carrier, translation) * delta_secs,
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
                carrier_touches_gate(candidate, *gate) && !carrier_touches_gate(translation, *gate)
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

    #[test]
    fn gate_blocks_only_what_passes_through_it() {
        let on_belt = Vec3::new(0.0, 0.0, 1.0);
        let off_belt = Vec3::new(0.0, 200.0, 1.0);
        let carrier_far = Vec3::new(40.0, 0.0, 0.0);
        let carrier_close = Vec3::new(17.0, 0.0, 0.0);

        assert!(
            !carrier_touches_gate(carrier_far, on_belt),
            "carrier ancora lontano dal gate: deve passare"
        );
        assert!(
            carrier_touches_gate(carrier_close, on_belt),
            "carrier arrivato sul gate: deve essere fermato"
        );
        assert!(
            !carrier_touches_gate(carrier_close, off_belt),
            "gate fuori dal flusso: non deve bloccare nessuno"
        );
        assert!(!carrier_touches_gate(carrier_far, off_belt));
    }
}

fn despawn_offscreen(
    mut commands: Commands,
    query: Query<(Entity, &Transform), With<Carrier>>,
    camera_query: Query<(&Camera, &GlobalTransform)>,
) {
    let (camera, camera_transform) = camera_query.single().unwrap();
    for (entity, transform) in query.iter() {
        if let Some(pos) = camera.world_to_ndc(camera_transform, transform.translation) {
            // Add a buffer (e.g., 1.2 instead of 1.0) to hide things before they pop out
            if pos.x.abs() > 1.2 || pos.y.abs() > 1.2 {
                commands.entity(entity).despawn();
            }
        }
    }
}
