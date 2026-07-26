use std::collections::HashMap;
use std::error::Error;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use bevy::prelude::*;
use bevy::ui::UiGlobalTransform;
use ron::ser::PrettyConfig;
use serde::{Deserialize, Serialize};

use crate::carrier::{Carrier, CarrierType, Heading, spawn_carrier};
use crate::editor::{
    BUTTON_IDLE, BUTTON_UNAVAILABLE, Mode, PALETTE_WIDTH, button_label, top_button,
};
use crate::layout::{Layout, Placed, spawn_layout};
use crate::name::{PieceId, PieceName};
use crate::piece::Facing;
use crate::simulation::SimulationState;
use crate::switch::Switch;

/// Campionamenti al secondo. Non serve seguire il frame rate: a venti al secondo
/// il movimento e' gia' fluido e il file resta piccolo.
const TRACE_FPS: f32 = 20.0;
const RECORDING_COLOR: Color = Color::srgb(0.70, 0.15, 0.15);
const REPLAYING_COLOR: Color = Color::srgb(0.25, 0.45, 0.80);
/// Per quanto tempo il bottone spiega perche' non e' partito niente.
const NOTICE_SECONDS: f32 = 2.0;
const NOTICE_COLOR: Color = Color::srgb(0.75, 0.45, 0.10);

