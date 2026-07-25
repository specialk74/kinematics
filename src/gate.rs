use bevy::prelude::*;

pub const GATE_WIDTH: f32 = 8.0;
pub const GATE_HEIGHT: f32 = 44.0;

/// Sbarra piazzabile sul percorso: quando e' attiva i carrier si fermano davanti,
/// quando e' spenta li lascia passare. Se e' fuori dal flusso non blocca nessuno,
/// perche' il controllo e' puramente geometrico.
#[derive(Component)]
pub struct Gate {
    pub active: bool,
}

pub struct GatePlugin;

impl Plugin for GatePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_gate_assets)
            .add_systems(Update, place_gate);
    }
}

#[derive(Resource)]
struct GateAssets {
    mesh: Handle<Mesh>,
    active_material: Handle<ColorMaterial>,
    idle_material: Handle<ColorMaterial>,
}

fn setup_gate_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    commands.insert_resource(GateAssets {
        mesh: meshes.add(Rectangle::new(GATE_WIDTH, GATE_HEIGHT)),
        active_material: materials.add(Color::srgb(0.9, 0.1, 0.1)),
        idle_material: materials.add(Color::srgb(0.3, 0.3, 0.3)),
    });
}

/// Vero se un cerchio di raggio `radius` centrato in `point` tocca il gate.
/// Il raggio arriva da fuori: cosi' il gate non ha bisogno di sapere nulla
/// di com'e' fatto un carrier.
pub fn blocks_circle(gate: Vec3, point: Vec3, radius: f32) -> bool {
    let half_gate = Vec2::new(GATE_WIDTH, GATE_HEIGHT) / 2.0;
    let distance = (point.truncate() - gate.truncate()).abs() - half_gate;
    distance.max(Vec2::ZERO).length() < radius
}

/// Vero se il punto cade dentro il rettangolo del gate (usato per i click).
fn contains(gate: Vec3, point: Vec2) -> bool {
    let half_gate = Vec2::new(GATE_WIDTH, GATE_HEIGHT) / 2.0;
    (point - gate.truncate()).abs().cmple(half_gate).all()
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
        if contains(transform.translation, position) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::carrier::CARRIER_RADIUS;

    #[test]
    fn gate_blocks_only_what_passes_through_it() {
        let on_belt = Vec3::new(0.0, 0.0, 1.0);
        let off_belt = Vec3::new(0.0, 200.0, 1.0);
        let carrier_far = Vec3::new(40.0, 0.0, 0.0);
        let carrier_close = Vec3::new(17.0, 0.0, 0.0);

        assert!(
            !blocks_circle(on_belt, carrier_far, CARRIER_RADIUS),
            "carrier ancora lontano dal gate: deve passare"
        );
        assert!(
            blocks_circle(on_belt, carrier_close, CARRIER_RADIUS),
            "carrier arrivato sul gate: deve essere fermato"
        );
        assert!(
            !blocks_circle(off_belt, carrier_close, CARRIER_RADIUS),
            "gate fuori dal flusso: non deve bloccare nessuno"
        );
        assert!(!blocks_circle(off_belt, carrier_far, CARRIER_RADIUS));
    }

    #[test]
    fn click_toggles_only_the_gate_under_the_cursor() {
        let gate = Vec3::new(100.0, 0.0, 1.0);

        assert!(contains(gate, Vec2::new(100.0, 10.0)));
        assert!(!contains(gate, Vec2::new(100.0, 40.0)));
        assert!(!contains(gate, Vec2::new(120.0, 0.0)));
    }
}
