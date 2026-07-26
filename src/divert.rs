use bevy::prelude::*;

use crate::carrier::{Carrier, Heading};
use crate::editor::Tool;
use crate::engagement::Engaged;
use crate::piece::{self, PieceShapes};
use crate::switch::{Look, Switch};

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
    /// Se in questo istante ha un carrier nella propria cella. Lo scrive la
    /// simulazione, lo legge il colore.
    pub engaged: bool,
}

impl DivertKind {
    /// Lo strumento che piazza questa variante: serve a chiedere la figura
    /// giusta senza ripetere la mappatura.
    pub fn tool(self) -> Tool {
        match self {
            DivertKind::Divert => Tool::Divert,
            DivertKind::Atr => Tool::Atr,
        }
    }
}

impl Divert {
    /// Cosa succede da spento e' l'unica differenza fra i due. Il divert lascia
    /// proseguire il flusso: la via dritta esiste, semplicemente non viene
    /// deviata. Dall'ATR invece non si passa, perche' quella via dritta non c'e':
    /// spento non devia piu' e i carrier si fermano davanti.
    /// Un ATR disabilitato invece non blocca niente: fuori servizio vuol dire
    /// come se non ci fosse.
    pub fn is_blocking(&self, switch: Switch) -> bool {
        switch.enabled && !switch.active && self.kind == DivertKind::Atr
    }

    /// Da che parte il deviatore sposta un carrier che marcia in `heading`.
    ///
    /// La freccia disegnata e' la **diagonale** fra `facing` e la sua sinistra,
    /// cioe' la traiettoria che il carrier seguira'. Quella diagonale descrive
    /// due sole marce possibili — le sue due componenti — e per ciascuna lo
    /// spostamento e' l'altra componente. Da un'altra direzione la diagonale non
    /// vuol dire niente, e il deviatore non tocca nessuno.
    ///
    /// I due assi risultano cosi' sempre perpendicolari: e' questo che impedisce
    /// a un deviatore di agganciare carrier lontanissimi sulla perpendicolare,
    /// come succedeva quando freccia e marcia cadevano sullo stesso asse.
    pub fn shift(facing: Heading, heading: Heading) -> Option<Heading> {
        if heading == facing {
            Some(facing.turn_left())
        } else if heading == facing.turn_left() {
            Some(facing)
        } else {
            None
        }
    }

    /// Vero se il deviatore, piazzato in `position` e girato verso `facing`,
    /// aggancia un carrier che marcia in `heading`. La fascia copre il
    /// corridoio fra la linea del deviatore e quella di destinazione: ci sta
    /// dentro tutta la manovra, e i carrier che viaggiano altrove non vengono
    /// toccati.
    pub fn catches(
        &self,
        switch: Switch,
        position: Vec3,
        carrier: Vec3,
        facing: Heading,
        heading: Heading,
    ) -> bool {
        let Some(shift) = Divert::shift(facing, heading) else {
            return false;
        };

        let delta = carrier.truncate() - position.truncate();
        if delta.dot(heading.as_vec()).abs() > DIVERT_ZONE_HALF_WIDTH {
            return false;
        }

        let corridor = -DIVERT_LANE_TOLERANCE..=LANE_HEIGHT + DIVERT_LANE_TOLERANCE;
        let offset = delta.dot(shift.as_vec());
        if !corridor.contains(&offset) {
            return false;
        }

        // Chi ha gia' lasciato la sua linea sta a meta' di una manovra, e la
        // manovra va finita anche se nel frattempo il deviatore e' stato spento:
        // abbandonarlo li' lo lascerebbe fra due corsie, fuori dalla griglia e
        // fuori dalla portata di qualsiasi altro oggetto.
        // Il limite superiore e' la destinazione esatta, non la destinazione
        // meno la tolleranza: fermarsi prima lascerebbe il carrier appena fuori
        // corsia, che e' lo stesso difetto in scala ridotta.
        let under_way = (DIVERT_LANE_TOLERANCE..LANE_HEIGHT).contains(&offset);

        switch.working() || under_way
    }
}

/// Come il gate: la deviazione la applica il movimento dei carrier, qui resta
/// solo l'aspetto.

impl Engaged for Divert {
    fn engaged(&self) -> bool {
        self.engaged
    }

    fn set_engaged(&mut self, engaged: bool) {
        self.engaged = engaged;
    }