/// Messaggio momentaneo sul bottone Riproduci. Senza, chi preme e non vede
/// accadere niente non ha modo di sapere perche': il log non ce l'ha davanti.
#[derive(Resource, Default)]
struct ReplayNotice(Option<(&'static str, Timer)>);

impl ReplayNotice {
    fn show(&mut self, message: &'static str) {
        self.0 = Some((
            message,
            Timer::from_seconds(NOTICE_SECONDS, TimerMode::Once),
        ));
    }
}

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

fn trace_path() -> String {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or_default();

    format!("registrazione-{stamp}.ron")
}

#[derive(Resource, Default)]
pub struct Recording {
    active: bool,
    countdown: f32,
    frames: Vec<TraceFrame>,
    /// Di che tipo era ogni carrier nell'istante scritto prima: il termine di
    /// paragone per scrivere solo i cambi, come per gli interruttori.
    last_kinds: Vec<(u32, CarrierType)>,
    /// Dove stava ognuno nell'istante scritto prima, per lo stesso motivo.
    last_places: Vec<TracedCarrier>,
    /// Come erano messi gli interruttori nell'istante scritto prima: e' il
    /// termine di paragone che permette di scrivere solo i cambi.
    last: Vec<(u32, Switch)>,
}

/// Gli interruttori che sono cambiati rispetto all'istante precedente. Chi non
/// c'era prima conta come cambiato: e' cosi' che il primo istante li porta tutti.
fn changes<T: Copy + PartialEq>(now: &[(u32, T)], before: &[(u32, T)]) -> Vec<(u32, T)> {
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
fn moved(now: &[TracedCarrier], before: &[TracedCarrier]) -> Vec<TracedCarrier> {
    now.iter()
        .filter(|here| !before.iter().any(|was| was == *here))
        .copied()
        .collect()
}

/// Chi c'era e adesso non c'e' piu': uscito dall'impianto.
fn left(now: &[TracedCarrier], before: &[TracedCarrier]) -> Vec<u32> {
    before
        .iter()
        .filter(|was| !now.iter().any(|here| here.id() == was.id()))
        .map(|was| was.id())
        .collect()
}

/// Dove stavano tutti a un certo istante: si sommano gli spostamenti dall'inizio
/// e si tolgono quelli usciti per strada. Serve dopo un salto con la barra, che
/// scavalca gli istanti in mezzo.
fn places_up_to(trace: &Trace, index: usize) -> Vec<TracedCarrier> {
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
fn apply<T: Copy + PartialEq>(state: &mut Vec<(u32, T)>, changes: &[(u32, T)]) {
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
fn folded<T: Copy + PartialEq>(
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

#[derive(Resource, Default)]
pub struct Replay {
    trace: Option<Trace>,
    /// L'istante da mostrare. Lo fa avanzare il tempo, oppure lo sposta di
    /// colpo la barra.
    frame: usize,
    /// L'ultimo istante gia' messo in scena. Serve a distinguere "il tempo non
    /// e' passato" da "sei saltato altrove": in pausa e' l'unico modo per
    /// accorgersi di un salto e ridisegnare.
    applied: Option<usize>,
    countdown: f32,
    paused: bool,
}

impl Replay {
    /// Prepara la riproduzione. Chi chiama deve anche portare lo stato a
    /// `Replaying`: e' quello a fermare la simulazione vera.
    pub fn start(&mut self, trace: Trace) {
        info!(
            "riproduco {:.1} s di registrazione ({} istanti)",
            trace.seconds(),
            trace.frames.len()
        );

        self.trace = Some(trace);
        self.frame = 0;
        self.applied = None;
        self.countdown = 0.0;
        self.paused = false;
    }

    /// Ferma e riprende lo scorrere della registrazione, senza ricominciare:
    /// e' quello che permette di guardare un istante con calma.
    pub fn toggle_pause(&mut self) {
        self.paused = !self.paused;
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }
}

#[derive(Component)]
struct RecordButton;

#[derive(Component)]
struct RecordLabel;

/// La barra sotto: mostra a che punto e' la riproduzione e permette di
/// spostarsi avanti e indietro.
#[derive(Component)]
struct ScrubBar;

#[derive(Component)]
struct ScrubFill;

/// A che punto si e' nella registrazione, in secondi. La barra da sola dice
/// "circa a meta'": per ritrovare un istante serve un numero.
#[derive(Component)]
struct ScrubTime;

#[derive(Component)]
struct ReplayButton;

#[derive(Component)]
struct ReplayLabel;

/// L'elenco delle registrazioni disponibili, aperto dal bottone Riproduci.
#[derive(Component)]
struct TraceList;

/// Una voce dell'elenco: porta con se' il file che rappresenta.
#[derive(Component)]
struct TraceEntry(String);

/// La registrazione vera e propria. Sta da sola perche' serve anche senza
/// finestra: `--hide_gui --record` fa girare l'impianto e ne raccoglie le
/// posizioni, che e' proprio il modo in cui si registra una sessione lunga.
pub struct TracePlugin;

impl Plugin for TracePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Recording>()
            .add_systems(Update, record_frames)
            // Uscire mentre si registra non deve buttare via quello che si e'
            // raccolto: senza finestra e' l'unico modo in cui la registrazione
            // finisce, visto che si chiude con Ctrl+C.
            .add_systems(Last, save_on_exit);
    }
}

/// I comandi e la riproduzione. Vogliono tutti qualcosa da guardare o da
/// premere, quindi vivono solo con la finestra.
pub struct TraceVisualsPlugin;

impl Plugin for TraceVisualsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Replay>()
            .init_resource::<ReplayNotice>()
            .add_systems(Startup, setup_trace_buttons)
            .add_systems(
                Update,
                (
                    toggle_recording,
                    toggle_replay,
                    play_frames.run_if(in_state(SimulationState::Replaying)),
                    choose_trace,
                    refresh_buttons,
                    scrub,
                ),
            );
    }
}

fn setup_trace_buttons(mut commands: Commands) {
    commands.spawn((
        top_button(3),
        BackgroundColor(BUTTON_IDLE),
        RecordButton,
        children![(button_label("Registra"), RecordLabel)],
    ));
    commands.spawn((
        top_button(4),
        BackgroundColor(BUTTON_IDLE),
        ReplayButton,
        children![(button_label("Riproduci"), ReplayLabel)],
    ));

    commands.spawn((
        Button,
        Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(12.0),
            left: Val::Px(PALETTE_WIDTH + 12.0),
            right: Val::Px(12.0),
            height: Val::Px(22.0),
            ..default()
        },
        BackgroundColor(BUTTON_IDLE),
        // Nasce nascosta: senza una registrazione in corso non ha niente da dire.
        Visibility::Hidden,
        ScrubBar,
        children![
            (
                Node {
                    width: Val::Percent(0.0),
                    height: Val::Percent(100.0),
                    ..default()
                },
                BackgroundColor(REPLAYING_COLOR),
                ScrubFill,
            ),
            // Fuori dal flusso, altrimenti spingerebbe di lato il riempimento.
            (
                Text::new(""),
                TextFont {
                    font_size: 12.0,
                    ..default()
                },
                TextColor(Color::WHITE),
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(8.0),
                    top: Val::Px(3.0),
                    ..default()
                },
                ScrubTime,
            ),
        ],
    ));
}

