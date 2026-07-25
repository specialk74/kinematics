use std::f32::consts::PI;

use bevy::prelude::*;

pub const DIVERT_SIZE: f32 = 30.0;
/// Dislivello fra la corsia principale e quella deviata.
pub const LANE_HEIGHT: f32 = 64.0;
/// Semilarghezza della finestra orizzontale in cui il deviatore aggancia i carrier.
/// Deve bastare a completare il cambio di corsia: servono
/// `LANE_HEIGHT * CARRIER_DIVERT_SPEED / BELT_SPEED` = 32 px orizzontali.
pub const DIVERT_ZONE_HALF_WIDTH: f32 = 24.0;
/// Margine verticale con cui si riconosce un carrier "sulla corsia".
const DIVERT_LANE_TOLERANCE: f32 = 4.0;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DivertKind {
    /// Uscita: porta i carrier sulla corsia deviata.
    Divert,
    /// Rientro: riporta i carrier sulla corsia principale.
    Atr,
}

/// Deviatore. Se ne piazza uno per estremo della corsia deviata: un `Divert`
/// dove i carrier devono uscire, un `Atr` dove devono rientrare.
#[derive(Component)]
pub struct Divert {
    pub kind: DivertKind,
    pub active: bool,
}

impl Divert {
    /// Quota a cui questo deviatore porta un carrier partito da `home_lane`.
    /// E' il punto fisso del movimento: il carrier ci arriva esatto e li' smette
    /// di salire o scendere. Nota che non dipende da dove e' piazzato il
    /// deviatore, ma dalla corsia della sorgente: e' cosi' che l'ATR riporta il
    /// flusso sulla linea di partenza anche se lo piazzi sulla corsia deviata.
    pub fn target_y(&self, home_lane: f32) -> f32 {
        match self.kind {
            DivertKind::Divert => home_lane + LANE_HEIGHT,
            DivertKind::Atr => home_lane,
        }
    }

    /// Vero se il deviatore, piazzato in `position`, sta agganciando il carrier.
    /// La fascia copre una corsia sopra e una sotto, cosi' il marcatore lavora
    /// sia che lo si metta sulla corsia principale sia su quella deviata.
    pub fn catches(&self, position: Vec3, carrier: Vec3) -> bool {
        if !self.active {
            return false;
        }

        if (carrier.x - position.x).abs() > DIVERT_ZONE_HALF_WIDTH {
            return false;
        }

        (carrier.y - position.y).abs() <= LANE_HEIGHT + DIVERT_LANE_TOLERANCE
    }
}

pub struct DivertPlugin;

impl Plugin for DivertPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_divert_assets);
    }
}

#[derive(Resource)]
pub struct DivertAssets {
    mesh: Handle<Mesh>,
    active_material: Handle<ColorMaterial>,
    idle_material: Handle<ColorMaterial>,
}

fn setup_divert_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    commands.insert_resource(DivertAssets {
        mesh: meshes.add(RegularPolygon::new(DIVERT_SIZE / 2.0, 3)),
        active_material: materials.add(Color::srgb(1.0, 0.6, 0.1)),
        idle_material: materials.add(Color::srgb(0.3, 0.3, 0.3)),
    });
}

/// Mesh e orientamento del deviatore: il triangolo punta nel verso della
/// deviazione. La usa anche l'anteprima dell'editor.
pub fn shape(assets: &DivertAssets, kind: DivertKind) -> (Handle<Mesh>, Quat) {
    let rotation = match kind {
        DivertKind::Divert => Quat::IDENTITY,
        DivertKind::Atr => Quat::from_rotation_z(PI),
    };

    (assets.mesh.clone(), rotation)
}

/// Piazza un deviatore gia' attivo.
pub fn spawn_divert(
    commands: &mut Commands,
    assets: &DivertAssets,
    position: Vec3,
    kind: DivertKind,
) -> Entity {
    let (mesh, rotation) = shape(assets, kind);

    commands
        .spawn((
            Mesh2d(mesh),
            MeshMaterial2d(assets.active_material.clone()),
            Transform::from_translation(position).with_rotation(rotation),
            Divert { kind, active: true },
        ))
        .id()
}

/// Accende o spegne il deviatore, colore compreso.
pub fn toggle_divert(
    divert: &mut Divert,
    material: &mut MeshMaterial2d<ColorMaterial>,
    assets: &DivertAssets,
) {
    divert.active = !divert.active;
    material.0 = if divert.active {
        assets.active_material.clone()
    } else {
        assets.idle_material.clone()
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn divert(kind: DivertKind) -> Divert {
        Divert { kind, active: true }
    }

    #[test]
    fn the_two_kinds_target_opposite_lanes() {
        assert_eq!(divert(DivertKind::Divert).target_y(0.0), LANE_HEIGHT);
        assert_eq!(divert(DivertKind::Atr).target_y(0.0), 0.0);
    }

    /// Il caso che conta: l'ATR piazzato sulla corsia deviata deve comunque
    /// riportare il carrier sulla linea della sorgente, non sulla propria.
    #[test]
    fn atr_targets_the_source_lane_even_when_placed_on_the_diverted_lane() {
        let source_lane = -120.0;
        let atr = divert(DivertKind::Atr);
        let atr_position = Vec3::new(0.0, source_lane + LANE_HEIGHT, 0.0);
        let carrier = Vec3::new(0.0, source_lane + LANE_HEIGHT, 0.0);

        assert!(atr.catches(atr_position, carrier));
        assert_eq!(atr.target_y(source_lane), source_lane);
    }

    #[test]
    fn a_deviator_works_from_either_lane() {
        let up = divert(DivertKind::Divert);
        let position = Vec3::ZERO;

        assert!(
            up.catches(position, Vec3::new(0.0, 0.0, 0.0)),
            "corsia propria"
        );
        assert!(
            up.catches(position, Vec3::new(-10.0, 30.0, 0.0)),
            "a meta' del cambio di corsia"
        );
        assert!(
            up.catches(position, Vec3::new(0.0, LANE_HEIGHT, 0.0)),
            "una corsia piu' su: e' il caso dell'ATR piazzato in alto"
        );
        assert!(
            !up.catches(position, Vec3::new(0.0, 2.0 * LANE_HEIGHT, 0.0)),
            "due corsie piu' su: troppo lontano"
        );
        assert!(
            !up.catches(position, Vec3::new(60.0, 0.0, 0.0)),
            "fuori dalla finestra orizzontale"
        );
    }

    #[test]
    fn inactive_divert_lets_everything_through() {
        let mut up = divert(DivertKind::Divert);
        up.active = false;

        assert!(!up.catches(Vec3::ZERO, Vec3::ZERO));
    }
}
