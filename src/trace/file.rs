//! Il formato di una registrazione, e l'aritmetica che lo regge.
//!
//! Qui non c'e' niente di Bevy oltre alle derive: si legge, si scrive e si
//! sommano differenze. E' voluto - questa e' la parte con un vincolo che non
//! scade, la compatibilita' con i file gia' salvati, ed e' anche l'unica su cui
//! si possa ragionare senza far girare un'applicazione. I test del modulo sono
//! tutti qui perche' e' qui che ci sono regole da provare; la macchina che lo
//! usa - registrare, rigiocare, la barra di scorrimento - sta nel modulo padre.

use std::error::Error;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use bevy::prelude::*;
use ron::ser::PrettyConfig;
use serde::{Deserialize, Serialize};

use crate::carrier::CarrierType;
use crate::layout::Layout;
use crate::switch::Switch;

/// Dove si trovava un carrier in un certo istante: id, x, y. E' una tupla e non
/// una struttura con i campi scritti perche' di righe come questa un file ne
/// contiene decine di migliaia, ed erano quattro ciascuna.
///
/// Che tipo di carrier sia non c'e' scritto: il tipo e' uno stato che cambia di
/// rado, quindi si registra come gli interruttori, solo quando cambia.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct TracedCarrier(pub u32, pub f32, pub f32);

impl TracedCarrier {
    pub fn id(self) -> u32 {
        self.0
    }

    pub fn at(self) -> Vec3 {
        Vec3::new(self.1, self.2, 0.0)
    }
}

/// Un istante della simulazione: chi c'era, dove, e quali interruttori sono
/// cambiati. Gli oggetti sono indicati per cella e non per posizione
/// nell'elenco: cosi' l'istante si legge da solo, senza dover contare.
///
/// I carrier ci sono tutti a ogni istante, perche' si muovono di continuo. Gli
/// interruttori invece cambiano di rado: elencarli tutti ogni volta gonfiava il
/// file di righe identiche, quindi si scrivono solo quando cambiano. Il primo
/// istante li porta tutti, perche' li' non c'e' un "prima".
///
/// Un file scritto alla maniera vecchia, con tutti gli interruttori in ogni
/// istante, resta valido: e' una sequenza di cambi anche quella, solo ripetuta.
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct TraceFrame {
    /// Il numero di questo istante, progressivo dall'inizio della
    /// registrazione. La riproduzione va avanti per posizione nell'elenco, ma
    /// il numero e' scritto lo stesso: e' quello che si legge sulla barra e che
    /// serve a ritrovare nel file un punto visto a schermo.
    #[serde(default)]
    pub id: u32,
    /// I carrier che si sono mossi, piu' quelli che compaiono adesso per la
    /// prima volta. Chi e' fermo - una coda davanti a un gate, per esempio - non
    /// compare: le sue coordinate sono quelle dell'istante prima, e riscriverle
    /// venti volte al secondo era la ripetizione piu' grossa del file.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub carriers: Vec<TracedCarrier>,
    /// Chi e' uscito dall'impianto in questo istante. Serve proprio perche' i
    /// fermi non compaiono: senza, "non e' nell'elenco" vorrebbe dire insieme
    /// "e' uscito" e "sta fermo", che sono cose opposte.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gone: Vec<u32>,
    /// Gli interruttori cambiati, ciascuno con l'id dell'oggetto a cui
    /// appartiene. L'id e' l'unica chiave che regge: la cella si sposta
    /// trascinando l'oggetto, e per giunta una cella puo' ospitarne tre
    /// sovrapposti. Negli istanti in cui non cambia niente il campo non viene
    /// scritto affatto.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub switches: Vec<(u32, Switch)>,
    /// I carrier che hanno cambiato tipo, piu' quelli che compaiono adesso per
    /// la prima volta: anche entrare in scena e' un cambio, visto che prima non
    /// c'erano. Un carrier che resta com'e' non compare qui.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub kinds: Vec<(u32, CarrierType)>,
}

/// Una registrazione completa. Contiene anche il layout, cosi' il file e' lo
/// scenario intero e non un elenco di coordinate senza contesto.
#[derive(Serialize, Deserialize, Debug, Default, PartialEq)]
pub struct Trace {
    pub fps: f32,
    pub layout: Layout,
    pub frames: Vec<TraceFrame>,
}

impl Trace {
    /// Quanto dura la registrazione.
    pub fn seconds(&self) -> f32 {
        self.frames.len() as f32 / self.fps
    }
}