/// Tiene la barra allineata alla riproduzione, e la usa come comando: premendo
/// dentro si salta a quell'istante. E' il modo per tornare indietro a guardare
/// di nuovo il punto che interessa, cosa che un filmato non permette di fare
/// con la stessa precisione.
fn scrub(
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    state: Res<State<SimulationState>>,
    mut replay: ResMut<Replay>,
    mut bars: Query<
        (
            &Interaction,
            &ComputedNode,
            // I nodi dell'interfaccia non hanno un `GlobalTransform`: il loro
            // posto nel mondo lo tiene `UiGlobalTransform`.
            &UiGlobalTransform,
            &mut Visibility,
        ),
        With<ScrubBar>,
    >,
    mut fills: Query<&mut Node, With<ScrubFill>>,
    mut times: Query<&mut Text, With<ScrubTime>>,
) {
    let replaying = *state.get() == SimulationState::Replaying;

    let Ok((interaction, node, transform, mut visibility)) = bars.single_mut() else {
        return;
    };
    *visibility = if replaying {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };

    let Some((total, fps, showing)) = replay.trace.as_ref().map(|trace| {
        // Il numero e' quello scritto nell'istante, non la sua posizione
        // nell'elenco: cosi' resta vero anche in un file tagliato a mano.
        let showing = trace
            .frames
            .get(replay.frame)
            .map(|frame| frame.id)
            .unwrap_or_default();

        (trace.frames.len(), trace.fps, showing)
    }) else {
        return;
    };
    if total == 0 {
        return;
    }

    // Solo con il tasto premuto *sulla barra*: passarci sopra per caso non
    // deve spostare niente, ma tenendo premuto si trascina avanti e indietro.
    if mouse.pressed(MouseButton::Left) && *interaction == Interaction::Pressed {
        // Il puntatore va chiesto in pixel fisici: `ComputedNode` e
        // `UiGlobalTransform` sono in quelli, e su uno schermo ad alta densita'
        // le due unita' differiscono di un fattore due.
        let cursor = windows
            .single()
            .ok()
            .and_then(|window| window.physical_cursor_position());

        if let Some(cursor) = cursor {
            let width = node.size().x;
            let left = transform.translation.x - width / 2.0;
            let fraction = ((cursor.x - left) / width).clamp(0.0, 1.0);

            replay.frame = (fraction * total as f32) as usize;
        }
    }

    for mut fill in fills.iter_mut() {
        fill.width = Val::Percent(100.0 * replay.frame as f32 / total as f32);
    }
    for mut time in times.iter_mut() {
        time.0 = format!(
            "{:.1} / {:.1} s   #{showing}",
            replay.frame as f32 / fps,
            total as f32 / fps
        );
    }
}

fn pressed<Button: Component>(
    buttons: &Query<&Interaction, (Changed<Interaction>, With<Button>)>,
) -> bool {
    buttons
        .iter()
        .any(|interaction| *interaction == Interaction::Pressed)
}

fn toggle_recording(
    buttons: Query<&Interaction, (Changed<Interaction>, With<RecordButton>)>,
    mode: Res<State<Mode>>,
    state: Res<State<SimulationState>>,
    mut recording: ResMut<Recording>,
    placed: Query<(&Placed, &Facing, &PieceId, &PieceName)>,
) {
    if !pressed(&buttons) || !tape_available(&mode, &state) {
        return;
    }

    if recording.active {
        stop_recording(&mut recording, &placed);
    } else {
        begin(&mut recording);
    }
}

