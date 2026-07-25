use std::f32::consts::PI;

use bevy::ecs::query::QueryFilter;
use bevy::prelude::*;

pub const DIVERT_SIZE: f32 = 30.0;
/// Semilarghezza della finestra orizzontale in cui il divert agisce. E' questa a
/// decidere di quanto sale la corsia deviata: il carrier viene spinto in verticale
/// per tutto il tempo che impiega ad attraversarla.
pub const DIVERT_ZONE_HALF_WIDTH: f32 = 16.0;
/// Sotto questo dislivello il carrier e' considerato "arrivato" sulla quota del divert.
const DIVERT_LANE_TOLERANCE: f32 = 4.0;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DivertDirection {
    Up,
    Down,
}

/// Deviatore: fa uscire i carrier dalla corsia (`Up`) o ce li fa rientrare (`Down`).
/// Se ne piazza uno per estremo della corsia deviata.
#[derive(Component)]
pub struct Divert {
    pub direction: DivertDirection,
    pub active: bool,
}

impl Divert {
    /// Vero se il divert, piazzato in `position`, sta deviando il carrier in `carrier`.
    pub fn catches(&self, position: Vec3, carrier: Vec3) -> bool {
        if !self.active {
            return false;
        }

        if (carrier.x - position.x).abs() > DIVERT_ZONE_HALF_WIDTH {
            return false;
        }

        match self.direction {
            // L'uscita spinge in alto tutto quello che gli passa sopra: quanto sale
            // dipende solo da quanto ci mette ad attraversare la finestra.
            DivertDirection::Up => carrier.y >= position.y - DIVERT_LANE_TOLERANCE,
            // Il rientro agisce solo su chi sta piu' in alto di lui e smette da
            // solo quando il carrier e' tornato sulla sua quota.
            DivertDirection::Down => carrier.y > position.y + DIVERT_LANE_TOLERANCE,
        }
    }

    /// Verso della spinta verticale: la velocita' vera la decide il carrier.
    pub fn lift_sign(&self) -> f32 {
        match self.direction {
            DivertDirection::Up => 1.0,
            DivertDirection::Down => -1.0,
        }
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

/// Piazza un divert gia' attivo. Il triangolo e' ruotato nel verso della deviazione.
pub fn spawn_divert(
    commands: &mut Commands,
    assets: &DivertAssets,
    position: Vec3,
    direction: DivertDirection,
) {
    let rotation = match direction {
        DivertDirection::Up => 0.0,
        DivertDirection::Down => PI,
    };

    commands.spawn((
        Mesh2d(assets.mesh.clone()),
        MeshMaterial2d(assets.active_material.clone()),
        Transform::from_translation(position).with_rotation(Quat::from_rotation_z(rotation)),
        Divert {
            direction,
            active: true,
        },
    ));
}

/// Commuta il divert sotto al punto indicato, colore compreso. Restituisce `false`
/// se li' non c'e' nessun divert, cosi' chi chiama sa che il clic e' ancora libero.
pub fn toggle_divert_at<F: QueryFilter>(
    position: Vec2,
    diverts: &mut Query<(&mut Divert, &Transform, &mut MeshMaterial2d<ColorMaterial>), F>,
    assets: &DivertAssets,
) -> bool {
    for (mut divert, transform, mut material) in diverts.iter_mut() {
        if contains(transform.translation, position) {
            divert.active = !divert.active;
            material.0 = if divert.active {
                assets.active_material.clone()
            } else {
                assets.idle_material.clone()
            };
            return true;
        }
    }

    false
}

/// Vero se il punto cade sul divert (usato per i click).
fn contains(divert: Vec3, point: Vec2) -> bool {
    (point - divert.truncate())
        .abs()
        .cmple(Vec2::splat(DIVERT_SIZE / 2.0))
        .all()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn divert(direction: DivertDirection) -> Divert {
        Divert {
            direction,
            active: true,
        }
    }

    #[test]
    fn exit_divert_lifts_carriers_on_its_lane() {
        let up = divert(DivertDirection::Up);
        let position = Vec3::ZERO;

        assert!(
            up.catches(position, Vec3::new(0.0, 0.0, 0.0)),
            "carrier in arrivo"
        );
        assert!(
            up.catches(position, Vec3::new(-10.0, 30.0, 0.0)),
            "sta ancora salendo dentro la finestra"
        );
        assert!(
            !up.catches(position, Vec3::new(60.0, 0.0, 0.0)),
            "fuori dalla finestra orizzontale"
        );
    }

    #[test]
    fn rejoin_divert_only_touches_carriers_above_it() {
        let down = divert(DivertDirection::Down);
        let position = Vec3::ZERO;

        assert!(
            down.catches(position, Vec3::new(0.0, 64.0, 0.0)),
            "carrier sulla corsia deviata: deve scendere"
        );
        assert!(
            !down.catches(position, Vec3::new(0.0, 0.0, 0.0)),
            "carrier gia' sulla corsia principale: non deve essere spinto sotto"
        );
    }

    #[test]
    fn inactive_divert_lets_everything_through() {
        let mut up = divert(DivertDirection::Up);
        up.active = false;

        assert!(!up.catches(Vec3::ZERO, Vec3::ZERO));
    }
}