pub fn to_ron(trace: &Trace) -> Result<String, ron::Error> {
    ron::ser::to_string_pretty(trace, PrettyConfig::default())
}

pub fn from_ron(text: &str) -> Result<Trace, ron::de::SpannedError> {
    ron::from_str(text)
}

pub fn save(trace: &Trace, path: &str) -> Result<(), Box<dyn Error>> {
    fs::write(path, to_ron(trace)?)?;
    Ok(())
}

pub fn load(path: &str) -> Result<Trace, Box<dyn Error>> {
    Ok(from_ron(&fs::read_to_string(path)?)?)
}

pub fn trace_path() -> String {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or_default();

    format!("registrazione-{stamp}.ron")
}

/// Gli interruttori che sono cambiati rispetto all'istante precedente. Chi non
/// c'era prima conta come cambiato: e' cosi' che il primo istante li porta tutti.
pub fn changes<T: Copy + PartialEq>(now: &[(u32, T)], before: &[(u32, T)]) -> Vec<(u32, T)> {
    now.iter()
        .filter(|(id, value)| {
            !before
                .iter()
                .any(|(was_id, was_value)| was_id == id && was_value == value)
        })
        .copied()
        .collect()
}

/// Chi si e' mosso rispetto all'istante prima, piu' chi e' appena arrivato.
pub fn moved(now: &[TracedCarrier], before: &[TracedCarrier]) -> Vec<TracedCarrier> {
    now.iter()
        .filter(|here| !before.iter().any(|was| was == *here))
        .copied()
        .collect()
}

/// Chi c'era e adesso non c'e' piu': uscito dall'impianto.
pub fn left(now: &[TracedCarrier], before: &[TracedCarrier]) -> Vec<u32> {
    before
        .iter()
        .filter(|was| !now.iter().any(|here| here.id() == was.id()))
        .map(|was| was.id())
        .collect()
}

/// Dove stavano tutti a un certo istante: si sommano gli spostamenti dall'inizio
/// e si tolgono quelli usciti per strada. Serve dopo un salto con la barra, che
/// scavalca gli istanti in mezzo.
pub fn places_up_to(trace: &Trace, index: usize) -> Vec<TracedCarrier> {
    let mut state: Vec<TracedCarrier> = Vec::new();

    for frame in trace.frames.iter().take(index + 1) {
        for here in &frame.carriers {
            match state.iter_mut().find(|known| known.id() == here.id()) {
                Some(known) => *known = *here,
                None => state.push(*here),
            }
        }
        state.retain(|known| !frame.gone.contains(&known.id()));
    }

    state
}

/// Applica dei cambi a uno stato: quello che c'era viene aggiornato, quello che
/// non c'era si aggiunge.
pub fn apply<T: Copy + PartialEq>(state: &mut Vec<(u32, T)>, changes: &[(u32, T)]) {
    for (id, value) in changes {
        match state.iter_mut().find(|(known, _)| known == id) {
            Some((_, known)) => *known = *value,
            None => state.push((*id, *value)),
        }
    }
}

/// Somma i cambi dall'inizio fino a quell'istante compreso: e' come stavano le
/// cose li'. Serve dopo un salto con la barra, quando gli istanti intermedi non
/// sono passati per la scena. Vale per gli interruttori e per i tipi dei
/// carrier, che si registrano allo stesso modo.
pub fn folded<T: Copy + PartialEq>(
    trace: &Trace,
    index: usize,
    of: impl Fn(&TraceFrame) -> &[(u32, T)],
) -> Vec<(u32, T)> {
    let mut state = Vec::new();

    for frame in trace.frames.iter().take(index + 1) {
        apply(&mut state, of(frame));
    }

    state
}

#[cfg(test)]
mod tests {
    use super::*;
    // La cadenza la decide chi registra, non il formato: il file se la porta
    // scritta dentro. Qui serve solo a costruire un esempio credibile.
    use crate::trace::TRACE_FPS;

    /// Un oggetto in servizio ma non comandato: il gate che lascia passare.
    const CLOSED: Switch = Switch {
        enabled: true,
        active: false,
    };