/// Fa ripartire la raccolta da zero. Sta in un posto solo perche' l'avvio da
/// riga di comando e il bottone devono azzerare le stesse cose: in particolare
/// il termine di paragone degli interruttori, senza il quale una seconda
/// registrazione non scriverebbe il loro stato di partenza.
fn begin(recording: &mut Recording) {
    recording.active = true;
    recording.countdown = 0.0;
    recording.frames.clear();
    recording.last = Vec::new();
    recording.last_kinds = Vec::new();
    recording.last_places = Vec::new();
    info!("registrazione avviata");
}

/// Vero se i comandi del nastro rispondono. In editor non rispondono: non c'e'
/// niente da registrare, e far partire una riproduzione spegnerebbe l'editor
/// senza preavviso. Una riproduzione gia' in corso invece si deve poter fermare
/// da qualunque modo, altrimenti si resta chiusi dentro.
fn tape_available(mode: &State<Mode>, state: &State<SimulationState>) -> bool {
    *mode.get() == Mode::Simulating || *state.get() == SimulationState::Replaying
}

/// Chiude la registrazione e la scrive su file, layout compreso.
fn stop_recording(
    recording: &mut Recording,
    placed: &Query<(&Placed, &Facing, &PieceId, &PieceName)>,
) {
    recording.active = false;

    if recording.frames.is_empty() {
        warn!("registrazione vuota: niente da salvare");
        return;
    }

    let trace = Trace {
        fps: TRACE_FPS,
        layout: crate::layout::collect(placed),
        frames: std::mem::take(&mut recording.frames),
    };
    let path = trace_path();

    match save(&trace, &path) {
        Ok(()) => info!("registrazione salvata in {path} ({:.1} s)", trace.seconds()),
        Err(error) => error!("salvataggio della registrazione fallito: {error}"),
    }
}

/// Lo stato di tutti gli oggetti in scena, ciascuno con il suo id.
fn switch_states(objects: &Query<(&PieceId, &Switch)>) -> Vec<(u32, Switch)> {
    objects.iter().map(|(id, switch)| (id.0, *switch)).collect()
}

fn record_frames(
    time: Res<Time>,
    mut recording: ResMut<Recording>,
    carriers: Query<(&Carrier, &Transform)>,
    objects: Query<(&PieceId, &Switch)>,
) {
    if !recording.active {
        return;
    }

    recording.countdown -= time.delta_secs();
    if recording.countdown > 0.0 {
        return;
    }
    recording.countdown += 1.0 / TRACE_FPS;

    let now = switch_states(&objects);
    let kinds_now: Vec<(u32, CarrierType)> = carriers
        .iter()
        .map(|(carrier, _)| (carrier.carrier_id, carrier.kind))
        .collect();

    let places: Vec<TracedCarrier> = carriers
        .iter()
        .map(|(carrier, transform)| {
            TracedCarrier(
                carrier.carrier_id,
                transform.translation.x,
                transform.translation.y,
            )
        })
        .collect();

    let frame = TraceFrame {
        id: recording.frames.len() as u32,
        carriers: moved(&places, &recording.last_places),
        gone: left(&places, &recording.last_places),
        switches: changes(&now, &recording.last),
        kinds: changes(&kinds_now, &recording.last_kinds),
    };

    recording.last_kinds = kinds_now;
    recording.last_places = places;
    recording.last = now;
    recording.frames.push(frame);
}

fn save_on_exit(
    mut exits: MessageReader<AppExit>,
    mut recording: ResMut<Recording>,
    placed: Query<(&Placed, &Facing, &PieceId, &PieceName)>,
) {
    if exits.read().next().is_some() && recording.active {
        stop_recording(&mut recording, &placed);
    }
}

