use bevy::prelude::*;

mod carrier;
mod divert;
mod editor;
mod gate;
mod grid;
mod layout;
mod simulation;
mod source;

use carrier::CarrierPlugin;
use divert::DivertPlugin;
use editor::{EditorPlugin, PALETTE_WIDTH};
use gate::GatePlugin;
use grid::GridPlugin;
use layout::LayoutFile;
use simulation::SimulationPlugin;
use source::SourcePlugin;

pub const WIDTH: u32 = 1024;
pub const HEIGTH: u32 = 768;
/// Bordo sinistro dell'area di lavoro in coordinate mondo: piu' a sinistra c'e'
/// la barra degli strumenti, dove non si piazza nulla e i carrier spariscono.
pub const WORK_AREA_LEFT: f32 = -(WIDTH as f32) / 2.0 + PALETTE_WIDTH;

fn main() {
    App::new()
        // Primo argomento: il file di layout da aprire. Senza, si parte vuoti.
        .insert_resource(LayoutFile::from_args(std::env::args().skip(1)))
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Carrier Flow".to_string(),
                resolution: (WIDTH, HEIGTH).into(),
                ..Default::default()
            }),
            ..Default::default()
        }))
        .add_plugins((
            SimulationPlugin,
            GridPlugin,
            CarrierPlugin,
            SourcePlugin,
            GatePlugin,
            DivertPlugin,
            EditorPlugin,
        ))
        .add_systems(Startup, setup_camera)
        .run();
}

fn setup_camera(mut commands: Commands) {
    commands.spawn(Camera2d);
}
