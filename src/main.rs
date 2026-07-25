use std::time::Duration;

use bevy::app::{ScheduleRunnerPlugin, TerminalCtrlCHandlerPlugin};
use bevy::log::LogPlugin;
use bevy::prelude::*;
use bevy::state::app::StatesPlugin;
use clap::Parser;

mod camera;
mod carrier;
mod cli;
mod despawner;
mod divert;
mod editor;
mod gate;
mod geometry;
mod grid;
mod layout;
mod piece;
mod reverser;
mod simulation;
mod source;
mod turner;

use camera::CameraPlugin;
use carrier::{CarrierPlugin, CarrierVisualsPlugin};
use cli::Options;
use despawner::{DespawnerPlugin, DespawnerVisualsPlugin};
use divert::DivertVisualsPlugin;
use editor::EditorPlugin;
use gate::GateVisualsPlugin;
use grid::GridPlugin;
use layout::LayoutPlugin;
use piece::PiecePlugin;
use reverser::ReverserVisualsPlugin;
use simulation::{SimulationControlsPlugin, SimulationPlugin};
use source::{SourcePlugin, SourceVisualsPlugin};
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
            DespawnerVisualsPlugin,
            TurnerVisualsPlugin,
            ReverserVisualsPlugin,
        ));
    }

    app.add_plugins((
        SimulationPlugin,
        LayoutPlugin,
        CarrierPlugin,
        SourcePlugin,
        DespawnerPlugin,
    ))
    .run();
}