/// Avvia o interrompe la riproduzione dell'ultima registrazione salvata.
fn toggle_replay(
    mut commands: Commands,
    buttons: Query<&Interaction, (Changed<Interaction>, With<ReplayButton>)>,
    mode: Res<State<Mode>>,
    mut replay: ResMut<Replay>,
    state: Res<State<SimulationState>>,
    mut next_state: ResMut<NextState<SimulationState>>,
    mut notice: ResMut<ReplayNotice>,
    lists: Query<Entity, With<TraceList>>,
    carriers: Query<Entity, With<Carrier>>,
) {
    if !pressed(&buttons) || !tape_available(&mode, &state) {
        return;
    }

    if *state.get() == SimulationState::Replaying {
        replay.trace = None;
        // I carrier rigiocati devono sparire: restando in scena, alla
        // ripartenza la simulazione li adotterebbe come suoi e li farebbe
        // proseguire da dove li ha lasciati la registrazione.
        for entity in carriers.iter() {
            commands.entity(entity).despawn();
        }
        next_state.set(SimulationState::Paused);
        info!("riproduzione interrotta");
        return;
    }

    // Se l'elenco e' gia' aperto, il tasto lo richiude.
    if let Ok(open) = lists.single() {
        commands.entity(open).despawn();
        return;
    }

    let traces = available_traces();
    if traces.is_empty() {
        warn!("nessuna registrazione da riprodurre: prima serve un Registra");
        notice.show("Nessuna");
        return;
    }

    open_trace_list(&mut commands, &traces);
}

/// L'elenco delle registrazioni nella cartella di lavoro, dalla piu' recente.
/// Il nome contiene l'istante, quindi l'ordine alfabetico e' quello cronologico.
fn available_traces() -> Vec<String> {
    let mut traces: Vec<String> = fs::read_dir(".")
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("registrazione-") && name.ends_with(".ron"))
        .collect();

    traces.sort();
    traces.reverse();
    traces
}

fn open_trace_list(commands: &mut Commands, traces: &[String]) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(44.0),
                left: Val::Px(PALETTE_WIDTH + 12.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(6.0)),
                row_gap: Val::Px(4.0),
                ..default()
            },
            BackgroundColor(Color::srgb(0.10, 0.10, 0.12)),
            TraceList,
        ))
        .with_children(|list| {
            for name in traces {
                list.spawn((
                    Button,
                    Node {
                        padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
                        ..default()
                    },
                    BackgroundColor(BUTTON_IDLE),
                    TraceEntry(name.clone()),
                    children![button_label(name)],
                ));
            }
        });
}

/// Un clic su una voce dell'elenco fa partire quella registrazione.
fn choose_trace(
    mut commands: Commands,
    mode: Res<State<Mode>>,
    entries: Query<(&Interaction, &TraceEntry), Changed<Interaction>>,
    lists: Query<Entity, With<TraceList>>,
    mut replay: ResMut<Replay>,
    mut notice: ResMut<ReplayNotice>,
    mut next_state: ResMut<NextState<SimulationState>>,
    carriers: Query<Entity, With<Carrier>>,
    placed: Query<(Entity, &Placed)>,
    state: Res<State<SimulationState>>,
) {
    if !tape_available(&mode, &state) {
        return;
    }

    let Some((_, entry)) = entries
        .iter()
        .find(|(interaction, _)| **interaction == Interaction::Pressed)
    else {
        return;
    };

    match load(&entry.0) {
        Ok(trace) => {
            // Si sgombra tutto: quello che si vedra' arriva dal file, impianto
            // compreso. Riprodurre una registrazione sopra un layout diverso
            // mostrerebbe carrier che sfilano fra oggetti che non c'entrano.
            for entity in carriers.iter() {
                commands.entity(entity).despawn();
            }
            for (entity, _) in placed.iter() {
                commands.entity(entity).despawn();
            }
            spawn_layout(&mut commands, &trace.layout);

            replay.start(trace);
            next_state.set(SimulationState::Replaying);
        }
        Err(error) => {
            error!("non riesco a leggere {}: {error}", entry.0);
            notice.show("Illeggibile");
        }
    }

    for list in lists.iter() {
        commands.entity(list).despawn();
    }
}