    fn sample() -> Trace {
        Trace {
            fps: TRACE_FPS,
            layout: Layout::default(),
            frames: vec![
                TraceFrame {
                    id: 0,
                    carriers: vec![TracedCarrier(1, 10.0, -20.0)],
                    gone: vec![],
                    // Il gate (1) e l'antenna che gli sta sotto (2), accesi.
                    switches: vec![(1, Switch::default()), (2, Switch::default())],
                    // Il carrier 1 entra in scena adesso, con la provetta.
                    kinds: vec![(1, CarrierType::WithTube)],
                },
                TraceFrame {
                    id: 1,
                    carriers: vec![TracedCarrier(1, 5.0, -20.0)],
                    gone: vec![],
                    // Nel secondo istante e' cambiato solo il gate: l'antenna
                    // e' rimasta com'era, e infatti non compare.
                    switches: vec![(1, CLOSED)],
                    // Il carrier ha perso la provetta per strada.
                    kinds: vec![(1, CarrierType::Empty)],
                },
            ],
        }
    }

    #[test]
    fn a_recording_survives_the_round_trip() {
        let written = to_ron(&sample()).expect("scrittura");

        assert_eq!(from_ron(&written).expect("rilettura"), sample());
    }

    /// La durata si ricava dal numero di istanti: e' l'unica cosa che serve
    /// sapere per rigiocarla alla velocita' giusta.
    #[test]
    fn the_length_comes_from_the_number_of_moments() {
        assert_eq!(sample().seconds(), 2.0 / TRACE_FPS);
    }

    /// Ogni oggetto ha il suo id, quindi due che condividono la cella - un gate
    /// e l'antenna che gli sta sotto - restano due voci distinte in un elenco
    /// solo. Era per non confonderli che prima ci volevano tre elenchi.
    #[test]
    fn objects_sharing_a_cell_keep_their_own_state() {
        let reread = from_ron(&to_ron(&sample()).expect("scrittura")).expect("rilettura");
        let closed = folded(&reread, 1, |frame| &frame.switches);

        assert_eq!(closed, vec![(1, CLOSED), (2, Switch::default())]);
    }

    /// Un istante senza cambi non ha proprio il campo, e si rilegge lo stesso.
    #[test]
    fn an_instant_without_changes_has_no_switches_at_all() {
        let quiet = "(fps: 20.0, layout: (objects: []), frames: [(carriers: [])])";

        let trace = from_ron(quiet).expect("rilettura");

        assert!(trace.frames[0].switches.is_empty());
    }

    /// Il tipo di un carrier si scrive quando cambia, non a ogni istante: e'
    /// uno stato, come l'acceso e spento degli oggetti. Un carrier che resta
    /// com'e' non fa scrivere niente.
    #[test]
    fn the_kind_of_a_carrier_is_written_only_when_it_changes() {
        let mut trace = sample();
        // Un terzo istante in cui il carrier prosegue senza cambiare.
        trace.frames.push(TraceFrame {
            id: 2,
            carriers: vec![TracedCarrier(1, 0.0, -20.0)],
            ..Default::default()
        });

        let written = to_ron(&trace).expect("scrittura");
        let reread = from_ron(&written).expect("rilettura");

        assert_eq!(written.matches("WithTube").count(), 1, "{written}");
        assert!(reread.frames[2].kinds.is_empty(), "niente da dire");
        assert_eq!(
            folded(&reread, 2, |frame| &frame.kinds),
            vec![(1, CarrierType::Empty)],
            "il tipo di allora e' la somma dei cambi fino a li'"
        );
    }

    /// Un carrier fermo non viene riscritto: chi si ferma davanti a un gate
    /// sparisce dagli istanti finche' non riparte, e le sue coordinate restano
    /// quelle dell'ultimo spostamento.
    #[test]
    fn a_carrier_standing_still_is_not_written_again() {
        let before = [TracedCarrier(1, 10.0, 0.0), TracedCarrier(2, 40.0, 0.0)];
        let now = [TracedCarrier(1, 10.0, 0.0), TracedCarrier(2, 35.0, 0.0)];

        assert_eq!(
            moved(&now, &before),
            vec![TracedCarrier(2, 35.0, 0.0)],
            "solo chi si e' spostato"
        );
        assert!(left(&now, &before).is_empty(), "nessuno e' uscito");
    }

    /// Chi esce dall'impianto va detto: senza, "non compare" vorrebbe dire
    /// insieme "e' uscito" e "sta fermo".
    #[test]
    fn who_leaves_the_plant_is_named() {
        let before = [TracedCarrier(1, 10.0, 0.0), TracedCarrier(2, 40.0, 0.0)];
        let now = [TracedCarrier(2, 35.0, 0.0)];

        assert_eq!(left(&now, &before), vec![1]);
    }

