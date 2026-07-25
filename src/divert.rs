use bevy::prelude::*;

use crate::carrier::Heading;
use crate::piece::{self, Arrow, PieceShapes};

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
    /// Vero se il deviatore, piazzato in `position` e girato verso `facing`,
    /// aggancia un carrier che marcia in `heading`. La fascia copre il
    /// corridoio fra la linea del deviatore e quella di destinazione: ci sta
    /// dentro tutta la manovra, e i carrier che viaggiano altrove non vengono
    /// toccati.
    pub fn catches(
        &self,
        position: Vec3,
        carrier: Vec3,
        facing: Heading,
        heading: Heading,
    ) -> bool {
        if !self.active {
            return false;
        }

        // Un deviatore sposta *di lato*: se la sua freccia e' parallela alla
        // marcia del carrier non c'e' nessun lato verso cui spostarlo, e non
        // deve fare niente. Senza questo controllo i due vincoli qui sotto
        // cadrebbero entrambi sullo stesso asse, lasciando l'altro libero: il
        // deviatore aggancerebbe carrier a qualunque distanza sulla
        // perpendicolare, anche a mezzo impianto di distanza.
        if facing.as_vec().dot(heading.as_vec()) != 0.0 {
            return false;
        }

        let delta = carrier.truncate() - position.truncate();
        if delta.dot(heading.as_vec()).abs() > DIVERT_ZONE_HALF_WIDTH {
            return false;
        }

        let corridor = -DIVERT_LANE_TOLERANCE..=LANE_HEIGHT + DIVERT_LANE_TOLERANCE;

        corridor.contains(&delta.dot(facing.as_vec()))
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
    divert_material: Handle<ColorMaterial>,
    atr_material: Handle<ColorMaterial>,
    idle_material: Handle<ColorMaterial>,
}

fn setup_divert_assets(mut commands: Commands, mut materials: ResMut<Assets<ColorMaterial>>) {
    commands.insert_resource(DivertAssets {
        divert_material: materials.add(Color::srgb(1.0, 0.6, 0.1)),
        // Con l'orientamento i due si comportano allo stesso modo: il colore e'
        // quello che resta a distinguerli a colpo d'occhio.
        atr_material: materials.add(Color::srgb(0.85, 0.40, 0.05)),
        idle_material: materials.add(Color::srgb(0.3, 0.3, 0.3)),
    });
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

fn material_for(assets: &DivertAssets, divert: &Divert) -> Handle<ColorMaterial> {
    match (divert.active, divert.kind) {
        (false, _) => assets.idle_material.clone(),
        (true, DivertKind::Divert) => assets.divert_material.clone(),
        (true, DivertKind::Atr) => assets.atr_material.clone(),
    }
}

fn attach_divert_visuals(
    mut commands: Commands,
    shapes: Res<PieceShapes>,
    assets: Res<DivertAssets>,
    diverts: Query<(Entity, &Divert), Without<Mesh2d>>,
) {
    for (entity, divert) in diverts.iter() {
        piece::dress(
            &mut commands,
            entity,
            &shapes,
            material_for(&assets, divert),
            Arrow::Straight,
        );
    }
}

fn refresh_divert_colour(
    assets: Res<DivertAssets>,
    diverts: Query<(&Divert, &mut MeshMaterial2d<ColorMaterial>), Changed<Divert>>,
) {
    for (divert, mut material) in diverts {
        material.0 = material_for(&assets, divert);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn divert() -> Divert {
        Divert {
            kind: DivertKind::Divert,
            active: true,
        }
    }

    /// La fascia di aggancio copre il corridoio della manovra e niente altro:
    /// dalla linea del deviatore a quella indicata dalla freccia.
    #[test]
    fn the_catch_band_covers_the_manoeuvre_only() {
        let divert = divert();
        let position = Vec3::ZERO;
        let flow = Heading::Left;
        let arrow = Heading::Up;

        assert!(
            divert.catches(position, Vec3::ZERO, arrow, flow),
            "linea del deviatore"
        );
        assert!(
            divert.catches(
                position,
                Vec3::new(-10.0, LANE_HEIGHT / 2.0, 0.0),
                arrow,
                flow
            ),
            "a meta' dello spostamento"
        );
        assert!(
            !divert.catches(position, Vec3::new(0.0, -LANE_HEIGHT, 0.0), arrow, flow),
            "dalla parte opposta alla freccia"
        );
        assert!(
            !divert.catches(position, Vec3::new(60.0, 0.0, 0.0), arrow, flow),
            "fuori dalla finestra di aggancio"
        );
    }

    /// La freccia decide da sola: lo stesso deviatore, girato, prende il
    /// corridoio opposto senza sapere nulla della marcia del carrier.
    #[test]
    fn turning_the_arrow_turns_the_corridor() {
        let divert = divert();
        let position = Vec3::ZERO;
        let flow = Heading::Left;
        let below = Vec3::new(0.0, -LANE_HEIGHT / 2.0, 0.0);

        assert!(!divert.catches(position, below, Heading::Up, flow));
        assert!(divert.catches(position, below, Heading::Down, flow));
    }

    /// Il caso che bloccava un impianto vero: un deviatore girato nel verso del
    /// flusso, quattro celle piu' in alto, agganciava lo stesso i carrier e li
    /// spingeva indietro. Deve ignorarli, per quanto vicini siano sull'altro asse.
    #[test]
    fn an_arrow_parallel_to_the_flow_touches_nobody() {
        let divert = divert();
        let far_above = Vec3::new(192.0, 256.0, 0.0);
        let carrier = Vec3::new(215.0, 0.0, 0.0);

        assert!(
            !divert.catches(far_above, carrier, Heading::Right, Heading::Left),
            "freccia opposta alla marcia"
        );
        assert!(
            !divert.catches(far_above, carrier, Heading::Left, Heading::Left),
            "freccia nel verso della marcia"
        );
        assert!(
            !divert.catches(
                Vec3::ZERO,
                Vec3::new(10.0, 0.0, 0.0),
                Heading::Left,
                Heading::Left
            ),
            "nemmeno quando e' vicino: non c'e' un lato verso cui spostarlo"
        );
    }

    #[test]
    fn inactive_divert_lets_everything_through() {
        let mut off = divert();
        off.active = false;

        assert!(!off.catches(Vec3::ZERO, Vec3::ZERO, Heading::Up, Heading::Left));
    }
}
