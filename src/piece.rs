use std::f32::consts::PI;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::PrimitiveTopology;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::carrier::{CARRIER_RADIUS, Heading};
use crate::grid::GRID_STEP;
use crate::guide::GuideShape;

pub const PIECE_SIZE: f32 = 30.0;
/// Un paio di pixel piu' larga di un carrier: quando il carrier le si ferma
/// sopra ne resta scoperta una corona sottile, che dice che l'antenna c'e', di
/// che colore e', e la rende cliccabile.
pub const ANTENNA_RADIUS: f32 = CARRIER_RADIUS + 2.0;

/// Il gate non e' un quadrato come gli altri ma una sbarra su un lato della
/// cella. Cosi' il carrier che ferma si arresta quasi al centro della cella
/// invece che sul confine, e sotto di lui ci puo' stare un'antenna che lo legge.
pub const BAR_LENGTH: f32 = 48.0;
/// La linea della guida: lunga quanto la cella, cosi' due tratti accostati si
/// saldano senza lasciare buchi, e sottile perche' e' uno sfondo, non un oggetto.
pub const GUIDE_LENGTH: f32 = GRID_STEP;
pub const GUIDE_THICKNESS: f32 = 4.0;
/// I due bordi stanno sui confini della cella, non al suo interno: una corsia
/// e' cosi' alta esattamente una cella, che e' anche di quanto il divert sposta
/// un carrier. Due corsie affiancate condividono il bordo che le separa.
pub const GUIDE_OFFSET: f32 = GRID_STEP / 2.0;
pub const BAR_THICKNESS: f32 = 8.0;
/// Quanto dista dal centro della cella: appoggiata al confine e tutta dentro la
/// propria cella, cosi' due gate accostati non si sovrappongono.
pub const BAR_OFFSET: f32 = (GRID_STEP - BAR_THICKNESS) / 2.0;

/// Anche l'antenna sta spostata verso il lato, e non a caso: e' esattamente
/// dove si ferma il carrier che la sbarra blocca. Il conto e' la faccia interna
/// della sbarra meno il raggio del carrier, cioe' il punto in cui il carrier si
/// arresta; cosi' se un giorno cambiano lo spessore della sbarra o la misura
/// del carrier, l'antenna resta sotto di lui senza doverla ritoccare.
pub const ANTENNA_OFFSET: f32 = BAR_OFFSET - BAR_THICKNESS / 2.0 - CARRIER_RADIUS;

/// Lunghezza complessiva della freccia, stelo compreso.
const ARROW_LENGTH: f32 = 20.0;
const ARROW_HEAD: f32 = 8.0;
const ARROW_HEAD_WIDTH: f32 = 11.0;
const ARROW_STEM_WIDTH: f32 = 4.0;
/// Sopra al quadrato che la contiene.
const ARROW_Z: f32 = 0.1;
const ARROW_COLOR: Color = Color::WHITE;
/// Il quadrato nero del despawn.
const STOP_SIZE: f32 = 14.0;
const STOP_COLOR: Color = Color::BLACK;

/// Gli oggetti che si possono piazzare nella scena. E' anche il vocabolario del
/// file di layout, quindi rinominare una variante invalida i file gia' salvati.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Tool {
    CarrierSource,
    Gate,
    Divert,
    Atr,
    Despawner,
    /// Svolta a destra rispetto alla marcia del carrier. Il nome serializzato
    /// resta quello di prima per non invalidare i layout gia' salvati.
    #[serde(alias = "Riser")]
    Turner,
    Reverser,
    /// Lettore sotto la linea. Non tocca il flusso: guarda e basta.
    Antenna,
    /// Fotocellula su una parete della cella: vede passare le provette.
    TubeSensor,
    /// La stessa cosa, ma conta qualunque carrier.
    CarrierSensor,
    /// Un tratto di corridoio: i due bordi di una corsia.
    Guide,
    /// Un bordo solo, dalla parte verso cui e' girato. Serve dove i due lati non
    /// sono simmetrici e il corridoio metterebbe un muro dove invece si passa.
    GuideLine,
}

