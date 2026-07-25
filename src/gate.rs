use bevy::prelude::*;

use crate::geometry::circle_touches_box;
use crate::piece::{self, Arrow, PIECE_SIZE, PieceShapes};

/// Sbarra piazzabile sul percorso: quando e' attiva i carrier si fermano davanti,
/// quando e' spenta li lascia passare. Se e' fuori dal flusso non blocca nessuno,
/// perche' il controllo e' puramente geometrico.
#[derive(Component)]
pub struct Gate {
    pub active: bool,
}

/// Il gate non ha sistemi propri: a bloccare i carrier ci pensa il loro
/// movimento. Qui c'e' solo l'aspetto, quindi si monta solo con l'interfaccia.
pub struct GateVisualsPlugin;

impl Plugin for GateVisualsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_gate_assets)
            .add_systems(Update, (attach_gate_visuals, refresh_gate_colour));
    }
}

#[derive(Resource)]
pub struct GateAssets {
    active_material: Handle<ColorMaterial>,
    idle_material: Handle<ColorMaterial>,
}

fn setup_gate_assets(mut commands: Commands, mut materials: ResMut<Assets<ColorMaterial>>) {
    commands.insert_resource(GateAssets {
        active_material: materials.add(Color::srgb(0.9, 0.1, 0.1)),
        idle_material: materials.add(Color::srgb(0.3, 0.3, 0.3)),
    });
}

/// Vero se un cerchio di raggio `radius` centrato in `point` tocca il gate.
/// Il raggio arriva da fuori: cosi' il gate non ha bisogno di sapere nulla
/// di com'e' fatto un carrier.
pub fn blocks_circle(gate: Vec3, point: Vec3, radius: f32) -> bool {
    circle_touches_box(gate, Vec2::splat(PIECE_SIZE / 2.0), point, radius)
}

/// Piazza un gate gia' attivo. La z lo tiene davanti ai carrier, cosi' la sbarra
/// resta visibile quando si accodano.
pub fn spawn_gate(commands: &mut Commands, position: Vec3) -> Entity {
    commands
        .spawn((Transform::from_translation(position), Gate { active: true }))
        .id()
}

fn material_for(assets: &GateAssets, active: bool) -> Handle<ColorMaterial> {
    if active {
        assets.active_material.clone()
    } else {
        assets.idle_material.clone()
    }
}

fn attach_gate_visuals(
    mut commands: Commands,
    shapes: Res<PieceShapes>,
    assets: Res<GateAssets>,
    gates: Query<(Entity, &Gate), Without<Mesh2d>>,
) {
    for (entity, gate) in gates.iter() {
        piece::dress(
            &mut commands,
            entity,
            &shapes,
            material_for(&assets, gate.active),
            Arrow::None,
        );
    }
}

/// Il colore segue lo stato invece di essere aggiornato da chi lo cambia: chi
/// accende o spegne un gate tocca solo `active` e non deve sapere nulla di mesh.
fn refresh_gate_colour(
    assets: Res<GateAssets>,
    gates: Query<(&Gate, &mut MeshMaterial2d<ColorMaterial>), Changed<Gate>>,
) {
    for (gate, mut material) in gates {
        material.0 = material_for(&assets, gate.active);
    }
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
}