    fn reaches(
        &self,
        switch: Switch,
        at: Vec3,
        facing: Heading,
        carrier: &Carrier,
        carrier_at: Vec3,
    ) -> bool {
        // La condizione vera: sto spostando proprio questo carrier. Un carrier
        // che passa per la cella senza essere agganciato - perche' arriva da un
        // verso che non tocco, o perche' non sono comandato - non mi riguarda,
        // e non devo accendermi per lui.
        let Some(heading) = carrier.motion.heading() else {
            return false;
        };

        self.catches(switch, at, carrier_at, facing, heading)
    }
}

pub struct DivertVisualsPlugin;

impl Plugin for DivertVisualsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_divert_assets)
            .add_systems(Update, (attach_divert_visuals, refresh_divert_colour));
    }
}

#[derive(Resource)]
pub struct DivertAssets {
    divert: Look,
    atr: Look,
}

fn setup_divert_assets(mut commands: Commands, mut materials: ResMut<Assets<ColorMaterial>>) {
    commands.insert_resource(DivertAssets {
        divert: Look::new(&mut materials, Color::srgb(1.0, 0.6, 0.1)),
        // Con l'orientamento i due si comportano allo stesso modo: il colore e'
        // quello che resta a distinguerli a colpo d'occhio.
        atr: Look::new(&mut materials, Color::srgb(0.85, 0.40, 0.05)),
    });
}

/// Piazza un deviatore gia' attivo.
pub fn spawn_divert(commands: &mut Commands, position: Vec3, kind: DivertKind) -> Entity {
    commands
        .spawn((
            Transform::from_translation(position),
            Divert {
                kind,
                engaged: false,
            },
        ))
        .id()
}

fn material_for(assets: &DivertAssets, divert: &Divert, switch: Switch) -> Handle<ColorMaterial> {
    let look = match divert.kind {
        DivertKind::Divert => &assets.divert,
        DivertKind::Atr => &assets.atr,
    };

    look.material(switch, divert.engaged)
}

fn attach_divert_visuals(
    mut commands: Commands,
    shapes: Res<PieceShapes>,
    assets: Res<DivertAssets>,
    diverts: Query<(Entity, &Divert, &Switch), Without<Mesh2d>>,
) {
    for (entity, divert, switch) in diverts.iter() {
        let (shape, arrow) = piece::dressing(&shapes, divert.kind.tool());

        piece::dress_shape(
            &mut commands,
            entity,
            &shapes,
            shape,
            material_for(&assets, divert, *switch),
            arrow,
        );
    }
}