impl Tool {
    pub fn label(self) -> &'static str {
        match self {
            Tool::CarrierSource => "Sorgente",
            Tool::Gate => "Gate",
            Tool::Divert => "Divert",
            Tool::Atr => "ATR",
            Tool::Despawner => "Despawn",
            Tool::Turner => "Svolta",
            Tool::Reverser => "Inversione",
            Tool::Antenna => "Antenna",
            Tool::TubeSensor => "Sens. tubo",
            Tool::CarrierSensor => "Sens. carrier",
            Tool::Guide => "Guida",
            Tool::GuideLine => "Guida singola",
        }
    }

    /// Vero se per questo oggetto "attivo" vuol dire forzare il proprio segnale
    /// invece di comandare un'azione: sensori e antenna. Serve a saperlo prima
    /// ancora che l'oggetto esista, quando gli si da' lo stato di partenza.
    pub fn forces_signal(self) -> bool {
        matches!(self, Tool::Antenna | Tool::TubeSensor | Tool::CarrierSensor)
    }

    pub fn layer(self) -> Layer {
        match self {
            Tool::Guide | Tool::GuideLine => Layer::Rail,
            Tool::Antenna => Layer::Under,
            Tool::TubeSensor | Tool::CarrierSensor => Layer::Side,
            _ => Layer::Track,
        }
    }

    /// Vero se l'oggetto e' solo disegno: niente stato da comandare, niente
    /// nome da dire a mqtt, e quindi niente da mostrare nell'elenco dei nomi
    /// ne' da scrivere nelle registrazioni. Un impianto ne contiene molti.
    pub fn is_passive(self) -> bool {
        matches!(self, Tool::Guide | Tool::GuideLine)
    }

    /// Verso quale cella confinante questo oggetto apre il fianco della corsia,
    /// cioe' da che parte fa uscire il carrier. `None` per chi non devia
    /// nessuno: un gate ferma dentro la corsia, un'antenna guarda e basta.
    ///
    /// Poterlo dire e' recente. Finche' un deviatore serviva due flussi a
    /// seconda di come gli arrivava il carrier, l'apertura poteva essere su due
    /// lati perpendicolari e non c'era modo di sapere quale: adesso lo decide il
    /// tipo, come in `Divert::shift`, e questa e' la stessa regola vista da
    /// fuori. Se una cambia, l'altra la segue.
    pub fn opens_toward(self, facing: Facing) -> Option<Heading> {
        match self {
            // Il divert sposta nel verso in cui e' girato.
            Tool::Divert => Some(facing.0),
            // L'ATR e l'inversione portano il carrier alla propria sinistra.
            Tool::Atr | Tool::Reverser => Some(facing.0.turn_left()),
            // La svolta non sposta di corsia: ci esce, e lo fa dalla parte in
            // cui e' girata. Contarla e' diventato necessario da quando le
            // guide sono muri veri - prima il carrier attraversava il bordo
            // disegnato e nessuno se ne accorgeva, adesso ci sbatterebbe e la
            // svolta diventerebbe un vicolo cieco.
            Tool::Turner => Some(facing.0),
            _ => None,
        }
    }
}

/// Su che piano vive un oggetto. Serve perche' l'antenna sta sotto la linea:
/// non contende la cella agli altri, e una cella puo' benissimo avere un gate
/// con un'antenna sotto. Due oggetti dello stesso piano invece si escludono,
/// come e' sempre stato.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layer {
    /// Sulla linea: sorgenti, gate, deviatori, uscite.
    Track,
    /// Su una parete della cella: i sensori. Uno solo per cella, ma convive con
    /// l'oggetto di linea, perche' sta su un lato che quello lascia libero.
    Side,
    /// Sotto la linea: ci passano sopra sia i carrier sia gli oggetti.
    Under,
    /// Il fondo: i tratti di guida, che stanno sotto a tutto il resto.
    Rail,
}

impl Layer {
    /// Quota a cui nasce l'oggetto. I carrier viaggiano a zero, quindi il piano
    /// di sotto deve stare in negativo per finire davvero sotto di loro.
    pub fn z(self) -> f32 {
        match self {
            Layer::Track => 1.0,
            Layer::Side => 0.9,
            Layer::Under => -1.0,
            Layer::Rail => -2.0,
        }
    }
}

