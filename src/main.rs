use std::time::Duration;

use bevy::app::{ScheduleRunnerPlugin, TerminalCtrlCHandlerPlugin};
use bevy::log::LogPlugin;
use bevy::prelude::*;
use bevy::state::app::StatesPlugin;
use clap::Parser;

mod carrier;
mod cli;
mod divert;
mod editor;
mod gate;
mod grid;
mod layout;
mod simulation;
mod source;

use carrier::{CarrierPlugin, CarrierVisualsPlugin};
use cli::Options;
use divert::DivertVisualsPlugin;
use editor::{EditorPlugin, PALETTE_WIDTH};
use gate::GateVisualsPlugin;
use grid::GridPlugin;
use layout::LayoutPlugin;
use simulation::{SimulationControlsPlugin, SimulationPlugin};
use source::{SourcePlugin, SourceVisualsPlugin};

pub const WIDTH: u32 = 1024;
pub const HEIGTH: u32 = 768;
/// Confini dell'area di lavoro in coordinate mondo. A sinistra si ferma dove
/// inizia la barra degli strumenti; sugli altri lati coincide con la finestra.
/// Sono costanti perche' la simulazione deve poterli usare anche senza camera.
pub const WORK_AREA_LEFT: f32 = -(WIDTH as f32) / 2.0 + PALETTE_WIDTH;
pub const WORK_AREA_RIGHT: f32 = WIDTH as f32 / 2.0;
pub const WORK_AREA_TOP: f32 = HEIGTH as f32 / 2.0;
pub const WORK_AREA_BOTTOM: f32 = -(HEIGTH as f32) / 2.0;

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
            GridPlugin,
            EditorPlugin,
            SimulationControlsPlugin,
            CarrierVisualsPlugin,
            SourceVisualsPlugin,
            GateVisualsPlugin,
            DivertVisualsPlugin,
        ))
        .add_systems(Startup, setup_camera);
    }

    app.add_plugins((SimulationPlugin, LayoutPlugin, CarrierPlugin, SourcePlugin))
        .run();
}

fn setup_camera(mut commands: Commands) {
    commands.spawn(Camera2d);
}
