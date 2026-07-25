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
    /// Quota a cui il deviatore porta i carrier che aggancia: una corsia sopra la
    /// propria per il divert, una sotto per l'ATR. Il riferimento e' dove sta il
    /// deviatore, non da dove viene il carrier — che quindi non ha bisogno di
    /// ricordarsi niente. A far tornare i conti e' la griglia: ha il passo di una
    /// corsia, quindi un divert e l'ATR della riga sopra si compongono esatti.
    pub fn target_y(&self, own_y: f32) -> f32 {
        match self.kind {
            DivertKind::Divert => own_y + LANE_HEIGHT,
            DivertKind::Atr => own_y - LANE_HEIGHT,
        }
    }

    /// Vero se il deviatore, piazzato in `position`, sta agganciando il carrier.
    /// La fascia copre il corridoio fra la corsia del deviatore e quella di
    /// destinazione: ci sta dentro tutta la manovra, e i carrier che viaggiano
    /// altrove non vengono toccati.
    pub fn catches(&self, position: Vec3, carrier: Vec3) -> bool {
        if !self.active {
            return false;
        }

        if (carrier.x - position.x).abs() > DIVERT_ZONE_HALF_WIDTH {
            return false;
        }

        let height = carrier.y - position.y;
        match self.kind {
            DivertKind::Divert => {
                (-DIVERT_LANE_TOLERANCE..=LANE_HEIGHT + DIVERT_LANE_TOLERANCE).contains(&height)
            }
            DivertKind::Atr => {
                (-LANE_HEIGHT - DIVERT_LANE_TOLERANCE..=DIVERT_LANE_TOLERANCE).contains(&height)
            }
        }
    }
}

/// Come il gate: la deviazione la applica il movimento dei carrier, qui resta
/// solo l'aspetto.
pub struct DivertVisualsPlugin;

impl Plugin for DivertVisualsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_divert_assets)
            .add_systems(Update, (attach_divert_visuals, refresh_divert_colour));
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
pub fn spawn_divert(commands: &mut Commands, position: Vec3, kind: DivertKind) -> Entity {
    commands
        .spawn((
            Transform::from_translation(position),
            Divert { kind, active: true },
        ))
        .id()
}

fn material_for(assets: &DivertAssets, active: bool) -> Handle<ColorMaterial> {
    if active {
        assets.active_material.clone()
    } else {
        assets.idle_material.clone()
    }
}

fn attach_divert_visuals(
    mut commands: Commands,
    assets: Res<DivertAssets>,
    diverts: Query<(Entity, &Divert, &mut Transform), Without<Mesh2d>>,
) {
    for (entity, divert, mut transform) in diverts {
        let (mesh, rotation) = shape(&assets, divert.kind);
        transform.rotation = rotation;

        commands.entity(entity).insert((
            Mesh2d(mesh),
            MeshMaterial2d(material_for(&assets, divert.active)),
        ));
    }
}

fn refresh_divert_colour(
    assets: Res<DivertAssets>,
    diverts: Query<(&Divert, &mut MeshMaterial2d<ColorMaterial>), Changed<Divert>>,
) {
    for (divert, mut material) in diverts {
        material.0 = material_for(&assets, divert.active);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn divert(kind: DivertKind) -> Divert {
        Divert { kind, active: true }
    }

    /// Traslazioni opposte e della stessa ampiezza: e' quello che permette a un
    /// divert e a un ATR di annullarsi a vicenda.
    #[test]
    fn the_two_kinds_translate_by_one_lane_in_opposite_directions() {
        let lane = -120.0;

        assert_eq!(
            divert(DivertKind::Divert).target_y(lane),
            lane + LANE_HEIGHT
        );
        assert_eq!(divert(DivertKind::Atr).target_y(lane), lane - LANE_HEIGHT);
    }

    /// La fascia di aggancio copre il corridoio della manovra e niente altro:
    /// sopra il divert, sotto l'ATR.
    #[test]
    fn the_catch_band_covers_the_manoeuvre_only() {
        let up = divert(DivertKind::Divert);
        let down = divert(DivertKind::Atr);
        let position = Vec3::ZERO;

        assert!(up.catches(position, Vec3::ZERO), "corsia del deviatore");
        assert!(
            up.catches(position, Vec3::new(-10.0, LANE_HEIGHT / 2.0, 0.0)),
            "a meta' della salita"
        );
        assert!(
            !up.catches(position, Vec3::new(0.0, -LANE_HEIGHT, 0.0)),
            "il divert non guarda sotto di se'"
        );

        assert!(down.catches(position, Vec3::ZERO), "corsia del deviatore");
        assert!(
            down.catches(position, Vec3::new(-10.0, -LANE_HEIGHT / 2.0, 0.0)),
            "a meta' della discesa"
        );
        assert!(
            !down.catches(position, Vec3::new(0.0, LANE_HEIGHT, 0.0)),
            "l'ATR non guarda sopra di se'"
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