/// Che freccia ci va dentro. Non tutti gli oggetti ne hanno una: il gate blocca
/// e basta, in qualunque verso lo si giri.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Arrow {
    /// Nessuna: l'oggetto non ha un verso.
    None,
    /// Dritta: il carrier prosegue di li'.
    Straight,
    /// Un quadrato nero: il carrier finisce li' e basta, da qualunque parte
    /// arrivi. Non e' un verso, quindi non e' una freccia.
    Stop,
}

/// Come e' girato l'oggetto, cioe' dove manda il carrier. Il tasto destro la fa
/// ruotare di un quarto di giro per volta.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Facing(pub Heading);

impl Default for Facing {
    fn default() -> Self {
        // Verso sinistra: e' il verso in cui i carrier viaggiano di partenza.
        Facing(Heading::Left)
    }
}

/// Mesh condivise da tutti gli oggetti: sono tutti quadrati uguali, cambia solo
/// il colore. La freccia dentro dice dove finisce il carrier.
#[derive(Resource)]
pub struct PieceShapes {
    square: Handle<Mesh>,
    circle: Handle<Mesh>,
    bar: Handle<Mesh>,
    /// Le figure delle guide: per ogni forma, le quattro combinazioni di bordi
    /// aperti. Una forma che di un lato non sa niente ne ripete due uguali, e
    /// costano due triangoli: meno di un ramo in piu' da tenere d'accordo.
    guide_lines: [[Handle<Mesh>; 4]; 2],
    divert: Handle<Mesh>,
    atr: Handle<Mesh>,
    straight_arrow: Handle<Mesh>,
    reverser: Handle<Mesh>,
    stop: Handle<Mesh>,
    arrow_material: Handle<ColorMaterial>,
    stop_material: Handle<ColorMaterial>,
}

impl PieceShapes {
    /// La figura di un tratto di guida di quella forma, con aperti i bordi
    /// chiesti: `[avanti, dietro]` rispetto al verso in cui il pezzo e' girato.
    pub fn guide_lines(&self, shape: GuideShape, edges: [bool; 2]) -> Handle<Mesh> {
        let index = usize::from(!edges[0]) * 2 + usize::from(!edges[1]);

        self.guide_lines[shape.slot()][index].clone()
    }
}

/// Mesh piatta da un elenco di triangoli. Le frecce non sono figure primitive:
/// vanno costruite a mano vertice per vertice.
fn triangles(points: Vec<[f32; 3]>) -> Mesh {
    let normals = vec![[0.0, 0.0, 1.0]; points.len()];
    let uvs = vec![[0.0, 0.0]; points.len()];

    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, points)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
}

/// Il corridoio: i due bordi della corsia, uno sopra e uno sotto. E' un pezzo
/// solo perche' una corsia sono due linee, e chiederle all'utente una per volta
/// vorrebbe dire fargli piazzare il doppio dei pezzi per lo stesso disegno.
///
/// Non sempre pero' se ne disegnano due: dove un deviatore fa uscire il carrier,
/// quel fianco deve aprirsi, e il tratto di guida che lo attraversa deve tacere
/// invece di richiuderlo. Il primo dei due bordi e' quello dalla parte in cui il
/// pezzo e' girato.
/// I tratti da cui e' fatta la figura li dice `guide::strokes`, e non li rifa'
/// qui: la stessa lista diventa i muri che fermano i carrier. Passando di li'
/// una guida non puo' disegnare un bordo dove non ferma nessuno, ne' fermare
/// qualcuno dove non ha disegnato niente - che e' il guaio che si vedeva a
/// schermo quando il disegno era l'unica cosa che le guide facevano.
fn guide_lines(shape: GuideShape, edges: [bool; 2]) -> Mesh {
    let mut points = Vec::new();

    for [from, to] in crate::guide::strokes(shape, edges) {
        thick_segment(&mut points, from, to, GUIDE_THICKNESS);
    }

    triangles(points)
}

/// Le quattro combinazioni di bordi aperti, per una forma di guida.
fn guide_meshes(meshes: &mut Assets<Mesh>, shape: GuideShape) -> [Handle<Mesh>; 4] {
    [[true, true], [true, false], [false, true], [false, false]]
        .map(|edges| meshes.add(guide_lines(shape, edges)))
}

