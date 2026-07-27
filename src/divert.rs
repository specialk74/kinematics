use bevy::prelude::*;

use crate::carrier::{Carrier, CarrierType, Heading};
use crate::engagement::Engaged;
use crate::piece::{self, PieceShapes, Tool};
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
    /// Vale sia da disabilitato sia da non comandato, e non e' un dettaglio:
    /// nella corsia secondaria la via dritta non esiste, quindi un ATR che non
    /// lavora non e' un passaggio aperto - e' un fondo cieco. Il carrier resta
    /// li' perche' gli manca la spinta per rientrare, non perche' qualcuno gli
    /// abbia messo una sbarra davanti.
    pub fn is_blocking(&self, switch: Switch) -> bool {
        !switch.working() && self.kind == DivertKind::Atr
    }

    /// Su chi agisce. Per ora il divert e' uno smistatore e prende solo i
    /// carrier con la provetta; l'ATR li riporta indietro tutti, altrimenti un
    /// vuoto finito sul ramo secondario non ne uscirebbe piu'.
    ///
    /// E' una semplificazione in attesa di mqtt: la cinematica non dovrebbe
    /// guardare dentro al carrier, e quando ci sara' la logica di impianto sara'
    /// lei a dire chi deviare e chi no.
    pub fn takes(&self, kind: CarrierType) -> bool {
        match self.kind {
            DivertKind::Divert => kind == CarrierType::WithTube,
            DivertKind::Atr => true,
        }
    }

    /// Quanto manca ancora al carrier per arrivare nella corsia di destinazione,
    /// se questo deviatore lo sta portando. Zero vuol dire che c'e' gia': e'
    /// nel corridoio della manovra ma non c'e' piu' niente da spostare.
    pub fn still_to_go(
        &self,
        position: Vec3,
        carrier: Vec3,
        facing: Heading,
        heading: Heading,
    ) -> Option<f32> {
        let shift = self.shift(facing, heading)?;
        let offset = (carrier.truncate() - position.truncate()).dot(shift.as_vec());

        Some((LANE_HEIGHT - offset).max(0.0))
    }

    /// Da che parte il deviatore sposta un carrier che marcia in `heading`, e
    /// `None` se quel carrier non lo riguarda.
    ///
    /// La figura disegnata e' la **diagonale** fra `facing` e la sua sinistra:
    /// come traiettoria descrive due marce possibili - le sue due componenti - e
    /// per ciascuna lo spostamento sarebbe l'altra. Un oggetto solo potrebbe
    /// quindi servire due flussi con corsie perpendicolari, e per un po' l'ha
    /// fatto. Il guaio e' che le due corsie hanno anche i fianchi perpendicolari:
    /// finche' non si sa quale dei due flussi e' in uso, il disegno del bordo
    /// non puo' essere giusto, e infatti sbagliava meta' dei casi.
    ///
    /// Adesso a scegliere e' il **tipo**, e non a caso: divert e ATR sono la
    /// stessa manovra a specchio. Il divert porta il carrier alla sua destra per
    /// toglierlo dalla linea, l'ATR lo riporta alla sua sinistra per rimettercelo.
    /// Con lo spostamento deciso, la marcia che l'oggetto aggancia e' una sola, e
    /// il disegno puo' dire la verita'.
    ///
    /// I due assi restano perpendicolari, che e' cio' che impedisce a un
    /// deviatore di agganciare carrier lontanissimi sulla perpendicolare.
    pub fn shift(&self, facing: Heading, heading: Heading) -> Option<Heading> {
        match self.kind {
            // Il verso indica lo spostamento; il carrier arriva di traverso.
            DivertKind::Divert => (heading == facing.turn_left()).then_some(facing),
            // Il verso indica la marcia agganciata; lo spostamento e' la sua
            // sinistra, cioe' il contrario di quello che fa il divert.
            DivertKind::Atr => (heading == facing).then(|| facing.turn_left()),
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
        let Some(shift) = self.shift(facing, heading) else {
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

        // Il corridoio della manovra arriva fino alla corsia di destinazione,
        // quindi comprende anche chi ci sta gia' viaggiando dentro: quello e'
        // agganciato ma non viene spostato di un millimetro, e non deve far
        // accendere niente. Si accende solo se c'e' ancora strada da fargli fare.
        let arriving = self
            .still_to_go(at, carrier_at, facing, heading)
            .is_some_and(|left| left > DIVERT_LANE_TOLERANCE);

        arriving
            && self.takes(carrier.kind)
            && self.catches(switch, at, carrier_at, facing, heading)
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

    fn atr() -> Divert {
        Divert {
            kind: DivertKind::Atr,
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

    /// La freccia decide tutta la manovra: girato dall'altra parte, lo stesso
    /// deviatore aggancia il flusso opposto e lo porta dalla parte opposta.
    #[test]
    fn turning_the_arrow_turns_the_whole_manoeuvre() {
        let divert = divert();
        let position = Vec3::ZERO;
        let below = Vec3::new(0.0, -LANE_HEIGHT / 2.0, 0.0);

        assert!(
            !divert.catches(ON, position, below, Heading::Up, Heading::Left),
            "girato in su alza chi va a sinistra, e sotto non ha corridoio"
        );
        assert!(
            divert.catches(ON, position, below, Heading::Down, Heading::Right),
            "girato in giu' abbassa chi va a destra, e quel corridoio scende"
        );
    }

    /// Divert e ATR sono la stessa manovra a specchio, e ciascuno serve un
    /// flusso solo: il divert porta il carrier alla propria destra per toglierlo
    /// dalla linea, l'ATR alla propria sinistra per rimettercelo. E' quello che
    /// permette al disegno di dire da che parte la corsia si apre - finche' un
    /// oggetto ne serviva due, non si poteva sapere.
    #[test]
    fn the_two_kinds_are_the_same_manoeuvre_mirrored() {
        // Freccia in alto: per il divert e' lo spostamento, per l'ATR la marcia.
        let arrow = Heading::Up;

        assert_eq!(
            divert().shift(arrow, Heading::Left),
            Some(Heading::Up),
            "chi va a sinistra viene alzato, cioe' spostato alla sua destra"
        );
        assert_eq!(
            atr().shift(arrow, Heading::Up),
            Some(Heading::Left),
            "chi sale viene spostato a sinistra, che e' la sua sinistra"
        );

        assert_eq!(
            divert().shift(arrow, Heading::Up),
            None,
            "quella marcia e' dell'ATR, non del divert"
        );
        assert_eq!(atr().shift(arrow, Heading::Left), None, "e viceversa");

        // Le altre due marce non le tocca nessuno dei due: gli assi restano
        // perpendicolari.
        for other in [Heading::Right, Heading::Down] {
            assert_eq!(divert().shift(arrow, other), None);
            assert_eq!(atr().shift(arrow, other), None);
        }
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
            !divert.catches(ON, far_above, carrier, Heading::Up, Heading::Left),
            "questa marcia la prende, ma il carrier e' quattro celle fuori dal corridoio"
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
        let atr = atr();
        // L'ATR aggancia chi marcia nel suo stesso verso.
        let (arrow, flow) = (Heading::Up, Heading::Up);

        assert!(atr.is_blocking(OFF), "non comandato, l'ATR sbarra");
        assert!(
            !atr.catches(OFF, Vec3::ZERO, Vec3::ZERO, arrow, flow),
            "e quindi non devia piu'"
        );

        assert!(
            !atr.is_blocking(ON),
            "comandato torna a essere un passaggio"
        );
        assert!(atr.catches(ON, Vec3::ZERO, Vec3::ZERO, arrow, flow));

        // E fuori servizio sbarra allo stesso modo: senza la spinta per
        // rientrare, dalla corsia secondaria non si va da nessuna parte.
        let out_of_service = Switch {
            enabled: false,
            active: false,
        };
        assert!(atr.is_blocking(out_of_service));
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
        let position = Vec3::ZERO;
        let arrow = Heading::Up;

        // A meta' strada vuol dire mezza cella lungo lo spostamento, che per i
        // due tipi e' perpendicolare: l'ATR porta a sinistra, il divert in alto.
        for (deviator, flow, halfway) in [
            (atr(), Heading::Up, Vec3::new(-LANE_HEIGHT / 2.0, 0.0, 0.0)),
            (
                divert(),
                Heading::Left,
                Vec3::new(0.0, LANE_HEIGHT / 2.0, 0.0),
            ),
        ] {
            assert!(
                deviator.catches(OFF, position, halfway, arrow, flow),
                "a meta' strada la manovra prosegue"
            );
            assert!(
                !deviator.catches(OFF, position, Vec3::ZERO, arrow, flow),
                "chi deve ancora cominciare non parte nemmeno"
            );
        }
    }
}
