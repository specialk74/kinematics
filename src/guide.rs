use bevy::prelude::*;

use crate::carrier::{Blocker, Heading};
use crate::layout::Placed;
use crate::piece::{Facing, GUIDE_LENGTH, GUIDE_OFFSET, GUIDE_THICKNESS, PieceShapes, Tool};

/// Un tratto di guida: il bordo della corsia. E' un muro vero - i carrier non lo
/// attraversano - e non ha niente da comandare: dove passa il flusso lo decide
/// chi disegna l'impianto, non un interruttore.
#[derive(Component)]
pub struct Guide(pub GuideShape);

/// Di quante linee e' fatto un tratto, e da che parte stanno.
///
/// Il corridoio e' la forma comoda: una corsia sono due bordi, e chiederli
/// all'utente uno per volta vorrebbe dire fargli piazzare il doppio dei pezzi
/// per lo stesso disegno. La singola serve dove i due bordi non sono simmetrici
/// - un innesto, la parete esterna di una curva, il fianco che si affaccia su
/// una cella gia' occupata da un altro pezzo - e li' il corridoio metterebbe un
/// muro dove il passaggio serve.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GuideShape {
    Corridor,
    Single,
}

/// I due lati di una cella su cui puo' correre una linea, come indici: `0` e' il
/// lato verso cui il pezzo e' girato, `1` quello dietro. E' l'ordine in cui
/// `edges_for` risponde, e la forma dice quali dei due la riguardano.
const SIDES: [f32; 2] = [GUIDE_OFFSET, -GUIDE_OFFSET];

impl GuideShape {
    /// Vero se questa forma ha una linea da quel lato. La singola disegna quello
    /// verso cui e' girata, cosi' portarla dall'altra parte e' un giro di tasto
    /// destro invece di un secondo tipo di pezzo.
    fn draws(self, side: usize) -> bool {
        match self {
            GuideShape::Corridor => true,
            GuideShape::Single => side == 0,
        }
    }

    /// Posto della forma fra le figure gia' pronte in `PieceShapes`.
    pub fn slot(self) -> usize {
        match self {
            GuideShape::Corridor => 0,
            GuideShape::Single => 1,
        }
    }

    /// Che forma di guida e' questo tipo di pezzo, se lo e'. Sta qui e non in
    /// `piece` perche' e' il vocabolario delle guide: chi aggiunge una forma
    /// tocca questo file, non quello dei tipi.
    pub fn of(tool: Tool) -> Option<Self> {
        match tool {
            Tool::Guide => Some(GuideShape::Corridor),
            Tool::GuideLine => Some(GuideShape::Single),
            _ => None,
        }
    }
}

/// Quali dei due bordi questo tratto tiene chiusi, `[avanti, dietro]` rispetto
/// al verso in cui e' girato. Lo scrive la logica, e lo leggono in due: il
/// disegno per la figura, il movimento per i muri.
///
/// Sta su un componente e non dentro il sistema che disegna perche' senza
/// finestra quel sistema non c'e', e i carrier devono trovare gli stessi muri in
/// tutti e due i casi: un impianto che si comporta diversamente a seconda che
/// qualcuno lo stia guardando non e' un impianto.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub struct GuideEdges(pub [bool; 2]);

/// Un corridoio chiuso da tutt'e due i lati: e' com'e' un tratto finche' non si
/// guarda che cosa ha attorno.
pub const CLOSED: [bool; 2] = [true, true];

/// I muri di tutte le guide in scena, gia' pronti per il movimento.
///
/// Sono in una risorsa e non ricavati a ogni frame perche' un impianto ne ha
/// molti - due per tratto, e un arco molti di piu' - mentre cambiano solo quando
/// cambia il layout, cioe' quando si sta in editor e i carrier sono fermi.
#[derive(Resource, Default)]
pub struct GuideWalls(pub Vec<Blocker>);

