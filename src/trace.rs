use std::collections::HashMap;
use std::fs;

use bevy::prelude::*;
use bevy::ui::UiGlobalTransform;

use crate::carrier::{Carrier, CarrierType, Heading, spawn_carrier};
use crate::layout::{Layout, Placed, spawn_layout};
use crate::name::{PieceId, PieceName};
use crate::piece::Facing;
use crate::simulation::Mode;
use crate::simulation::SimulationState;
use crate::switch::Switch;
use crate::ui::{
    BUTTON_IDLE, BUTTON_READY, BUTTON_UNAVAILABLE, PALETTE_WIDTH, button_label, top_button,
};

mod file;

use file::{
    Trace, TraceFrame, TracedCarrier, changes, folded, left, load, moved, places_up_to, save,
    trace_path,
};

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

/// L'impianto che c'era prima di far partire una riproduzione. Una
/// registrazione porta con se' il proprio layout e per mostrarlo deve sgombrare
/// la scena: senza mettere da parte quello di prima, finita la riproduzione il
/// lavoro dell'editor sarebbe perso. Se la scena era vuota si mette da parte il
/// vuoto, e alla fine si torna al vuoto.
#[derive(Resource, Default)]
pub struct ParkedLayout(Option<Layout>);

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
            .init_resource::<ParkedLayout>()
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
    placed: Query<(&Placed, &Facing, Option<&PieceId>, Option<&PieceName>)>,
) {
    if !pressed(&buttons) || !can_record(&mode, &state, &recording) {
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

/// Mette da parte l'impianto che c'e' adesso, se non c'e' gia' qualcosa in
/// serbo: una seconda riproduzione avviata di seguito non deve seppellire il
/// layout dell'editor con quello della registrazione precedente.
fn park_layout<'a>(
    parked: &mut ParkedLayout,
    pieces: impl Iterator<
        Item = (
            &'a Placed,
            &'a Facing,
            Option<&'a PieceId>,
            Option<&'a PieceName>,
        ),
    >,
) {
    if parked.0.is_none() {
        parked.0 = Some(crate::layout::collect(pieces));
    }
}

/// Rimette in scena l'impianto di prima e toglie quello della registrazione.
/// Senza niente in serbo non fa nulla: vuol dire che nessuna riproduzione ha
/// mai sgombrato la scena.
fn unpark_layout(
    commands: &mut Commands,
    parked: &mut ParkedLayout,
    placed: &Query<Entity, With<Placed>>,
) {
    let Some(layout) = parked.0.take() else {
        return;
    };

    for entity in placed.iter() {
        commands.entity(entity).despawn();
    }

    spawn_layout(commands, &layout);
}

/// Vero se si puo' registrare o smettere di registrare. In editor non c'e'
/// niente da registrare, perche' il tempo sta fermo; durante una riproduzione
/// nemmeno, perche' quello che si vede arriva da un file e riscriverlo non
/// aggiungerebbe niente. Una registrazione in corso invece si deve poter
/// chiudere sempre, altrimenti resterebbe aperta.
fn can_record(mode: &State<Mode>, state: &State<SimulationState>, recording: &Recording) -> bool {
    recording.active
        || (*mode.get() == Mode::Simulating && *state.get() != SimulationState::Replaying)
}

/// Vero se si puo' far partire o fermare una riproduzione. Si puo' quasi
/// sempre, editor compreso: rivedere una registrazione e' un'azione a se',
/// e prende lo schermo per conto suo. L'unico momento in cui non si puo' e'
/// mentre si sta registrando: le due cose si pesterebbero i piedi.
fn can_replay(recording: &Recording) -> bool {
    !recording.active
}

/// Chiude la registrazione e la scrive su file, layout compreso.
fn stop_recording(
    recording: &mut Recording,
    placed: &Query<(&Placed, &Facing, Option<&PieceId>, Option<&PieceName>)>,
) {
    recording.active = false;

    if recording.frames.is_empty() {
        warn!("registrazione vuota: niente da salvare");
        return;
    }

    let trace = Trace {
        fps: TRACE_FPS,
        layout: crate::layout::collect(placed.iter()),
        frames: std::mem::take(&mut recording.frames),
    };
    let path = trace_path();

    match save(&trace, &path) {
        Ok(()) => info!("registrazione salvata in {path} ({:.1} s)", trace.seconds()),
        Err(error) => error!("salvataggio della registrazione fallito: {error}"),
    }
}

/// Lo stato di tutti gli oggetti in scena, ciascuno con il suo id.
/// Un pezzo passivo non ha ne' id ne' interruttori, quindi non compare qui
/// senza bisogno di escluderlo: la query da sola non lo trova.
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
    placed: Query<(&Placed, &Facing, Option<&PieceId>, Option<&PieceName>)>,
) {
    if exits.read().next().is_some() && recording.active {
        stop_recording(&mut recording, &placed);
    }
}

