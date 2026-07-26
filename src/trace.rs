use std::collections::HashMap;
use std::error::Error;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use bevy::prelude::*;
use bevy::ui::UiGlobalTransform;
use ron::ser::PrettyConfig;
use serde::{Deserialize, Serialize};

use crate::carrier::{Carrier, CarrierType, Heading, spawn_carrier};
use crate::editor::{BUTTON_IDLE, Layer, PALETTE_WIDTH, button_label, top_button};
use crate::layout::{Layout, Placed, Switches, spawn_layout};
use crate::piece::Facing;
use crate::simulation::SimulationState;

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

/// Dove si trovava un carrier in un certo istante.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct TracedCarrier {
    pub id: u32,
    pub kind: CarrierType,
    pub at: (f32, f32),
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
    pub carriers: Vec<TracedCarrier>,
    /// Gli oggetti di linea che sono cambiati, per cella. Negli istanti in cui
    /// non cambia niente il campo non viene proprio scritto: erano tre righe
    /// vuote per istante, e un file lungo e' fatto di istanti in cui non cambia
    /// niente.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub switches: Vec<((i32, i32), bool)>,
    /// I sensori sulle pareti, sempre per cella. Assente nelle registrazioni
    /// fatte prima che esistessero.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub side: Vec<((i32, i32), bool)>,
    /// Quelli del piano di sotto, sempre per cella. Stanno in un elenco a parte
    /// perche' un'antenna puo' condividere la cella con un oggetto di linea: in
    /// un elenco solo le due voci avrebbero la stessa chiave e in riproduzione
    /// non si saprebbe piu' quale stato appartiene a quale oggetto. Assente
    /// nelle registrazioni fatte prima delle antenne, che restano leggibili.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub under: Vec<((i32, i32), bool)>,
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
    /// Come erano messi gli interruttori nell'istante scritto prima: e' il
    /// termine di paragone che permette di scrivere solo i cambi.
    last: Switchboard,
}

/// Lo stato di tutti gli interruttori in un istante, un elenco per piano.
#[derive(Default, Clone, PartialEq, Debug)]
struct Switchboard {
    track: Vec<((i32, i32), bool)>,
    side: Vec<((i32, i32), bool)>,
    under: Vec<((i32, i32), bool)>,
}

impl Switchboard {
    /// I cambi di un solo istante, presi cosi' come stanno nel file.
    fn of(frame: &TraceFrame) -> Self {
        Switchboard {
            track: frame.switches.clone(),
            side: frame.side.clone(),
            under: frame.under.clone(),
        }
    }
}

/// Gli interruttori che sono cambiati rispetto all'istante precedente. Chi non
/// c'era prima conta come cambiato: e' cosi' che il primo istante li porta tutti.
fn changes(now: &[((i32, i32), bool)], before: &[((i32, i32), bool)]) -> Vec<((i32, i32), bool)> {
    now.iter()
        .filter(|(cell, active)| {
            !before
                .iter()
                .any(|(was_cell, was_active)| was_cell == cell && was_active == active)
        })
        .copied()
        .collect()
}

/// Applica dei cambi a uno stato: quello che c'era viene aggiornato, quello che
/// non c'era si aggiunge.
fn apply(state: &mut Vec<((i32, i32), bool)>, changes: &[((i32, i32), bool)]) {
    for (cell, active) in changes {
        match state.iter_mut().find(|(known, _)| known == cell) {
            Some((_, known)) => *known = *active,
            None => state.push((*cell, *active)),
        }
    }
}