fn refresh_divert_colour(
    assets: Res<DivertAssets>,
    diverts: Query<
        (&Divert, &Switch, &mut MeshMaterial2d<ColorMaterial>),
        Or<(Changed<Divert>, Changed<Switch>)>,
    >,
) {
    for (divert, switch, mut material) in diverts {
        material.0 = material_for(&assets, divert, *switch);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn divert() -> Divert {
        Divert {
            kind: DivertKind::Divert,
            engaged: false,
        }
    }

    /// In servizio e comandato, e in servizio ma non comandato.
    const ON: Switch = Switch {
        enabled: true,
        active: true,
    };
    const OFF: Switch = Switch {
        enabled: true,
        active: false,
    };

    /// La fascia di aggancio copre il corridoio della manovra e niente altro:
    /// dalla linea del deviatore a quella indicata dalla freccia.
    #[test]
    fn the_catch_band_covers_the_manoeuvre_only() {
        let divert = divert();
        let position = Vec3::ZERO;
        let flow = Heading::Left;
        let arrow = Heading::Up;

        assert!(
            divert.catches(ON, position, Vec3::ZERO, arrow, flow),
            "linea del deviatore"
        );
        assert!(
            divert.catches(
                ON,
                position,
                Vec3::new(-10.0, LANE_HEIGHT / 2.0, 0.0),
                arrow,
                flow
            ),
            "a meta' dello spostamento"
        );
        assert!(
            !divert.catches(ON, position, Vec3::new(0.0, -LANE_HEIGHT, 0.0), arrow, flow),
            "dalla parte opposta alla freccia"
        );
        assert!(
            !divert.catches(ON, position, Vec3::new(60.0, 0.0, 0.0), arrow, flow),
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

        assert!(!divert.catches(ON, position, below, Heading::Up, flow));
        assert!(divert.catches(ON, position, below, Heading::Left, flow));
    }

    /// La stessa freccia serve due flussi, ed e' quello che la rende leggibile:
    /// la diagonale disegnata e' la traiettoria, e lo spostamento e' la
    /// componente che manca alla marcia del carrier.
    #[test]
    fn the_same_diagonal_serves_the_two_flows_it_describes() {
        // Freccia in alto a sinistra: diagonale fra "su" e "sinistra".
        let arrow = Heading::Up;

        assert_eq!(
            Divert::shift(arrow, Heading::Left),
            Some(Heading::Up),
            "chi va a sinistra viene alzato"
        );
        assert_eq!(
            Divert::shift(arrow, Heading::Up),
            Some(Heading::Left),
            "chi sale viene spostato a sinistra"
        );
        assert_eq!(
            Divert::shift(arrow, Heading::Right),
            None,
            "da destra quella diagonale non dice niente"
        );
        assert_eq!(Divert::shift(arrow, Heading::Down), None);
    }

    /// Il caso che bloccava un impianto vero: un deviatore quattro celle piu' in
    /// alto agganciava carrier che non lo riguardavano e li spingeva indietro.
    #[test]
    fn a_far_away_divert_touches_nobody() {
        let divert = divert();
        let far_above = Vec3::new(192.0, 256.0, 0.0);
        let carrier = Vec3::new(215.0, 0.0, 0.0);

        assert!(
            !divert.catches(ON, far_above, carrier, Heading::Right, Heading::Left),
            "la sua diagonale non descrive questa marcia"
        );
        assert!(
            !divert.catches(ON, far_above, carrier, Heading::Left, Heading::Left),
            "la descrive, ma il carrier e' quattro celle fuori dal corridoio"
        );
    }

    #[test]
    fn inactive_divert_lets_everything_through() {
        let divert = divert();

        assert!(!divert.catches(OFF, Vec3::ZERO, Vec3::ZERO, Heading::Up, Heading::Left));
    }

    /// Spegnere un ATR non apre una via dritta, perche' quella via non esiste:
    /// smette di deviare e comincia a sbarrare.
    #[test]
    fn a_switched_off_atr_bars_the_way_instead_of_opening_it() {
        let atr = Divert {
            kind: DivertKind::Atr,
            engaged: false,
        };

        assert!(atr.is_blocking(OFF), "non comandato, l'ATR sbarra");
        assert!(
            !atr.catches(OFF, Vec3::ZERO, Vec3::ZERO, Heading::Up, Heading::Left),
            "e quindi non devia piu'"
        );

        assert!(
            !atr.is_blocking(ON),
            "comandato torna a essere un passaggio"
        );
        assert!(atr.catches(ON, Vec3::ZERO, Vec3::ZERO, Heading::Up, Heading::Left));

        // E fuori servizio non sbarra affatto: e' come se non ci fosse.
        let out_of_service = Switch {
            enabled: false,
            active: false,
        };
        assert!(!atr.is_blocking(out_of_service));
    }

    /// Il divert spento e' invece trasparente: la via dritta c'e' e resta aperta.
    #[test]
    fn a_switched_off_divert_is_transparent() {
        assert!(!divert().is_blocking(OFF));
    }

    /// Spegnere un deviatore mentre un carrier e' a meta' manovra non lo
    /// abbandona fra due corsie: quella manovra la finisce comunque.
    #[test]
    fn a_manoeuvre_already_begun_is_finished_anyway() {
        let mut off = Divert {
            kind: DivertKind::Atr,
            engaged: false,
        };
        let position = Vec3::ZERO;
        let arrow = Heading::Up;
        let flow = Heading::Left;

        let halfway = Vec3::new(0.0, LANE_HEIGHT / 2.0, 0.0);
        assert!(
            off.catches(OFF, position, halfway, arrow, flow),
            "a meta' strada la manovra prosegue"
        );

        let just_arriving = Vec3::new(0.0, 0.0, 0.0);
        assert!(
            !off.catches(OFF, position, just_arriving, arrow, flow),
            "chi deve ancora cominciare non parte nemmeno"
        );

        // E lo stesso vale per il divert: la differenza fra i due e' cosa
        // succede a chi arriva, non a chi e' gia' dentro.
        off.kind = DivertKind::Divert;
        assert!(off.catches(OFF, position, halfway, arrow, flow));
    }
}