/// Spessore della traiettoria disegnata dentro un oggetto, e misure della punta
/// che ne indica il verso. Senza la punta un oggetto girato in un verso e uno
/// girato nel verso opposto si disegnerebbero uguali - un segmento ruotato di
/// mezzo giro e' se stesso - e uno dei due non aggancia il flusso: si finiva
/// per guardare un oggetto che sembrava a posto e non faceva niente.
///
/// L'inversione non ne ha bisogno: li' il giro e' sempre lo stesso, e la figura
/// disegna il corridoio invece della strada.
const PATH_WIDTH: f32 = 5.0;
const PATH_HEAD: f32 = 15.0;
const PATH_HEAD_WIDTH: f32 = 16.0;
/// In quanti tratti si spezza un arco: abbastanza da non vedere gli spigoli.
const ARC_STEPS: usize = 14;

/// Un segmento spesso fra due punti: serve alle diagonali, che un rettangolo
/// dritto non sa fare.
fn thick_segment(points: &mut Vec<[f32; 3]>, from: Vec2, to: Vec2, width: f32) {
    let along = (to - from).normalize_or_zero();
    let across = Vec2::new(-along.y, along.x) * width / 2.0;

    for corner in [
        [from + across, from - across, to - across],
        [from + across, to - across, to + across],
    ] {
        points.extend(corner.map(|point| [point.x, point.y, 0.0]));
    }
}

/// Un arco spesso attorno a un centro, dall'angolo `from` all'angolo `to`,
/// spezzato in tratti dritti.
fn thick_arc(points: &mut Vec<[f32; 3]>, centre: Vec2, radius: f32, arc: (f32, f32), width: f32) {
    let (from, to) = arc;
    let on_arc = |angle: f32| centre + Vec2::from_angle(angle) * radius;
    let mut previous = on_arc(from);

    for step in 1..=ARC_STEPS {
        let next = on_arc(from + (to - from) * step as f32 / ARC_STEPS as f32);

        thick_segment(points, previous, next, width);
        previous = next;
    }
}

/// La punta di una traiettoria: un triangolo appoggiato al collo e rivolto dove
/// il carrier esce.
fn path_head(points: &mut Vec<[f32; 3]>, neck: Vec2, tip: Vec2) {
    let along = (tip - neck).normalize_or_zero();
    let across = Vec2::new(-along.y, along.x) * PATH_HEAD_WIDTH / 2.0;

    for point in [neck + across, neck - across, tip] {
        points.push([point.x, point.y, 0.0]);
    }
}