/// Somma i cambi dall'inizio fino a quell'istante compreso: e' come erano messi
/// gli interruttori li'. Serve dopo un salto con la barra, quando gli istanti
/// intermedi non sono passati per la scena.
fn switchboard_up_to(trace: &Trace, index: usize) -> Switchboard {
    let mut board = Switchboard::default();

    for frame in trace.frames.iter().take(index + 1) {
        apply(&mut board.track, &frame.switches);
        apply(&mut board.side, &frame.side);
        apply(&mut board.under, &frame.under);
    }

    board
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

    let Some((total, fps)) = replay
        .trace
        .as_ref()
        .map(|trace| (trace.frames.len(), trace.fps))
    else {
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
            "{:.1} / {:.1} s",
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
    mut recording: ResMut<Recording>,
    placed: Query<(&Placed, &Facing)>,
) {
    if !pressed(&buttons) {
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
    recording.last = Switchboard::default();
    info!("registrazione avviata");
}

/// Chiude la registrazione e la scrive su file, layout compreso.
fn stop_recording(recording: &mut Recording, placed: &Query<(&Placed, &Facing)>) {
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

/// Lo stato degli interruttori di un piano solo.
fn switches_on(
    layer: Layer,
    placed: &Query<(Entity, &Placed)>,
    switches: &Switches,
) -> Vec<((i32, i32), bool)> {
    placed
        .iter()
        .filter(|(_, placed)| placed.tool.layer() == layer)
        .filter_map(|(entity, placed)| {
            switches
                .get(entity)
                .map(|active| ((placed.cell.x, placed.cell.y), active))
        })
        .collect()
}

fn record_frames(
    time: Res<Time>,
    mut recording: ResMut<Recording>,
    carriers: Query<(&Carrier, &Transform)>,
    placed: Query<(Entity, &Placed)>,
    switches: Switches,
) {
    if !recording.active {
        return;
    }

    recording.countdown -= time.delta_secs();
    if recording.countdown > 0.0 {
        return;
    }
    recording.countdown += 1.0 / TRACE_FPS;

    let now = Switchboard {
        track: switches_on(Layer::Track, &placed, &switches),
        side: switches_on(Layer::Side, &placed, &switches),
        under: switches_on(Layer::Under, &placed, &switches),
    };

    let frame = TraceFrame {
        carriers: carriers
            .iter()
            .map(|(carrier, transform)| TracedCarrier {
                id: carrier.carrier_id,
                kind: carrier.kind,
                at: (transform.translation.x, transform.translation.y),
            })
            .collect(),
        switches: changes(&now.track, &recording.last.track),
        side: changes(&now.side, &recording.last.side),
        under: changes(&now.under, &recording.last.under),
    };

    recording.last = now;
    recording.frames.push(frame);
}

fn save_on_exit(
    mut exits: MessageReader<AppExit>,
    mut recording: ResMut<Recording>,
    placed: Query<(&Placed, &Facing)>,
) {
    if exits.read().next().is_some() && recording.active {
        stop_recording(&mut recording, &placed);
    }
}

/// Avvia o interrompe la riproduzione dell'ultima registrazione salvata.
fn toggle_replay(
    mut commands: Commands,
    buttons: Query<&Interaction, (Changed<Interaction>, With<ReplayButton>)>,
    mut replay: ResMut<Replay>,
    state: Res<State<SimulationState>>,
    mut next_state: ResMut<NextState<SimulationState>>,
    mut notice: ResMut<ReplayNotice>,
    lists: Query<Entity, With<TraceList>>,
    carriers: Query<Entity, With<Carrier>>,
) {
    if !pressed(&buttons) {
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
    entries: Query<(&Interaction, &TraceEntry), Changed<Interaction>>,
    lists: Query<Entity, With<TraceList>>,
    mut replay: ResMut<Replay>,
    mut notice: ResMut<ReplayNotice>,
    mut next_state: ResMut<NextState<SimulationState>>,
    carriers: Query<Entity, With<Carrier>>,
    placed: Query<(Entity, &Placed)>,
) {
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
    carriers: Query<(Entity, &Carrier)>,
    mut positions: Query<&mut Transform>,
    placed: Query<(Entity, &Placed)>,
    mut switches: Switches,
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

    let mut live: HashMap<u32, Entity> = carriers
        .iter()
        .map(|(entity, carrier)| (carrier.carrier_id, entity))
        .collect();

    for traced in &frame.carriers {
        let at = Vec3::new(traced.at.0, traced.at.1, 0.0);

        match live.remove(&traced.id) {
            Some(entity) => {
                if let Ok(mut transform) = positions.get_mut(entity) {
                    transform.translation = at;
                }
            }
            None => {
                spawn_carrier(&mut commands, at, traced.kind, traced.id, Heading::Left);
            }
        }
    }

    // Chi non compare in questo istante era gia' uscito dall'impianto.
    for entity in live.into_values() {
        commands.entity(entity).despawn();
    }

    // Gli interruttori tornano come erano: un gate chiuso a meta' registrazione
    // deve richiudersi anche qui, altrimenti la scena non spiega piu' la coda
    // che si vede. I piani si ripristinano separatamente: e' l'unico modo di non
    // confondere un'antenna con l'oggetto che le sta sopra.
    //
    // Nel file ci sono solo i cambi, quindi scorrendo basta applicare quelli
    // dell'istante; dopo un salto con la barra invece i cambi intermedi non sono
    // passati di qui, e va rifatta la somma dall'inizio.
    let scrolled = previous.map(|last| last + 1) == Some(wanted);
    let board = match (scrolled, replay.trace.as_ref()) {
        (false, Some(trace)) => switchboard_up_to(trace, wanted),
        _ => Switchboard::of(&frame),
    };

    restore(Layer::Track, &board.track, &placed, &mut switches);
    restore(Layer::Side, &board.side, &placed, &mut switches);
    restore(Layer::Under, &board.under, &placed, &mut switches);
}

fn restore(
    layer: Layer,
    states: &[((i32, i32), bool)],
    placed: &Query<(Entity, &Placed)>,
    switches: &mut Switches,
) {
    for ((x, y), active) in states {
        let cell = IVec2::new(*x, *y);
        let found = placed
            .iter()
            .find(|(_, placed)| placed.cell == cell && placed.tool.layer() == layer);

        if let Some((entity, _)) = found {
            switches.set(entity, *active);
        }
    }
}

fn refresh_buttons(
    time: Res<Time>,
    recording: Res<Recording>,
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

    for mut background in record_buttons.iter_mut() {
        background.0 = if recording.active {
            RECORDING_COLOR
        } else {
            BUTTON_IDLE
        };
    }
    for mut label in record_labels.iter_mut() {
        label.0 = if recording.active { "Stop" } else { "Registra" }.to_string();
    }

    // Un messaggio in corso ha la precedenza: e' l'unico momento in cui il
    // bottone deve spiegare qualcosa invece di dire cosa fa.
    let (colour, text) = match (pending, replaying) {
        (Some(message), _) => (NOTICE_COLOR, message),
        (None, true) => (REPLAYING_COLOR, "Stop"),
        (None, false) => (BUTTON_IDLE, "Riproduci"),
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

    fn sample() -> Trace {
        Trace {
            fps: TRACE_FPS,
            layout: Layout::default(),
            frames: vec![
                TraceFrame {
                    carriers: vec![TracedCarrier {
                        id: 1,
                        kind: CarrierType::WithTube,
                        at: (10.0, -20.0),
                    }],
                    switches: vec![((3, 0), true)],
                    side: vec![((3, 0), true)],
                    // Un'antenna accesa sotto quello stesso gate.
                    under: vec![((3, 0), true)],
                },
                TraceFrame {
                    carriers: vec![TracedCarrier {
                        id: 1,
                        kind: CarrierType::WithTube,
                        at: (5.0, -20.0),
                    }],
                    // Nel secondo istante e' cambiato solo il gate: l'antenna
                    // sotto e il sensore accanto sono rimasti come stavano, e
                    // infatti non compaiono.
                    switches: vec![((3, 0), false)],
                    side: vec![],
                    under: vec![],
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

    /// Un'antenna sotto un gate ha una vita sua: i due stati devono restare
    /// distinguibili anche se la cella e' la stessa.
    #[test]
    fn an_antenna_keeps_its_own_state_under_an_object() {
        let reread = from_ron(&to_ron(&sample()).expect("scrittura")).expect("rilettura");
        let closed = switchboard_up_to(&reread, 1);

        assert_eq!(closed.track, vec![((3, 0), false)], "il gate e' chiuso");
        assert_eq!(closed.under, vec![((3, 0), true)], "l'antenna e' accesa");
    }

    /// Le registrazioni fatte prima delle antenne non hanno il piano di sotto e
    /// devono continuare ad aprirsi.
    #[test]
    fn an_old_recording_without_the_lower_layer_still_loads() {
        let old = "(fps: 20.0, layout: (objects: []), frames: [(carriers: [], switches: [((1, 2), true)])])";

        let trace = from_ron(old).expect("rilettura");

        assert!(trace.frames[0].under.is_empty());
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
        let before = [((3, 0), true), ((5, 0), false)];
        let now = [((3, 0), false), ((5, 0), false)];

        assert_eq!(changes(&now, &before), vec![((3, 0), false)]);
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

        let start = switchboard_up_to(&trace, 0);
        assert_eq!(start.track, vec![((3, 0), true)]);

        let later = switchboard_up_to(&trace, 1);
        assert_eq!(later.track, vec![((3, 0), false)], "il gate si e' chiuso");
        assert_eq!(
            later.under,
            vec![((3, 0), true)],
            "l'antenna non e' cambiata, ma il suo stato si porta avanti"
        );
    }

    /// Gli interruttori sono registrati istante per istante: quello che si
    /// rivede non e' solo dove passavano i carrier, ma anche perche'.
    #[test]
    fn the_state_of_the_objects_travels_with_each_moment() {
        let trace = sample();

        assert_eq!(trace.frames[0].switches, vec![((3, 0), true)]);
        assert_eq!(trace.frames[1].switches, vec![((3, 0), false)]);

        let reread = from_ron(&to_ron(&trace).expect("scrittura")).expect("rilettura");

        assert_eq!(reread.frames[1].switches, trace.frames[1].switches);
    }
}
