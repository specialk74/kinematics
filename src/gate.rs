use bevy::prelude::*;

use crate::carrier::{Blocker, Heading};
use crate::piece::{self, Arrow, BAR_LENGTH, BAR_OFFSET, BAR_THICKNESS, PieceShapes};

/// Sbarra piazzabile sul percorso: quando e' attiva i carrier si fermano davanti,
/// quando e' spenta li lascia passare. Se e' fuori dal flusso non blocca nessuno,
/// perche' il controllo e' puramente geometrico.
///
/// Occupa un lato della cella, quello verso cui e' girata, e non tutta la cella:
/// cosi' il carrier che ferma si arresta quasi al centro della cella accanto,
/// dove puo' esserci un'antenna a leggerlo.
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

/// L'ostacolo che il gate mette sul percorso: la striscia occupata dalla sbarra,
/// non tutta la cella. Chi ferma i carrier non ha bisogno di sapere altro.
pub fn bar(position: Vec3, facing: Heading) -> Blocker {
    let along = facing.as_vec();
    let centre = position + (along * BAR_OFFSET).extend(0.0);

    // La sbarra e' lunga di traverso al verso in cui e' girata.
    let half = Vec2::new(
        along.x.abs() * BAR_THICKNESS + along.y.abs() * BAR_LENGTH,
        along.x.abs() * BAR_LENGTH + along.y.abs() * BAR_THICKNESS,
    ) / 2.0;

    Blocker { centre, half }
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
        piece::dress_shape(
            &mut commands,
            entity,
            &shapes,
            piece::bar(&shapes),
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
    use crate::carrier::{CARRIER_RADIUS, blocks};
    use crate::grid::GRID_STEP;

    /// Un gate girato a sinistra ferma chi arriva da destra, e non tocca chi
    /// passa sulla corsia accanto.
    #[test]
    fn gate_blocks_only_what_passes_through_it() {
        let gate = bar(Vec3::ZERO, Heading::Left);
        let carrier_far = Vec3::new(40.0, 0.0, 0.0);
        let carrier_close = Vec3::new(-10.0, 0.0, 0.0);
        let other_lane = Vec3::new(-10.0, GRID_STEP, 0.0);

        assert!(
            !blocks(gate, carrier_far, CARRIER_RADIUS),
            "carrier ancora lontano dal gate: deve passare"
        );
        assert!(
            blocks(gate, carrier_close, CARRIER_RADIUS),
            "carrier arrivato sulla sbarra: deve essere fermato"
        );
        assert!(
            !blocks(gate, other_lane, CARRIER_RADIUS),
            "gate fuori dal flusso: non deve bloccare nessuno"
        );
    }

    /// La sbarra sta tutta dentro la propria cella, appoggiata al confine: due
    /// gate accostati fanno un muro continuo invece di sovrapporsi.
    #[test]
    fn the_bar_stays_inside_its_own_cell() {
        let Blocker { centre, half } = bar(Vec3::ZERO, Heading::Left);

        assert_eq!(centre.x + half.x, -GRID_STEP / 2.0 + BAR_THICKNESS);
        assert!(centre.x - half.x >= -GRID_STEP / 2.0);
        assert_eq!(half.y, BAR_LENGTH / 2.0, "lunga di traverso alla marcia");
    }
}