/// I tratti di cui e' fatto un pezzo di guida, in coordinate locali: `[da, a]`,
/// con la linea che corre lungo la x e il verso del pezzo che punta in su.
///
/// E' l'unico posto in cui sta scritta la forma di una guida. La stessa lista
/// diventa due cose: i triangoli della figura (`piece::guide_lines`) e i muri
/// che fermano i carrier (`walls`). Tenerla una sola e' cio' che impedisce a una
/// guida di disegnare un bordo dove non ferma nessuno.
///
/// Restituisce segmenti, non un rettangolo per bordo, perche' e' la forma che
/// regge anche quello che ancora non c'e': una diagonale, un arco - che e' una
/// spezzata di segmenti come lo disegna `piece::thick_arc`.
///
/// Le due condizioni sono diverse e vanno tenute distinte: `shape` dice quali
/// linee quel pezzo ha, `edges` quali di quelle un deviatore accanto tiene
/// aperte. Una guida singola girata verso un divert non disegna niente e non
/// ferma nessuno, ed e' giusto cosi'.
pub fn strokes(shape: GuideShape, edges: [bool; 2]) -> Vec<[Vec2; 2]> {
    let half = GUIDE_LENGTH / 2.0;

    SIDES
        .into_iter()
        .enumerate()
        .filter(|(side, _)| shape.draws(*side) && edges[*side])
        .map(|(_, edge)| [Vec2::new(-half, edge), Vec2::new(half, edge)])
        .collect()
}

/// I muri di un tratto di guida piazzato in `position` e girato in `facing`.
///
/// Il verso arriva come argomento e non dal `Transform`, come per la sbarra del
/// gate, e non e' un vezzo: senza finestra i pezzi non vengono mai ruotati -
/// `piece::orient_pieces` sta fra i sistemi del disegno - quindi leggere la
/// rotazione dal transform darebbe muri tutti orizzontali in headless.
pub fn walls(position: Vec3, facing: Heading, shape: GuideShape, edges: [bool; 2]) -> Vec<Blocker> {
    let turn = facing.rotation();
    let world = |local: Vec2| (turn * local.extend(0.0)).truncate() + position.truncate();

    strokes(shape, edges)
        .into_iter()
        .map(|[from, to]| Blocker::Segment {
            from: world(from),
            to: world(to),
            half_thickness: GUIDE_THICKNESS / 2.0,
        })
        .collect()
}

/// Il comportamento delle guide: quali bordi tengono chiusi e quali muri ne
/// nascono. Si monta sempre, anche senza finestra, perche' quei muri sono
/// cinematica.
pub struct GuidePlugin;

impl Plugin for GuidePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GuideWalls>()
            .add_systems(Update, refresh_guides);
    }
}

pub struct GuideVisualsPlugin;

impl Plugin for GuideVisualsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_guide_assets)
            .add_systems(Update, (attach_guide_visuals, redraw_guides));
    }
}

#[derive(Resource)]
pub struct GuideAssets {
    material: Handle<ColorMaterial>,
}

fn setup_guide_assets(mut commands: Commands, mut materials: ResMut<Assets<ColorMaterial>>) {
    commands.insert_resource(GuideAssets {
        // Un grigio spento, senza gradini di stato: la guida non ha stati, e
        // deve far vedere il tracciato senza rubare l'occhio agli oggetti, che
        // invece con il colore dicono qualcosa.
        material: materials.add(Color::srgb(0.42, 0.44, 0.50)),
    });
}

pub fn spawn_guide(commands: &mut Commands, position: Vec3, shape: GuideShape) -> Entity {
    commands
        .spawn((
            Transform::from_translation(position),
            Guide(shape),
            // Chiuso finche' non si guarda che cosa ha attorno: nasce gia' con i
            // suoi bordi perche' anche i muri partono da qui, e un tratto senza
            // bordi sarebbe un buco nella corsia per il frame in cui manca.
            GuideEdges(CLOSED),
        ))
        .id()
}