/// Rimette in scena un istante della registrazione: sposta i carrier che c'erano
/// gia', crea quelli comparsi e toglie quelli spariti.
fn play_frames(
    mut commands: Commands,
    time: Res<Time>,
    mut replay: ResMut<Replay>,
    mut next_state: ResMut<NextState<SimulationState>>,
    mut carriers: Query<(Entity, &mut Carrier)>,
    mut positions: Query<&mut Transform>,
    mut objects: Query<(&PieceId, &mut Switch)>,
) {
    let Some(fps) = replay.trace.as_ref().map(|trace| trace.fps) else {
        return;
    };

    // Il tempo scorre solo se non e' in pausa; la barra invece puo' spostare
    // l'istante in qualunque momento, e anche allora la scena deve seguirlo.
    if !replay.paused {
        replay.countdown -= time.delta_secs();

        if replay.countdown <= 0.0 {
            replay.countdown += 1.0 / fps;
            replay.frame += 1;
        }
    }

    // Rimettere in scena costa: si fa solo quando l'istante e' cambiato
    // davvero, per scorrimento o per salto.
    if replay.applied == Some(replay.frame) {
        return;
    }

    let wanted = replay.frame;
    let frame = match replay
        .trace
        .as_ref()
        .and_then(|trace| trace.frames.get(wanted))
    {
        Some(frame) => frame.clone(),
        None => {
            info!("riproduzione finita");
            replay.trace = None;
            // Come per lo stop: quello che si vedeva era la registrazione, non
            // la simulazione, e non deve sopravviverle.
            for (entity, _) in carriers.iter() {
                commands.entity(entity).despawn();
            }
            next_state.set(SimulationState::Paused);
            return;
        }
    };
    let previous = replay.applied;
    replay.applied = Some(wanted);

    // Nel file ci sono solo i cambi, quindi scorrendo bastano quelli
    // dell'istante; dopo un salto con la barra invece gli istanti intermedi non
    // sono passati di qui, e va rifatta la somma dall'inizio. Vale per gli
    // interruttori e per i tipi dei carrier allo stesso modo.
    let scrolled = previous.map(|last| last + 1) == Some(wanted);
    let (board, kinds) = match (scrolled, replay.trace.as_ref()) {
        (false, Some(trace)) => (
            folded(trace, wanted, |frame| &frame.switches),
            folded(trace, wanted, |frame| &frame.kinds),
        ),
        _ => (frame.switches.clone(), frame.kinds.clone()),
    };
    let kind_of = |id: u32| {
        kinds
            .iter()
            .find(|(known, _)| *known == id)
            .map(|(_, kind)| *kind)
    };

    let mut live: HashMap<u32, Entity> = carriers
        .iter()
        .map(|(entity, carrier)| (carrier.carrier_id, entity))
        .collect();

    // Scorrendo, l'istante porta solo chi si e' mosso e chi e' uscito: il resto
    // della scena e' gia' al posto giusto da prima. Dopo un salto invece la
    // scena non c'entra niente con il punto in cui si e' finiti, e va rifatta
    // per intero sommando gli spostamenti dall'inizio.
    let whole = (!scrolled)
        .then(|| {
            replay
                .trace
                .as_ref()
                .map(|trace| places_up_to(trace, wanted))
        })
        .flatten();
    let places = whole.as_deref().unwrap_or(&frame.carriers);

    for traced in places {
        match live.remove(&traced.id()) {
            Some(entity) => {
                if let Ok(mut transform) = positions.get_mut(entity) {
                    transform.translation = traced.at();
                }
            }
            None => {
                let kind = kind_of(traced.id()).unwrap_or(CarrierType::Empty);

                spawn_carrier(&mut commands, traced.at(), kind, traced.id(), Heading::Left);
            }
        }
    }

    match whole.is_some() {
        // Dopo un salto, chi non risulta in scena a quell'istante non c'entra
        // piu' niente: era di un altro momento della registrazione.
        true => {
            for entity in live.into_values() {
                commands.entity(entity).despawn();
            }
        }
        // Scorrendo esce solo chi il file dice che e' uscito: gli altri che non
        // compaiono sono semplicemente fermi.
        false => {
            for id in &frame.gone {
                if let Some(entity) = live.get(id) {
                    commands.entity(*entity).despawn();
                }
            }
        }
    }

    // Un carrier che nel frattempo si e' svuotato o riempito. Si guarda l'elenco
    // dei tipi e non quello delle posizioni: il cambio puo' benissimo capitare a
    // un carrier fermo, che fra le posizioni non compare.
    for (id, kind) in &kinds {
        let found = carriers
            .iter()
            .find(|(_, carrier)| carrier.carrier_id == *id)
            .map(|(entity, _)| entity);

        if let Some(entity) = found
            && let Ok((_, mut carrier)) = carriers.get_mut(entity)
            && carrier.kind != *kind
        {
            carrier.kind = *kind;
        }
    }

    // Gli interruttori tornano come erano: un gate chiuso a meta' registrazione
    // deve richiudersi anche qui, altrimenti la scena non spiega piu' la coda
    // che si vede.
    restore(&board, &mut objects);
}