/// Il divert e l'ATR: una diagonale sola, da spigolo a spigolo, nel verso in cui
/// l'oggetto sposta il carrier.
///
/// Cambia la cella in cui viene disegnata, e non e' un dettaglio: sta sempre
/// nella **corsia secondaria**, quella fuori dal flusso principale. Il divert
/// vive nella corsia principale, quindi la sua diagonale finisce nella cella
/// accanto; l'ATR vive gia' nella secondaria, quindi la sua sta in casa propria.
/// Sono due oggetti che fanno il contrario l'uno dell'altro, e il disegno lo
/// dice invece di lasciarlo capire dal colore.
///
/// I bordi della corsia li disegnano i pezzi di guida, che l'utente mette dove
/// servono. Qui se ne disegna **uno solo**: quello che la guida non puo' mettere,
/// perche' la cella la occupa il deviatore. Sta sempre dalla parte opposta allo
/// spostamento - dall'altra il passaggio serve davvero, e' di li' che si esce -
/// e tiene chiuso il fianco lungo cui il flusso deve poter tirare dritto.
///
/// I due lo hanno perpendicolare, ed e' la conseguenza visibile del fatto che
/// sono la stessa manovra a specchio: il divert aggancia un flusso che gli passa
/// di traverso e lo spinge nel proprio verso, quindi la sua corsia e' quella
/// perpendicolare al verso; l'ATR aggancia il flusso che marcia nel proprio
/// verso, quindi la sua corsia e' quella. Fianchi di corsie perpendicolari sono
/// perpendicolari anche loro.
///
/// Poterlo dire e' recente: finche' un deviatore serviva due flussi a seconda di
/// come gli arrivava il carrier, quale fosse la sua corsia non si sapeva, e il
/// bordo cadeva di traverso in meta' dei casi. Adesso lo decide `Divert::shift`
/// guardando il tipo.
///
/// Divert e ATR condividono la traiettoria perche' condividono il movimento: a
/// distinguerli sono il colore, il fianco e il verso in cui li si gira.
fn divert_glyph(from_the_main_lane: bool) -> Mesh {
    let half = GRID_STEP / 2.0;
    // Il divert disegna una corsia piu' in la', l'ATR in casa propria.
    let lane = if from_the_main_lane { GRID_STEP } else { 0.0 };
    let mut points = Vec::new();

    // Il fianco che il divert si porta dietro, sul lato opposto allo
    // spostamento: la sua cella sta in mezzo alla corsia principale e il flusso
    // li' deve poter anche tirare dritto. L'ATR non ne ha bisogno - il bordo del
    // ramo lo disegnano i tratti di guida, che sanno tacere dove il passaggio si
    // apre - e disegnarglielo lo lasciava sospeso in mezzo al niente.
    if from_the_main_lane {
        thick_segment(
            &mut points,
            Vec2::new(-half, -half),
            Vec2::new(half, -half),
            GUIDE_THICKNESS,
        );
    }

    let from = Vec2::new(half, -half + lane);
    let to = Vec2::new(-half, half + lane);
    // Il carrier taglia la cella passando per la meta' dei due lati: una retta
    // da spigolo a spigolo gli finirebbe sotto. L'arco si incurva dalla parte
    // opposta al suo tragitto e gli lascia la strada libera.
    let bulge = Vec2::new(half, half + lane);
    let on_arc = |t: f32| {
        let u = 1.0 - t;

        from * (u * u) + bulge * (2.0 * u * t) + to * (t * t)
    };

    // Ci si ferma prima della fine: l'ultimo tratto e' la punta.
    let tip = on_arc(1.0);
    let neck = on_arc(1.0 - PATH_HEAD / from.distance(to));
    let mut previous = on_arc(0.0);
    for step in 1..=ARC_STEPS {
        let next = on_arc(step as f32 / ARC_STEPS as f32);
        if next.distance(tip) < neck.distance(tip) {
            break;
        }

        thick_segment(&mut points, previous, next, PATH_WIDTH);
        previous = next;
    }
    thick_segment(&mut points, previous, neck, PATH_WIDTH);

    // La punta, sull'estremita' verso cui il carrier viene spostato.
    path_head(&mut points, neck, tip);

    triangles(points)
}

/// Il raggio della curva d'inversione: mezza cella, cioe' la stessa misura che
/// la cinematica usa in `reverser::TURN_RADIUS`. Si riscrive invece di
/// importarla perche' il disegno non deve dipendere dal movimento; che le due
/// coincidano lo tiene fermo il test dell'inversione, che pretende l'uscita
/// esattamente una cella piu' in la'.
const TURN_RADIUS: f32 = GRID_STEP / 2.0;

/// L'inversione: il bordo della corsia che gira attorno al mezzo giro.
///
/// E' il tornante visto da fuori. Il centro e' quello vero della curva, mezza
/// cella alla **sinistra** di chi arriva, dove lo mette `Reverser::turn`; il
/// raggio e' una cella intera perche' il bordo corre mezza cella piu' in fuori
/// della traiettoria. Comincia e finisce quindi sui confini delle due corsie -
/// quella di andata e quella di ritorno - e le salda sopra la curva: senza, il
/// corridoio finirebbe nel vuoto proprio dove il carrier gira.
///
/// Bordo interno non ce n'e', e non e' una dimenticanza: con la curva stretta
/// mezza cella e la corsia larga una cella, il fianco interno si riduce al punto
/// attorno a cui si gira.
///
/// La traiettoria dentro non e' disegnata, e nemmeno una punta: il mezzo giro e'
/// sempre lo stesso - si gira a sinistra di chi arriva, comunque - quindi non
/// c'e' nessuna scelta da raccontare. Resta il corridoio, che e' l'unica cosa da
/// far vedere.
fn reverser_glyph() -> Mesh {
    let centre = Vec2::new(-TURN_RADIUS, 0.0);
    let radius = TURN_RADIUS + GRID_STEP / 2.0;
    let mut points = Vec::new();

    thick_arc(&mut points, centre, radius, (0.0, PI), GUIDE_THICKNESS);

    // I due montanti. L'arco finisce all'altezza del centro della cella, mentre
    // i tratti di guida delle corsie cominciano al confine: fra i due resterebbe
    // mezza cella di vuoto, e il raccordo che questa figura esiste per fare non
    // ci sarebbe. Scendono fino al bordo della propria cella, cioe' esattamente
    // dove comincia la guida della cella sotto.
    for end in [centre + Vec2::X * radius, centre - Vec2::X * radius] {
        thick_segment(
            &mut points,
            end,
            Vec2::new(end.x, -GRID_STEP / 2.0),
            GUIDE_THICKNESS,
        );
    }

    triangles(points)
}