fn attach_guide_visuals(
    mut commands: Commands,
    shapes: Res<PieceShapes>,
    assets: Res<GuideAssets>,
    guides: Query<(Entity, &Guide, &GuideEdges), Without<Mesh2d>>,
) {
    for (entity, guide, edges) in guides.iter() {
        commands.entity(entity).insert((
            Mesh2d(shapes.guide_lines(guide.0, edges.0)),
            MeshMaterial2d(assets.material.clone()),
        ));
    }
}

/// La figura segue i bordi invece di essere aggiornata da chi li cambia: e' la
/// stessa regola del colore degli altri oggetti, che segue l'interruttore.
fn redraw_guides(
    shapes: Res<PieceShapes>,
    guides: Query<(&Guide, &GuideEdges, &mut Mesh2d), Changed<GuideEdges>>,
) {
    for (guide, edges, mut mesh) in guides {
        let wanted = shapes.guide_lines(guide.0, edges.0);

        // Si riscrive solo se cambia davvero: assegnare lo stesso handle
        // sveglierebbe il rendering per niente.
        if mesh.0 != wanted {
            mesh.0 = wanted;
        }
    }
}

/// Dove un deviatore fa uscire il carrier, il fianco della corsia deve aprirsi -
/// e il tratto di guida che passa di li' deve smettere di disegnarlo.
///
/// E' il guaio che si vedeva a schermo: il carrier attraversava una riga, perche'
/// la guida e' solo disegno e non ferma nessuno, ma quella riga diceva il
/// contrario di quello che l'impianto faceva.
///
/// Il bordo sta **fra due celle**, quindi lo apre sia un deviatore di qua sia uno
/// di la': un ATR nel ramo apre il fianco della corsia principale che gli sta
/// sopra, dove il tratto di guida ce l'ha messo l'utente. Per questo non si
/// cancella niente e non si vieta niente: la guida resta dov'e' e disegna un
/// bordo solo, che e' esattamente cio' che quel pezzo di corridoio e'.
/// Quali dei due bordi disegna il tratto di guida in `cell`, viste le celle
/// attorno: `[avanti, dietro]` rispetto al verso in cui e' girato.
pub fn edges_for(cell: IVec2, facing: Facing, around: &[(IVec2, Tool, Facing)]) -> [bool; 2] {
    let open = |toward: Heading| {
        let beyond = cell + toward.as_vec().as_ivec2();

        around
            .iter()
            .any(|(at, tool, facing)| match tool.opens_toward(*facing) {
                // Il deviatore e' in questa stessa cella e apre di qua...
                Some(opening) if *at == cell => opening == toward,
                // ...oppure e' dall'altra parte del bordo e apre verso di noi.
                Some(opening) if *at == beyond => opening == toward.opposite(),
                _ => false,
            })
    };

    [!open(facing.0), !open(facing.0.opposite())]
}

