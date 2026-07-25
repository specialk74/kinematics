use std::collections::HashMap;
use std::error::Error;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use bevy::prelude::*;
use ron::ser::PrettyConfig;
use serde::{Deserialize, Serialize};

use crate::carrier::{Carrier, CarrierType, Heading, spawn_carrier};
use crate::editor::{BUTTON_IDLE, button_label, top_button};
use crate::layout::{Layout, Placed, spawn_layout};
use crate::piece::Facing;
use crate::simulation::SimulationState;

/// Campionamenti al secondo. Non serve seguire il frame rate: a venti al secondo
/// il movimento e' gia' fluido e il file resta piccolo.
const TRACE_FPS: f32 = 20.0;
const RECORDING_COLOR: Color = Color::srgb(0.70, 0.15, 0.15);
const REPLAYING_COLOR: Color = Color::srgb(0.25, 0.45, 0.80);

/// Dove si trovava un carrier in un certo istante.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct TracedCarrier {
    pub id: u32,
    pub kind: CarrierType,
    pub at: (f32, f32),
}

/// Un istante della simulazione: chi c'era e dove.
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct TraceFrame {
    pub carriers: Vec<TracedCarrier>,
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

#[derive(Component)]
struct ReplayButton;

#[derive(Component)]
struct ReplayLabel;

pub struct TracePlugin;

impl Plugin for TracePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Recording>()
            .init_resource::<Replay>()
            .add_systems(Startup, setup_trace_buttons)
            .add_systems(
                Update,
                (
                    toggle_recording,
                    record_frames,
                    toggle_replay,
                    play_frames.run_if(in_state(SimulationState::Replaying)),
                    refresh_buttons,
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
    carriers: Query<Entity, With<Carrier>>,
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
                // Il nastro va sgombrato: quello che si vedra' arriva tutto dal
                // file, e i carrier vivi si sovrapporrebbero.
                for entity in carriers.iter() {
                    commands.entity(entity).despawn();
                }

                replay.start(trace);
                next_state.set(SimulationState::Replaying);
            }
            Err(error) => error!("non riesco a leggere {path}: {error}"),
        },
        None => warn!("nessuna registrazione da riprodurre"),
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
}

fn refresh_buttons(
    recording: Res<Recording>,
    state: Res<State<SimulationState>>,
    mut record_buttons: Query<&mut BackgroundColor, (With<RecordButton>, Without<ReplayButton>)>,
    mut replay_buttons: Query<&mut BackgroundColor, (With<ReplayButton>, Without<RecordButton>)>,
    mut record_labels: Query<&mut Text, (With<RecordLabel>, Without<ReplayLabel>)>,
    mut replay_labels: Query<&mut Text, (With<ReplayLabel>, Without<RecordLabel>)>,
) {
    if !recording.is_changed() && !state.is_changed() {
        return;
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

    for mut background in replay_buttons.iter_mut() {
        background.0 = if replaying {
            REPLAYING_COLOR
        } else {
            BUTTON_IDLE
        };
    }
    for mut label in replay_labels.iter_mut() {
        label.0 = if replaying { "Stop" } else { "Riproduci" }.to_string();
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
                },
                TraceFrame {
                    carriers: vec![TracedCarrier {
                        id: 1,
                        kind: CarrierType::WithTube,
                        at: (5.0, -20.0),
                    }],
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
}