/// Freccia con lo stelo, disegnata verso l'alto: due triangoli per il gambo e
/// uno per la punta.
fn straight_arrow() -> Mesh {
    let tip = ARROW_LENGTH / 2.0;
    let neck = tip - ARROW_HEAD;
    let tail = -ARROW_LENGTH / 2.0;
    let stem = ARROW_STEM_WIDTH / 2.0;
    let head = ARROW_HEAD_WIDTH / 2.0;

    triangles(vec![
        [-stem, tail, 0.0],
        [stem, tail, 0.0],
        [stem, neck, 0.0],
        [-stem, tail, 0.0],
        [stem, neck, 0.0],
        [-stem, neck, 0.0],
        [-head, neck, 0.0],
        [head, neck, 0.0],
        [0.0, tip, 0.0],
    ])
}

pub struct PiecePlugin;

impl Plugin for PiecePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_piece_shapes)
            .add_systems(Update, orient_pieces);
    }
}

fn setup_piece_shapes(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    commands.insert_resource(PieceShapes {
        square: meshes.add(Rectangle::new(PIECE_SIZE, PIECE_SIZE)),
        circle: meshes
            .add(Mesh::from(Circle::new(ANTENNA_RADIUS)).translated_by(Vec3::Y * ANTENNA_OFFSET)),
        guide_lines: [
            guide_meshes(&mut meshes, GuideShape::Corridor),
            guide_meshes(&mut meshes, GuideShape::Single),
        ],
        divert: meshes.add(divert_glyph(true)),
        atr: meshes.add(divert_glyph(false)),
        // La sbarra e' spostata nella mesh stessa e non nel Transform: la
        // posizione dell'oggetto resta il centro della cella, che e' quello che
        // sanno la griglia, il salvataggio e il trascinamento.
        bar: meshes.add(
            Mesh::from(Rectangle::new(BAR_LENGTH, BAR_THICKNESS))
                .translated_by(Vec3::Y * BAR_OFFSET),
        ),
        straight_arrow: meshes.add(straight_arrow()),
        reverser: meshes.add(reverser_glyph()),
        stop: meshes.add(Rectangle::new(STOP_SIZE, STOP_SIZE)),
        arrow_material: materials.add(ARROW_COLOR),
        stop_material: materials.add(STOP_COLOR),
    });
}

/// La figura che un oggetto occupa davvero dentro la sua cella. Serve a sapere
/// che cosa si sta puntando quando due oggetti condividono la cella: sulla
/// figura c'e' quello di linea, altrove l'antenna che gli sta sotto.
pub fn covers(tool: Tool, facing: Facing, centre: Vec2, point: Vec2) -> bool {
    let offset = point - centre;

    match tool {
        // La sbarra sta su un lato: il punto va riportato nel verso del gate
        // prima di misurarlo, altrimenti si misurerebbe sempre lo stesso lato.
        Tool::Gate | Tool::TubeSensor | Tool::CarrierSensor => {
            let local = facing.0.rotation().inverse() * offset.extend(0.0);

            local.x.abs() <= BAR_LENGTH / 2.0 && (local.y - BAR_OFFSET).abs() <= BAR_THICKNESS / 2.0
        }
        // La linea e' sottile: pretendere il clic sui suoi quattro pixel
        // sarebbe una caccia al pixel. Vale tutta la cella, tanto sotto di lei
        // non c'e' nient'altro.
        Tool::Guide | Tool::GuideLine => offset.abs().cmple(Vec2::splat(GRID_STEP / 2.0)).all(),
        Tool::Antenna => {
            let local = facing.0.rotation().inverse() * offset.extend(0.0);

            local.truncate().distance(Vec2::Y * ANTENNA_OFFSET) <= ANTENNA_RADIUS
        }
        _ => offset.abs().cmple(Vec2::splat(PIECE_SIZE / 2.0)).all(),
    }
}