/// Decide quali bordi restano chiusi e ne rifa' i muri, in una passata sola:
/// sono la stessa cosa detta a due lettori diversi - la figura e il movimento -
/// e separarle vorrebbe dire scorrere due volte le stesse guide.
///
/// L'elenco dei muri non e' incrementale di proposito: i tratti che cambiano
/// sono quasi sempre tutti quelli attorno al pezzo appena piazzato, e tenere il
/// conto di quali sarebbe piu' codice che rifare la lista mentre l'impianto sta
/// fermo in editor.
fn refresh_guides(
    mut cached: ResMut<GuideWalls>,
    pieces: Query<(&Placed, &Facing)>,
    // Basta che qualcosa sia nato, si sia mosso o sia stato girato: fuori da
    // quei momenti i bordi restano quelli, e ricalcolarli a ogni frame vorrebbe
    // dire rifare lo stesso conto sessanta volte al secondo. `Placed` copre
    // anche la guida trascinata altrove, che cambia i muri senza cambiare bordi.
    touched: Query<(), Or<(Changed<Placed>, Changed<Facing>)>>,
    mut gone: RemovedComponents<Placed>,
    mut guides: Query<(&Transform, &Placed, &Facing, &Guide, &mut GuideEdges)>,
) {
    if touched.is_empty() && gone.read().next().is_none() {
        return;
    }

    let around: Vec<(IVec2, Tool, Facing)> = pieces
        .iter()
        .map(|(placed, facing)| (placed.cell, placed.tool, *facing))
        .collect();

    cached.0 = guides
        .iter_mut()
        .flat_map(|(at, placed, facing, guide, mut edges)| {
            let wanted = GuideEdges(edges_for(placed.cell, *facing, &around));

            // Si scrive solo se cambia davvero, altrimenti la figura si
            // riscriverebbe ogni volta che qualcuno tocca un pezzo qualsiasi
            // dell'impianto, fosse anche dall'altra parte della scena.
            if *edges != wanted {
                *edges = wanted;
            }

            walls(at.translation, facing.0, guide.0, wanted.0)
        })
        .collect();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::carrier::{BELT_SPEED, CARRIER_RADIUS, blocks};
    use crate::grid::GRID_STEP;
    use crate::simulation::MAX_SPEED;

    const BOTH: [bool; 2] = [true, true];

    /// Il caso visto a schermo: un ATR nel ramo, e sopra di lui la corsia
    /// principale disegnata con i suoi tratti di guida. Il fianco fra i due si
    /// deve aprire, ma il tratto da correggere e' quello **della cella sopra**,
    /// non quello della cella dell'ATR: e' li' che l'utente l'ha messo. La
    /// parete esterna della corsia principale resta invece dov'e'.
    #[test]
    fn an_atr_below_opens_the_lane_drawn_above_it() {
        let lane = IVec2::ZERO;
        let along = Facing(Heading::Up);
        // Girato a destra, l'ATR riporta il carrier alla propria sinistra,
        // cioe' in su: verso la corsia.
        let atr = [(IVec2::new(0, -1), Tool::Atr, Facing(Heading::Right))];

        assert_eq!(
            edges_for(lane, along, &atr),
            [true, false],
            "cade il bordo verso l'ATR, resta quello esterno"
        );
        assert_eq!(
            edges_for(lane, along, &[]),
            BOTH,
            "senza deviatori attorno il corridoio e' chiuso da tutt'e due i lati"
        );
    }

    /// Vale anche per il deviatore che sta nella cella della guida: e' il caso
    /// del divert, che vive in mezzo alla corsia principale.
    #[test]
    fn a_divert_in_the_same_cell_opens_the_side_it_pushes_toward() {
        let cell = IVec2::ZERO;
        let along = Facing(Heading::Up);
        let divert = [(cell, Tool::Divert, Facing(Heading::Up))];

        assert_eq!(edges_for(cell, along, &divert), [false, true]);
    }

    /// Chi non devia nessuno non apre niente: un gate nella stessa cella o in
    /// quella accanto lascia il corridoio chiuso com'era.
    #[test]
    fn a_gate_does_not_open_anything() {
        let cell = IVec2::ZERO;
        let along = Facing(Heading::Up);
        let gates = [
            (cell, Tool::Gate, Facing(Heading::Up)),
            (IVec2::new(0, 1), Tool::Gate, Facing(Heading::Down)),
        ];

        assert_eq!(edges_for(cell, along, &gates), BOTH);
    }

    /// Un deviatore che apre dall'altra parte non riguarda questo tratto: il
    /// bordo che si apre e' uno solo, e sta dove passa il carrier.
    #[test]
    fn a_divert_pushing_elsewhere_leaves_this_corridor_alone() {
        let cell = IVec2::ZERO;
        let along = Facing(Heading::Up);
        // Sotto, ma spinge verso il basso: dal corridoio di sopra se ne va.
        let divert = [(IVec2::new(0, -1), Tool::Divert, Facing(Heading::Down))];

        assert_eq!(edges_for(cell, along, &divert), BOTH);
    }

    /// Vero se un carrier fermo in quel punto sta toccando uno dei muri.
    fn stopped_at(walls: &[Blocker], point: Vec3) -> bool {
        walls
            .iter()
            .any(|wall| blocks(*wall, point, CARRIER_RADIUS))
    }

    /// Il senso di tutto il pezzo: chi corre dentro la corsia non tocca niente,
    /// chi prova a uscirne di traverso trova il muro. Prima le guide erano
    /// disegno, e un carrier ci passava attraverso lasciando a schermo una riga
    /// che diceva il contrario di quello che l'impianto faceva.
    #[test]
    fn a_closed_corridor_lets_the_flow_through_and_stops_who_leaves_it() {
        // Un corridoio orizzontale: girato in su, i suoi due bordi stanno sopra
        // e sotto la linea di marcia.
        let corridor = walls(Vec3::ZERO, Heading::Up, GuideShape::Corridor, CLOSED);

        assert!(
            !stopped_at(&corridor, Vec3::ZERO),
            "in mezzo alla corsia si passa"
        );
        assert!(
            !stopped_at(&corridor, Vec3::new(GRID_STEP / 2.0, 0.0, 0.0)),
            "e si passa per tutta la lunghezza del tratto"
        );
        assert!(
            stopped_at(&corridor, Vec3::new(0.0, GRID_STEP / 2.0, 0.0)),
            "chi arriva sul bordo lo trova chiuso"
        );
    }

    /// Il buco dove serve: il fianco che un deviatore apre non ha muro, o la
    /// manovra che il disegno mostra sarebbe impossibile da fare.
    #[test]
    fn the_side_a_divert_opens_has_no_wall() {
        let opened = edges_for(
            IVec2::ZERO,
            Facing(Heading::Up),
            &[(IVec2::ZERO, Tool::Divert, Facing(Heading::Up))],
        );
        let corridor = walls(Vec3::ZERO, Heading::Up, GuideShape::Corridor, opened);
        let leaving = Vec3::new(0.0, GRID_STEP / 2.0, 0.0);

        assert!(
            !stopped_at(&corridor, leaving),
            "di qui il divert fa uscire il carrier: muro non ce ne deve essere"
        );
        assert!(
            stopped_at(&corridor, Vec3::new(0.0, -GRID_STEP / 2.0, 0.0)),
            "dall'altra parte il fianco resta chiuso"
        );
    }

    /// I muri si girano con il pezzo: un tratto girato di un quarto chiude i
    /// fianchi di una corsia verticale, non di una orizzontale. E' la prova che
    /// il verso arriva da `Facing` - l'unica cosa che headless esiste - e non
    /// dalla rotazione del transform, che senza finestra nessuno scrive.
    #[test]
    fn a_turned_piece_walls_a_vertical_lane() {
        let corridor = walls(Vec3::ZERO, Heading::Right, GuideShape::Corridor, CLOSED);

        assert!(
            !stopped_at(&corridor, Vec3::new(0.0, GRID_STEP / 2.0, 0.0)),
            "la corsia adesso corre in verticale: sopra si passa"
        );
        assert!(
            stopped_at(&corridor, Vec3::new(GRID_STEP / 2.0, 0.0, 0.0)),
            "e il muro sta di lato"
        );
    }

    /// La forma nuova: una linea sola, dalla parte verso cui il pezzo e' girato.
    /// Girarlo la porta dall'altra parte, ed e' per questo che di guida singola
    /// ce n'e' un tipo solo invece di due.
    #[test]
    fn a_single_guide_draws_one_line_on_the_side_it_faces() {
        let above = Vec3::new(0.0, GRID_STEP / 2.0, 0.0);
        let below = Vec3::new(0.0, -GRID_STEP / 2.0, 0.0);

        assert_eq!(
            strokes(GuideShape::Single, CLOSED).len(),
            1,
            "una linea sola, non due"
        );

        let facing_up = walls(Vec3::ZERO, Heading::Up, GuideShape::Single, CLOSED);
        assert!(stopped_at(&facing_up, above), "il muro sta davanti");
        assert!(!stopped_at(&facing_up, below), "e dietro si passa");

        let facing_down = walls(Vec3::ZERO, Heading::Down, GuideShape::Single, CLOSED);
        assert!(!stopped_at(&facing_down, above));
        assert!(
            stopped_at(&facing_down, below),
            "girata, il muro passa dall'altra parte"
        );
    }

    /// Due singole affacciate valgono un corridoio: e' la prova che la forma
    /// nuova non e' un'altra geometria ma lo stesso bordo, e che chi disegna puo'
    /// scegliere il pezzo comodo senza cambiare quello che l'impianto fa.
    #[test]
    fn two_single_guides_facing_each_other_make_the_same_lane_as_a_corridor() {
        let corridor = walls(Vec3::ZERO, Heading::Up, GuideShape::Corridor, CLOSED);
        // Una nella cella di sopra girata in giu', una in quella di sotto girata
        // in su: i loro muri cadono sui due confini della cella di mezzo.
        let singles: Vec<Blocker> = [
            (Vec3::new(0.0, GRID_STEP, 0.0), Heading::Down),
            (Vec3::new(0.0, -GRID_STEP, 0.0), Heading::Up),
        ]
        .into_iter()
        .flat_map(|(at, facing)| walls(at, facing, GuideShape::Single, CLOSED))
        .collect();

        for point in [
            Vec3::ZERO,
            Vec3::new(0.0, GRID_STEP / 2.0, 0.0),
            Vec3::new(0.0, -GRID_STEP / 2.0, 0.0),
            Vec3::new(GRID_STEP / 4.0, GRID_STEP / 2.0, 0.0),
        ] {
            assert_eq!(
                stopped_at(&corridor, point),
                stopped_at(&singles, point),
                "in {point:?} le due composizioni devono dire la stessa cosa"
            );
        }
    }

    /// Una singola girata verso un deviatore non disegna niente e non ferma
    /// nessuno: la sua unica linea sta proprio sul fianco che si deve aprire.
    #[test]
    fn a_single_guide_facing_a_divert_walls_nothing() {
        let opened = edges_for(
            IVec2::ZERO,
            Facing(Heading::Up),
            &[(IVec2::ZERO, Tool::Divert, Facing(Heading::Up))],
        );

        assert!(strokes(GuideShape::Single, opened).is_empty());
        assert!(
            walls(Vec3::ZERO, Heading::Up, GuideShape::Single, opened).is_empty(),
            "niente linea, niente muro: sono la stessa lista"
        );
    }

    /// Il muro e' sottile, il passo di un frame no: alla velocita' massima un
    /// carrier deve comunque incontrarlo invece di scavalcarlo in un colpo solo.
    ///
    /// E' il vincolo che lega `MAX_SPEED` allo spessore delle guide, ed e' il
    /// primo a saltare se un giorno si alza il tetto: qui si vede subito, in un
    /// commento non se ne accorgerebbe nessuno.
    #[test]
    fn the_fastest_step_cannot_jump_over_a_wall() {
        let step = BELT_SPEED * MAX_SPEED / 60.0;
        // Il muro ferma finche' il carrier gli sta piu' vicino di questo: per
        // saltarlo servirebbe un passo che copre la portata da tutt'e due i lati.
        let reach = CARRIER_RADIUS + GUIDE_THICKNESS / 2.0;

        assert!(
            step < reach * 2.0,
            "a velocita' {MAX_SPEED} il passo e' {step} px e la portata del muro {reach}: si passerebbe attraverso"
        );
    }
}
