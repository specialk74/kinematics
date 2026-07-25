use std::f32::consts::PI;

use bevy::prelude::*;

use crate::carrier::Heading;

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
    /// Di quanto il deviatore sposta il carrier **di lato rispetto alla sua
    /// marcia**: una cella verso la sua destra per il divert, una verso la sua
    /// sinistra per l'ATR. Riferirlo al carrier e non agli assi e' quello che lo
    /// fa funzionare anche su un flusso verticale: chi sale viene spostato di una
    /// colonna a destra, esattamente come chi va a sinistra viene alzato di una riga.
    pub fn lateral_target(&self) -> f32 {
        match self.kind {
            DivertKind::Divert => LANE_HEIGHT,
            DivertKind::Atr => -LANE_HEIGHT,
        }
    }

    /// Quanto il carrier e' spostato di lato rispetto alla linea del deviatore.
    pub fn lateral_offset(position: Vec3, carrier: Vec3, heading: Heading) -> f32 {
        (carrier.truncate() - position.truncate()).dot(heading.turn_right().as_vec())
    }

    /// Vero se il deviatore, piazzato in `position`, aggancia un carrier che
    /// marcia in `heading`. La fascia copre il corridoio fra la linea del
    /// deviatore e quella di destinazione: ci sta dentro tutta la manovra, e i
    /// carrier che viaggiano altrove non vengono toccati.
    pub fn catches(&self, position: Vec3, carrier: Vec3, heading: Heading) -> bool {
        if !self.active {
            return false;
        }

        let delta = carrier.truncate() - position.truncate();
        if delta.dot(heading.as_vec()).abs() > DIVERT_ZONE_HALF_WIDTH {
            return false;
        }

        let target = self.lateral_target();
        let corridor =
            target.min(0.0) - DIVERT_LANE_TOLERANCE..=target.max(0.0) + DIVERT_LANE_TOLERANCE;

        corridor.contains(&Divert::lateral_offset(position, carrier, heading))
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

    /// Spostamenti opposti e della stessa ampiezza: e' quello che permette a un
    /// divert e a un ATR di annullarsi a vicenda.
    #[test]
    fn the_two_kinds_shift_by_one_cell_in_opposite_directions() {
        assert_eq!(divert(DivertKind::Divert).lateral_target(), LANE_HEIGHT);
        assert_eq!(divert(DivertKind::Atr).lateral_target(), -LANE_HEIGHT);
    }

    /// Il caso che mancava: su un flusso verticale lo spostamento resta "a
    /// destra del carrier", quindi diventa orizzontale.
    #[test]
    fn a_rising_carrier_is_shifted_sideways() {
        let up = divert(DivertKind::Divert);
        let position = Vec3::ZERO;

        // Il carrier sale lungo la colonna del deviatore: sta sulla sua linea.
        assert_eq!(
            Divert::lateral_offset(position, Vec3::ZERO, Heading::Up),
            0.0
        );
        assert!(up.catches(position, Vec3::ZERO, Heading::Up));

        // Una colonna a destra e' la destinazione: li' la manovra e' finita.
        let arrived = Vec3::new(LANE_HEIGHT, 0.0, 0.0);
        assert_eq!(
            Divert::lateral_offset(position, arrived, Heading::Up),
            LANE_HEIGHT
        );
        assert!(up.catches(position, arrived, Heading::Up));

        // Una colonna a sinistra e' fuori dal corridoio.
        assert!(!up.catches(position, Vec3::new(-LANE_HEIGHT, 0.0, 0.0), Heading::Up));
    }

    /// La fascia di aggancio copre il corridoio della manovra e niente altro:
    /// sopra il divert, sotto l'ATR.
    /// Su un flusso verso sinistra la destra del carrier e' l'alto: il divert
    /// guarda sopra di se', l'ATR sotto.
    #[test]
    fn the_catch_band_covers_the_manoeuvre_only() {
        let up = divert(DivertKind::Divert);
        let down = divert(DivertKind::Atr);
        let position = Vec3::ZERO;
        let left = Heading::Left;

        assert!(
            up.catches(position, Vec3::ZERO, left),
            "linea del deviatore"
        );
        assert!(
            up.catches(position, Vec3::new(-10.0, LANE_HEIGHT / 2.0, 0.0), left),
            "a meta' della salita"
        );
        assert!(
            !up.catches(position, Vec3::new(0.0, -LANE_HEIGHT, 0.0), left),
            "il divert non guarda sotto di se'"
        );

        assert!(
            down.catches(position, Vec3::ZERO, left),
            "linea del deviatore"
        );
        assert!(
            down.catches(position, Vec3::new(-10.0, -LANE_HEIGHT / 2.0, 0.0), left),
            "a meta' della discesa"
        );
        assert!(
            !down.catches(position, Vec3::new(0.0, LANE_HEIGHT, 0.0), left),
            "l'ATR non guarda sopra di se'"
        );

        assert!(
            !up.catches(position, Vec3::new(60.0, 0.0, 0.0), left),
            "fuori dalla finestra di aggancio"
        );
    }

    #[test]
    fn inactive_divert_lets_everything_through() {
        let mut up = divert(DivertKind::Divert);
        up.active = false;

        assert!(!up.catches(Vec3::ZERO, Vec3::ZERO, Heading::Left));
    }
}