/// Che figura e che freccia competono a un tipo di oggetto. Sta in un posto
/// solo perche' la usano sia gli oggetti veri sia l'anteprima dell'editor: se
/// le due mappature fossero separate, prima o poi l'anteprima mostrerebbe una
/// cosa e il piazzamento ne farebbe un'altra.
pub fn dressing(shapes: &PieceShapes, tool: Tool) -> (Handle<Mesh>, Arrow) {
    match tool {
        Tool::CarrierSource => (shapes.square.clone(), Arrow::Straight),
        Tool::Gate => (shapes.bar.clone(), Arrow::None),
        // La traiettoria e' dentro la figura: una freccia in piu' direbbe la
        // stessa cosa due volte.
        Tool::Divert => (shapes.divert.clone(), Arrow::None),
        Tool::Atr => (shapes.atr.clone(), Arrow::None),
        Tool::Despawner => (shapes.square.clone(), Arrow::Stop),
        Tool::Turner => (shapes.square.clone(), Arrow::Straight),
        // Il tornante lo disegna la figura, e il giro e' sempre quello: non c'e'
        // un verso da aggiungere.
        Tool::Reverser => (shapes.reverser.clone(), Arrow::None),
        Tool::Antenna => (shapes.circle.clone(), Arrow::None),
        Tool::TubeSensor | Tool::CarrierSensor => (shapes.bar.clone(), Arrow::None),
        // Chiuse tutt'e due: e' l'anteprima, che non ha ancora una cella in cui
        // guardarsi attorno. I bordi veri glieli da' `refresh_guides` appena il
        // pezzo e' piazzato.
        Tool::Guide => (
            shapes.guide_lines(GuideShape::Corridor, crate::guide::CLOSED),
            Arrow::None,
        ),
        Tool::GuideLine => (
            shapes.guide_lines(GuideShape::Single, crate::guide::CLOSED),
            Arrow::None,
        ),
    }
}

/// Da' corpo a un oggetto: la figura del suo colore, con dentro la freccia che
/// gli compete. Chi non ha un verso resta una figura piena, e infatti il gate
/// blocca comunque lo si giri.
pub fn dress_shape(
    commands: &mut Commands,
    entity: Entity,
    shapes: &PieceShapes,
    shape: Handle<Mesh>,
    material: Handle<ColorMaterial>,
    arrow: Arrow,
) {
    commands
        .entity(entity)
        .insert((Mesh2d(shape), MeshMaterial2d(material)));

    let Some((mesh, material, tilt)) = (match arrow {
        Arrow::None => None,
        Arrow::Straight => Some((
            shapes.straight_arrow.clone(),
            shapes.arrow_material.clone(),
            0.0,
        )),
        Arrow::Stop => Some((shapes.stop.clone(), shapes.stop_material.clone(), 0.0)),
    }) else {
        return;
    };

    commands.entity(entity).with_child((
        Mesh2d(mesh),
        MeshMaterial2d(material),
        Transform::from_xyz(0.0, 0.0, ARROW_Z).with_rotation(Quat::from_rotation_z(tilt)),
    ));
}