/// Avvia o interrompe la riproduzione dell'ultima registrazione salvata.
fn toggle_replay(
    mut commands: Commands,
    buttons: Query<&Interaction, (Changed<Interaction>, With<ReplayButton>)>,
    recording: Res<Recording>,
    mut replay: ResMut<Replay>,
    state: Res<State<SimulationState>>,
    mut next_state: ResMut<NextState<SimulationState>>,
    mut notice: ResMut<ReplayNotice>,
    mut parked: ResMut<ParkedLayout>,
    lists: Query<Entity, With<TraceList>>,
    carriers: Query<Entity, With<Carrier>>,
    placed: Query<Entity, With<Placed>>,
) {
    if !pressed(&buttons) || !can_replay(&recording) {
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
        unpark_layout(&mut commands, &mut parked, &placed);
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
    recording: Res<Recording>,
    mut parked: ResMut<ParkedLayout>,
    pieces: Query<(&Placed, &Facing, Option<&PieceId>, Option<&PieceName>)>,
    entries: Query<(&Interaction, &TraceEntry), Changed<Interaction>>,
    lists: Query<Entity, With<TraceList>>,
    mut replay: ResMut<Replay>,
    mut notice: ResMut<ReplayNotice>,
    mut next_state: ResMut<NextState<SimulationState>>,
    carriers: Query<Entity, With<Carrier>>,
    placed: Query<(Entity, &Placed)>,
) {
    if !can_replay(&recording) {
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
            // Quello che c'era si mette da parte e torna alla fine.
            park_layout(&mut parked, pieces.iter());
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
    mut parked: ResMut<ParkedLayout>,
    placed: Query<Entity, With<Placed>>,
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
            // la simulazione, e non deve sopravviverle. Impianto compreso: torna
            // quello che c'era prima, o il vuoto se non c'era niente.
            for (entity, _) in carriers.iter() {
                commands.entity(entity).despawn();
            }
            unpark_layout(&mut commands, &mut parked, &placed);
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
    // Come per l'avviso del salvataggio: il messaggio sul bottone dura quanto
    // deve durare per chi legge, non quanto dura per i carrier.
    time: Res<Time<Real>>,
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

    for mut background in record_buttons.iter_mut() {
        background.0 = match (can_record(&mode, &state, &recording), recording.active) {
            (false, _) => BUTTON_UNAVAILABLE,
            (true, true) => RECORDING_COLOR,
            (true, false) => BUTTON_READY,
        };
    }
    for mut label in record_labels.iter_mut() {
        label.0 = if recording.active { "Stop" } else { "Registra" }.to_string();
    }

    // Un messaggio in corso ha la precedenza: e' l'unico momento in cui il
    // bottone deve spiegare qualcosa invece di dire cosa fa.
    let (colour, text) = match (pending, replaying, can_replay(&recording)) {
        (Some(message), _, _) => (NOTICE_COLOR, message),
        (None, true, _) => (REPLAYING_COLOR, "Stop"),
        (None, false, false) => (BUTTON_UNAVAILABLE, "Riproduci"),
        (None, false, true) => (BUTTON_READY, "Riproduci"),
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
    parked: &mut ParkedLayout,
    pieces: &Query<(
        Entity,
        &Placed,
        &Facing,
        Option<&PieceId>,
        Option<&PieceName>,
    )>,
    next_state: &mut NextState<SimulationState>,
    path: &str,
) {
    match load(path) {
        Ok(trace) => {
            // Si mette da parte quello che c'e' e si sgombra la scena, come fa
            // la scelta dal pannello: il layout passato con --layout non deve
            // restare a mescolarsi con quello della registrazione, e alla fine
            // deve tornare.
            park_layout(
                parked,
                pieces
                    .iter()
                    .map(|(_, placed, facing, id, name)| (placed, facing, id, name)),
            );
            for (entity, _, _, _, _) in pieces.iter() {
                commands.entity(entity).despawn();
            }

            spawn_layout(commands, &trace.layout);
            replay.start(trace);
            next_state.set(SimulationState::Replaying);
        }
        Err(error) => error!("non riesco a leggere {path}: {error}"),
    }
}
