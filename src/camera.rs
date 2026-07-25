use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;

/// Quanto stringe o allarga un singolo scatto di rotella.
const ZOOM_STEP: f32 = 1.15;
/// La scala e' quanto mondo entra in un pixel: piu' e' piccola, piu' si e'
/// vicini. A 0.05 un carrier riempie mezzo schermo, a 2.0 si vedono quattro
/// schermate di nastro.
const MIN_ZOOM: f32 = 0.05;
const MAX_ZOOM: f32 = 2.0;
/// Una rotella a scatti manda +-1, un trackpad manda decine di pixel: senza
/// questo fattore lo zoom da trackpad sarebbe ingestibile.
const PIXELS_PER_STEP: f32 = 50.0;

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_camera)
            .add_systems(Update, zoom_with_ctrl_wheel);
    }
}

fn spawn_camera(mut commands: Commands) {
    commands.spawn(Camera2d);
}

/// Ctrl + rotella. Lo zoom e' centrato sul puntatore: il punto del mondo che
/// stai indicando resta fermo sotto il mouse, cosi' si raggiunge qualsiasi zona
/// senza bisogno di spostare la vista.
fn zoom_with_ctrl_wheel(
    mut wheel: MessageReader<MouseWheel>,
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window>,
    mut camera: Query<(&mut Projection, &mut Transform), With<Camera2d>>,
) {
    let steps: f32 = wheel
        .read()
        .map(|scroll| match scroll.unit {
            MouseScrollUnit::Line => scroll.y,
            MouseScrollUnit::Pixel => scroll.y / PIXELS_PER_STEP,
        })
        .sum();

    if steps == 0.0 || !keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]) {
        return;
    }

    let Ok(window) = windows.single() else {
        return;
    };
    let Ok((mut projection, mut transform)) = camera.single_mut() else {
        return;
    };
    let Projection::Orthographic(orthographic) = projection.as_mut() else {
        return;
    };

    let before = orthographic.scale;
    orthographic.scale = (before / ZOOM_STEP.powf(steps)).clamp(MIN_ZOOM, MAX_ZOOM);

    // Il punto sotto il mouse si sposterebbe insieme alla scala: si compensa
    // muovendo la camera di quanto quel punto si e' spostato.
    if let Some(cursor) = window.cursor_position() {
        let from_centre = cursor - Vec2::new(window.width(), window.height()) / 2.0;
        // La y dello schermo cresce verso il basso, quella del mondo verso l'alto.
        let direction = Vec2::new(from_centre.x, -from_centre.y);

        transform.translation += (direction * (before - orthographic.scale)).extend(0.0);
    }
}