/// Gira l'oggetto secondo il suo orientamento. Ruotare il quadrato equivale a
/// ruotare la freccia, visto che il quadrato e' simmetrico: cosi' basta un
/// unico sistema e la freccia non ha bisogno di saperne niente.
fn orient_pieces(pieces: Query<(&Facing, &mut Transform), Changed<Facing>>) {
    for (facing, mut transform) in pieces {
        transform.rotation = facing.0.rotation();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// La sbarra sta su un lato della cella e lascia libero il centro: e' li'
    /// che va l'antenna, ed e' li' che si ferma il carrier.
    #[test]
    fn the_bar_leans_on_the_side_and_leaves_the_middle_free() {
        let centre = Vec2::ZERO;
        let facing = Facing(Heading::Up);

        assert!(
            covers(Tool::Gate, facing, centre, Vec2::new(0.0, BAR_OFFSET)),
            "sulla sbarra"
        );
        assert!(
            !covers(Tool::Gate, facing, centre, centre),
            "il centro della cella resta dell'antenna"
        );
        // Girato di un quarto, la sbarra passa sull'altro lato.
        assert!(covers(
            Tool::Gate,
            Facing(Heading::Left),
            centre,
            Vec2::new(-BAR_OFFSET, 0.0)
        ));
        assert!(!covers(
            Tool::Gate,
            Facing(Heading::Left),
            centre,
            Vec2::new(0.0, BAR_OFFSET)
        ));
    }

    /// Gli altri oggetti restano quadrati, e coprono l'antenna che gli sta
    /// sotto: e' il prezzo di averla grande quanto un carrier.
    #[test]
    fn the_other_pieces_are_still_squares() {
        let inside = Vec2::splat(PIECE_SIZE / 2.0 - 1.0);
        let outside = Vec2::splat(PIECE_SIZE / 2.0 + 1.0);

        assert!(covers(Tool::Atr, Facing::default(), Vec2::ZERO, inside));
        assert!(!covers(Tool::Atr, Facing::default(), Vec2::ZERO, outside));
    }

    /// Da che parte si apre il fianco della corsia. E' la stessa regola che
    /// `Divert::shift` applica al movimento, vista da chi disegna: il divert
    /// sposta nel verso in cui e' girato, l'ATR e l'inversione alla propria
    /// sinistra. Le due devono restare d'accordo, altrimenti il disegno torna a
    /// raccontare una manovra che l'impianto non fa.
    #[test]
    fn the_lane_opens_where_the_carrier_leaves_it() {
        let up = Facing(Heading::Up);

        assert_eq!(Tool::Divert.opens_toward(up), Some(Heading::Up));
        assert_eq!(Tool::Atr.opens_toward(up), Some(Heading::Left));
        assert_eq!(
            Tool::Reverser.opens_toward(up),
            Some(Heading::Left),
            "anche l'inversione esce dal fianco sinistro"
        );
        assert_eq!(
            Tool::Turner.opens_toward(up),
            Some(Heading::Up),
            "la svolta esce di li' e il muro non deve chiuderle la strada"
        );
    }

    /// Chi non devia nessuno non apre niente, e con un tratto di guida convive
    /// senza contraddirlo: un gate ferma dentro la corsia, un'antenna guarda.
    #[test]
    fn a_piece_that_diverts_nobody_leaves_the_lane_closed() {
        for quiet in [Tool::Gate, Tool::Antenna, Tool::CarrierSource, Tool::Guide] {
            assert_eq!(quiet.opens_toward(Facing::default()), None, "{quiet:?}");
        }
    }

    /// L'antenna deve finire sotto ai carrier, che viaggiano a quota zero, e
    /// sotto agli oggetti di linea.
    #[test]
    fn the_antenna_lies_below_carriers_and_objects() {
        assert!(Tool::Antenna.layer().z() < 0.0);
        assert!(Tool::Antenna.layer().z() < Tool::Gate.layer().z());
    }

    /// L'antenna non e' al centro della cella ma spostata verso il lato, e si
    /// gira insieme all'oggetto: e' li' che si ferma il carrier da leggere.
    #[test]
    fn the_antenna_sits_where_the_stopped_carrier_stands() {
        let centre = Vec2::ZERO;
        let up = Facing(Heading::Up);

        assert!(covers(
            Tool::Antenna,
            up,
            centre,
            Vec2::new(0.0, ANTENNA_OFFSET)
        ));
        assert!(
            !covers(Tool::Antenna, up, centre, Vec2::new(0.0, -ANTENNA_OFFSET)),
            "dalla parte opposta non c'e'"
        );
        // Girata, si sposta con l'oggetto.
        assert!(covers(
            Tool::Antenna,
            Facing(Heading::Left),
            centre,
            Vec2::new(-ANTENNA_OFFSET, 0.0)
        ));
    }

    /// La freccia e' disegnata verso l'alto: la rotazione la porta nel verso
    /// dell'orientamento.
    #[test]
    fn the_rotation_points_the_arrow_where_the_carrier_goes() {
        for heading in [Heading::Left, Heading::Right, Heading::Up, Heading::Down] {
            let pointed = heading.rotation() * Vec3::Y;

            assert!(
                (pointed.truncate() - heading.as_vec()).length() < 0.001,
                "{heading:?} punta in {pointed:?}"
            );
        }
    }
}
