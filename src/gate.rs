use bevy::ecs::query::QueryFilter;
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
        app.add_systems(Startup, setup_gate_assets);
    }
}

#[derive(Resource)]
pub struct GateAssets {
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

/// Piazza un gate gia' attivo. La z lo tiene davanti ai carrier, cosi' la sbarra
/// resta visibile quando si accodano.
pub fn spawn_gate(commands: &mut Commands, assets: &GateAssets, position: Vec3) {
    commands.spawn((
        Mesh2d(assets.mesh.clone()),
        MeshMaterial2d(assets.active_material.clone()),
        Transform::from_translation(position),
        Gate { active: true },
    ));
}

/// Commuta il gate sotto al punto indicato, colore compreso. Restituisce `false`
/// se li' non c'e' nessun gate, cosi' chi chiama sa che il clic e' ancora libero.
pub fn toggle_gate_at<F: QueryFilter>(
    position: Vec2,
    gates: &mut Query<(&mut Gate, &Transform, &mut MeshMaterial2d<ColorMaterial>), F>,
    assets: &GateAssets,
) -> bool {
    for (mut gate, transform, mut material) in gates.iter_mut() {
        if contains(transform.translation, position) {
            gate.active = !gate.active;
            material.0 = if gate.active {
                assets.active_material.clone()
            } else {
                assets.idle_material.clone()
            };
            return true;
        }
    }

    false
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