fn restore(states: &[(u32, Switch)], objects: &mut Query<(&PieceId, &mut Switch)>) {
    for (id, wanted) in states {
        for (known, mut switch) in objects.iter_mut() {
            if known.0 == *id && *switch != *wanted {
                *switch = *wanted;
            }
        }
    }
}

fn refresh_buttons(
    time: Res<Time>,
    recording: Res<Recording>,
    mode: Res<State<Mode>>,
    state: Res<State<SimulationState>>,
    mut notice: ResMut<ReplayNotice>,
    mut record_buttons: Query<&mut BackgroundColor, (With<RecordButton>, Without<ReplayButton>)>,
    mut replay_buttons: Query<&mut BackgroundColor, (With<ReplayButton>, Without<RecordButton>)>,
    mut record_labels: Query<&mut Text, (With<RecordLabel>, Without<ReplayLabel>)>,
    mut replay_labels: Query<&mut Text, (With<ReplayLabel>, Without<RecordLabel>)>,
) {
    // Il messaggio momentaneo va fatto scorrere a ogni frame, quindi qui non si
    // puo' uscire in fretta quando nulla e' cambiato.
    let mut pending = None;
    if let Some((message, timer)) = notice.0.as_mut() {
        timer.tick(time.delta());
        let message = *message;

        if timer.is_finished() {
            notice.0 = None;
        } else {
            pending = Some(message);
        }
    }

    let replaying = *state.get() == SimulationState::Replaying;
    let available = tape_available(&mode, &state);

    for mut background in record_buttons.iter_mut() {
        background.0 = match (available, recording.active) {
            (false, _) => BUTTON_UNAVAILABLE,
            (true, true) => RECORDING_COLOR,
            (true, false) => BUTTON_IDLE,
        };
    }
    for mut label in record_labels.iter_mut() {
        label.0 = if recording.active { "Stop" } else { "Registra" }.to_string();
    }

    // Un messaggio in corso ha la precedenza: e' l'unico momento in cui il
    // bottone deve spiegare qualcosa invece di dire cosa fa.
    let (colour, text) = match (pending, replaying, available) {
        (Some(message), _, _) => (NOTICE_COLOR, message),
        (None, true, _) => (REPLAYING_COLOR, "Stop"),
        (None, false, false) => (BUTTON_UNAVAILABLE, "Riproduci"),
        (None, false, true) => (BUTTON_IDLE, "Riproduci"),
    };

    for mut background in replay_buttons.iter_mut() {
        background.0 = colour;
    }
    for mut label in replay_labels.iter_mut() {
        label.0 = text.to_string();
    }
}

/// Avvia la registrazione all'apertura, per chi la chiede da riga di comando.
pub fn start_recording(mut recording: ResMut<Recording>) {
    begin(&mut recording);
}

/// Apre una registrazione passata da riga di comando: rimette in scena il suo
/// layout e la fa partire.
pub fn play_from_file(
    commands: &mut Commands,
    replay: &mut Replay,
    next_state: &mut NextState<SimulationState>,
    path: &str,
) {
    match load(path) {
        Ok(trace) => {
            spawn_layout(commands, &trace.layout);
            replay.start(trace);
            next_state.set(SimulationState::Replaying);
        }
        Err(error) => error!("non riesco a leggere {path}: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