    /// Saltando con la barra la scena va rifatta: si sommano gli spostamenti
    /// dall'inizio e si tolgono quelli usciti per strada.
    #[test]
    fn the_scene_at_an_instant_is_the_sum_of_the_moves() {
        let trace = Trace {
            fps: TRACE_FPS,
            layout: Layout::default(),
            frames: vec![
                TraceFrame {
                    id: 0,
                    carriers: vec![TracedCarrier(1, 10.0, 0.0), TracedCarrier(2, 40.0, 0.0)],
                    ..Default::default()
                },
                // Il primo si ferma - e infatti non compare - il secondo avanza.
                TraceFrame {
                    id: 1,
                    carriers: vec![TracedCarrier(2, 35.0, 0.0)],
                    ..Default::default()
                },
                // Il fermo riparte, il secondo esce dall'impianto.
                TraceFrame {
                    id: 2,
                    carriers: vec![TracedCarrier(1, 5.0, 0.0)],
                    gone: vec![2],
                    ..Default::default()
                },
            ],
        };

        assert_eq!(
            places_up_to(&trace, 1),
            vec![TracedCarrier(1, 10.0, 0.0), TracedCarrier(2, 35.0, 0.0)],
            "il fermo sta dov'era"
        );
        assert_eq!(
            places_up_to(&trace, 2),
            vec![TracedCarrier(1, 5.0, 0.0)],
            "chi e' uscito non torna in scena"
        );
    }

    /// Ogni istante porta il proprio numero: e' quello che si legge sulla barra
    /// durante la riproduzione, e serve a ritrovare nel file il punto che si sta
    /// guardando a schermo.
    #[test]
    fn every_moment_carries_its_own_number() {
        let reread = from_ron(&to_ron(&sample()).expect("scrittura")).expect("rilettura");

        assert_eq!(reread.frames[0].id, 0);
        assert_eq!(reread.frames[1].id, 1);
    }

    /// Una posizione e' una riga sola: id, x, y. Di righe cosi' un file ne
    /// contiene decine di migliaia, e prima ne occupava quattro ciascuna.
    #[test]
    fn a_position_is_a_single_line() {
        let written = to_ron(&sample()).expect("scrittura");

        assert!(written.contains("(1, 10.0, -20.0)"), "{written}");
    }

    /// Il file porta con se' l'impianto: senza, le coordinate sarebbero numeri
    /// senza contesto e la riproduzione mostrerebbe carrier nel vuoto.
    #[test]
    fn the_file_carries_the_layout_too() {
        let written = to_ron(&sample()).expect("scrittura");

        assert!(written.contains("layout"), "{written}");
    }

    /// Il file non ripete gli interruttori a ogni istante: scrive solo quelli
    /// che cambiano. Su una registrazione lunga sono la parte che cresceva
    /// senza dire niente di nuovo.
    #[test]
    fn only_the_switches_that_changed_are_written() {
        let before = [(1, Switch::default()), (2, CLOSED)];
        let now = [(1, CLOSED), (2, CLOSED)];

        assert_eq!(changes(&now, &before), vec![(1, CLOSED)]);
        assert!(
            changes(&now, &now).is_empty(),
            "un istante identico al precedente non scrive niente"
        );
        assert_eq!(
            changes(&now, &[]),
            now.to_vec(),
            "il primo istante li porta tutti, non avendo un prima"
        );
    }

    /// Saltando con la barra gli istanti in mezzo non passano dalla scena:
    /// lo stato di quel punto si ottiene sommando i cambi dall'inizio.
    #[test]
    fn the_state_at_an_instant_is_the_sum_of_the_changes() {
        let trace = sample();

        let start = folded(&trace, 0, |frame| &frame.switches);
        assert_eq!(start, vec![(1, Switch::default()), (2, Switch::default())]);

        let later = folded(&trace, 1, |frame| &frame.switches);
        assert_eq!(
            later,
            vec![(1, CLOSED), (2, Switch::default())],
            "il gate si e' chiuso; l'antenna non e' cambiata, ma il suo stato \
             si porta avanti lo stesso"
        );
    }

    /// Gli interruttori sono registrati istante per istante: quello che si
    /// rivede non e' solo dove passavano i carrier, ma anche perche'.
    #[test]
    fn the_state_of_the_objects_travels_with_each_moment() {
        let trace = sample();

        assert_eq!(
            trace.frames[0].switches,
            vec![(1, Switch::default()), (2, Switch::default())]
        );
        assert_eq!(trace.frames[1].switches, vec![(1, CLOSED)]);

        let reread = from_ron(&to_ron(&trace).expect("scrittura")).expect("rilettura");

        assert_eq!(reread.frames[1].switches, trace.frames[1].switches);
    }
}
