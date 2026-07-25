use bevy::prelude::*;

mod carrier;
mod divert;
mod gate;

use carrier::CarrierPlugin;
use divert::DivertPlugin;
use gate::GatePlugin;

pub const WIDTH: u32 = 1024;
pub const HEIGTH: u32 = 768;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Carrier Flow".to_string(),
                resolution: (WIDTH, HEIGTH).into(),
                ..Default::default()
            }),
            ..Default::default()
        }))
        .add_plugins((CarrierPlugin, DivertPlugin, GatePlugin))
        .add_systems(Startup, setup_camera)
        .run();
}

fn setup_camera(mut commands: Commands) {
    commands.spawn(Camera2d);
}
