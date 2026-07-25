use std::collections::HashMap;
use std::error::Error;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use bevy::prelude::*;
use ron::ser::PrettyConfig;
use serde::{Deserialize, Serialize};

use crate::carrier::{Carrier, CarrierType, Heading, spawn_carrier};
use crate::editor::{BUTTON_IDLE, PALETTE_WIDTH, button_label, top_button};
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

/// Un istante della simulazione: chi c'era, dove, e come erano messi gli
/// interruttori. Gli oggetti sono indicati per cella e non per posizione
/// nell'elenco: cosi' l'istante si legge da solo, senza dover contare.
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct TraceFrame {
    pub carriers: Vec<TracedCarrier>,
    pub switches: Vec<((i32, i32), bool)>,
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
}

#[derive(Resource, Default)]
pub struct Replay {
    trace: Option<Trace>,
    next_frame: usize,
    countdown: f32,
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
        self.next_frame = 0;
        self.countdown = 0.0;
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

#[derive(Component)]
struct ReplayButton;

#[derive(Component)]
struct ReplayLabel;

pub struct TracePlugin;

impl Plugin for TracePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Recording>()
            .init_resource::<Replay>()
            .init_resource::<ReplayNotice>()
            .add_systems(Startup, setup_trace_buttons)
            .add_systems(
                Update,
                (
                    toggle_recording,
                    record_frames,
                    toggle_replay,
                    play_frames.run_if(in_state(SimulationState::Replaying)),
                    refresh_buttons,
                    scrub,
                ),
            )
            // Chiudere la finestra mentre si registra non deve buttare via
            // quello che si e' raccolto.
            .add_systems(Last, save_on_exit);
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
        children![(
            Node {
                width: Val::Percent(0.0),
                height: Val::Percent(100.0),
                ..default()
            },
            BackgroundColor(REPLAYING_COLOR),
            ScrubFill,
        )],
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
            &GlobalTransform,
            &mut Visibility,
        ),
        With<ScrubBar>,
    >,
    mut fills: Query<&mut Node, With<ScrubFill>>,
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

    let Some(total) = replay.trace.as_ref().map(|trace| trace.frames.len()) else {
        return;
    };
    if total == 0 {
        return;
    }

    // Basta che il tasto sia premuto sopra la barra: cosi' si puo' trascinare
    // avanti e indietro invece di dover cliccare punto per punto.
    if mouse.pressed(MouseButton::Left) && *interaction != Interaction::None {
        if let Some(cursor) = windows.single().ok().and_then(|w| w.cursor_position()) {
            let width = node.size().x;
            let left = transform.translation().x - width / 2.0;
            let fraction = ((cursor.x - left) / width).clamp(0.0, 1.0);

            replay.next_frame = (fraction * total as f32) as usize;
        }
    }

    for mut fill in fills.iter_mut() {
        fill.width = Val::Percent(100.0 * replay.next_frame as f32 / total as f32);
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
        recording.active = true;
        recording.countdown = 0.0;
        recording.frames.clear();
        info!("registrazione avviata");
    }
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

    let frame = TraceFrame {
        carriers: carriers
            .iter()
            .map(|(carrier, transform)| TracedCarrier {
                id: carrier.carrier_id,
                kind: carrier.kind,
                at: (transform.translation.x, transform.translation.y),
            })
            .collect(),
        switches: placed
            .iter()
            .filter_map(|(entity, placed)| {
                switches
                    .get(entity)
                    .map(|active| ((placed.cell.x, placed.cell.y), active))
            })
            .collect(),
    };

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
    carriers: Query<Entity, With<Carrier>>,
    placed: Query<(Entity, &Placed)>,
) {
    if !pressed(&buttons) {
        return;
    }

    if *state.get() == SimulationState::Replaying {
        replay.trace = None;
        next_state.set(SimulationState::Paused);
        info!("riproduzione interrotta");
        return;
    }

    match newest_trace() {
        Some(path) => match load(&path) {
            Ok(trace) => {
                // Si sgombra tutto: quello che si vedra' arriva dal file,
                // impianto compreso. Riprodurre una registrazione sopra un
                // layout diverso mostrerebbe carrier che sfilano fra oggetti
                // che non c'entrano.
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
                error!("non riesco a leggere {path}: {error}");
                notice.show("Illeggibile");
            }
        },
        None => {
            warn!("nessuna registrazione da riprodurre: prima serve un Registra");
            notice.show("Nessuna");
        }
    }
}

/// La registrazione piu' recente nella cartella di lavoro.
fn newest_trace() -> Option<String> {
    let mut traces: Vec<String> = fs::read_dir(".")
        .ok()?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("registrazione-") && name.ends_with(".ron"))
        .collect();

    // Il nome contiene l'istante: l'ordine alfabetico e' l'ordine cronologico.
    traces.sort();
    traces.pop()
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

    replay.countdown -= time.delta_secs();
    if replay.countdown > 0.0 {
        return;
    }
    replay.countdown += 1.0 / fps;

    let wanted = replay.next_frame;
    replay.next_frame += 1;

    let frame = match replay
        .trace
        .as_ref()
        .and_then(|trace| trace.frames.get(wanted))
    {
        Some(frame) => frame.clone(),
        None => {
            info!("riproduzione finita");
            replay.trace = None;
            next_state.set(SimulationState::Paused);
            return;
        }
    };

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
    // che si vede.
    for ((x, y), active) in &frame.switches {
        let cell = IVec2::new(*x, *y);

        if let Some((entity, _)) = placed.iter().find(|(_, placed)| placed.cell == cell) {
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
    recording.active = true;
    recording.countdown = 0.0;
    recording.frames.clear();
    info!("registrazione avviata");
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
                },
                TraceFrame {
                    carriers: vec![TracedCarrier {
                        id: 1,
                        kind: CarrierType::WithTube,
                        at: (5.0, -20.0),
                    }],
                    // Nel secondo istante il gate e' stato chiuso.
                    switches: vec![((3, 0), false)],
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

    /// Il file porta con se' l'impianto: senza, le coordinate sarebbero numeri
    /// senza contesto e la riproduzione mostrerebbe carrier nel vuoto.
    #[test]
    fn the_file_carries_the_layout_too() {
        let written = to_ron(&sample()).expect("scrittura");

        assert!(written.contains("layout"), "{written}");
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
