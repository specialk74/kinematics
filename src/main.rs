use std::time::Duration;

use bevy::app::{ScheduleRunnerPlugin, TerminalCtrlCHandlerPlugin};
use bevy::log::LogPlugin;
use bevy::prelude::*;
use bevy::state::app::StatesPlugin;
use clap::Parser;

mod antenna;
mod camera;
mod carrier;
mod cli;
mod despawner;
mod divert;
mod editor;
mod engagement;
mod gate;
mod geometry;
mod grid;
mod guide;
mod layout;
mod mqtt;
mod mqtt_panel;
mod name;
mod name_panel;
mod piece;
mod reverser;
mod sensor;
mod simulation;
mod source;
mod switch;
mod trace;
mod turner;
mod ui;

use antenna::AntennaVisualsPlugin;
use camera::CameraPlugin;
use carrier::{CarrierPlugin, CarrierVisualsPlugin};
use cli::Options;
use despawner::{DespawnerPlugin, DespawnerVisualsPlugin};
use divert::DivertVisualsPlugin;
use editor::EditorPlugin;
use engagement::EngagementPlugin;
use gate::GateVisualsPlugin;
use grid::GridPlugin;
use guide::{GuidePlugin, GuideVisualsPlugin};
use layout::LayoutPlugin;
use mqtt::MqttPlugin;
use mqtt_panel::MqttPanelPlugin;
use name::NamePlugin;
use name_panel::NamePanelPlugin;
use piece::PiecePlugin;
use reverser::ReverserVisualsPlugin;
use sensor::SensorVisualsPlugin;
use simulation::{SimulationControlsPlugin, SimulationPlugin, SimulationState};
use source::{SourcePlugin, SourceVisualsPlugin};
use trace::{Replay, TracePlugin, TraceVisualsPlugin};
use turner::TurnerVisualsPlugin;

pub const WIDTH: u32 = 1024;
pub const HEIGTH: u32 = 768;

/// Cadenza del passo headless. Senza finestra non c'e' un monitor a dare il
/// ritmo: senza questa attesa il ciclo girerebbe a vuoto al massimo della CPU.
const HEADLESS_STEP: Duration = Duration::from_micros(16_667);

fn main() {
    // clap si occupa anche di `--help` e degli errori sugli argomenti sbagliati.
    let options = Options::parse();

    let mut app = App::new();
    app.insert_resource(options.layout_file());
    // I parametri del broker esistono sempre, anche se non ci si collega: sono
    // il punto di partenza del pannello, che con la finestra puo' cambiarli.
    app.insert_resource(options.mqtt_settings());

    // La simulazione e' la stessa nei due casi: cambia solo chi la guarda.
    if options.hide_gui {
        app.add_plugins((
            MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(HEADLESS_STEP)),
            // Tre pezzi che stanno in DefaultPlugins ma non in MinimalPlugins:
            // gli stati (li usa la pausa), il log e il gestore di Ctrl+C, senza
            // il quale il processo verrebbe ucciso invece di uscire dal ciclo.
            StatesPlugin,
            LogPlugin::default(),
            TerminalCtrlCHandlerPlugin,
        ));
    } else {
        app.add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Carrier Flow".to_string(),
                resolution: (WIDTH, HEIGTH).into(),
                ..Default::default()
            }),
            ..Default::default()
        }))
        .add_plugins((
            CameraPlugin,
            GridPlugin,
            PiecePlugin,
            EditorPlugin,
            SimulationControlsPlugin,
            CarrierVisualsPlugin,
            SourceVisualsPlugin,
            GateVisualsPlugin,
            DivertVisualsPlugin,
            TraceVisualsPlugin,
            DespawnerVisualsPlugin,
            TurnerVisualsPlugin,
            ReverserVisualsPlugin,
            AntennaVisualsPlugin,
            SensorVisualsPlugin,
        ))
        // In due gruppi perche' un gruppo solo ha un tetto di quindici.
        .add_plugins((
            NamePlugin,
            NamePanelPlugin,
            GuideVisualsPlugin,
            MqttPanelPlugin,
        ));
    }

    // L'andatura si imposta sull'orologio virtuale, che c'e' in tutti e due i
    // casi: e' quello da cui `Res<Time>` prende il passo, quindi accelerarlo
    // accelera insieme carrier, sorgenti e registrazione senza che nessuno di
    // loro debba saperlo.
    simulation::set_speed(
        &mut app.world_mut().resource_mut::<Time<Virtual>>(),
        options.speed,
    );

    // Ci si collega a scena gia' costruita, come la registrazione: al primo
    // frame utile gli oggetti ci sono gia' e il loro stato parte subito.
    if options.mqtt {
        app.add_systems(
            PostStartup,
            (|mut commands: Commands, settings: Res<mqtt::MqttSettings>| {
                mqtt::connect(&mut commands, &settings);
            })
            .after(layout::load_layout_at_startup),
        );
    }

    // Registrazione e riproduzione partono a scena gia' costruita: il layout
    // nasce in PostStartup, e una riproduzione porta con se' il proprio.
    if options.record {
        app.add_systems(PostStartup, trace::start_recording);
    }
    // La riproduzione ha bisogno della finestra, e infatti la riga di comando
    // rifiuta la coppia con hide_gui: qui non ci si arriva mai senza interfaccia.
    if let Some(path) = options.replay.clone() {
        let start_replay =
            move |mut commands: Commands,
                  mut replay: ResMut<Replay>,
                  mut parked: ResMut<trace::ParkedLayout>,
                  pieces: Query<(
                Entity,
                &layout::Placed,
                &piece::Facing,
                Option<&name::PieceId>,
                Option<&name::PieceName>,
            )>,
                  mut next_state: ResMut<NextState<SimulationState>>| {
                trace::play_from_file(
                    &mut commands,
                    &mut replay,
                    &mut parked,
                    &pieces,
                    &mut next_state,
                    &path,
                );
            };

        // Dopo che il layout e' nato, non prima: la riproduzione deve trovare la
        // scena da mettere da parte e da sgombrare. Fra due sistemi dello stesso
        // schedule l'ordine altrimenti non e' garantito, e la cosa si vedeva
        // solo passando --layout e --replay insieme, che e' proprio il caso in
        // cui conta.
        app.add_systems(
            PostStartup,
            start_replay.after(layout::load_layout_at_startup),
        );
    }

    app.add_plugins((
        SimulationPlugin,
        LayoutPlugin,
        CarrierPlugin,
        SourcePlugin,
        DespawnerPlugin,
        // Anche senza finestra: registrare non ha bisogno di guardare, e i
        // sensori devono vedere passare i carrier comunque.
        TracePlugin,
        // Sensori, antenne e deviatori si accorgono dei carrier anche senza
        // finestra: quello stato servira' a mqtt, non solo al colore.
        EngagementPlugin,
        // Le guide sono muri, non disegno: senza questo plugin i carrier le
        // attraverserebbero in headless e l'impianto si comporterebbe in due
        // modi diversi a seconda che ci sia una finestra a guardarlo.
        GuidePlugin,
        // Il filo con il programma di comando: senza finestra e' l'unico modo
        // che l'impianto ha di ricevere ordini.
        MqttPlugin,
    ))
    .run();
}
