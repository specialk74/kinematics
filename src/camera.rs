use bevy::input::mouse::{MouseMotion, MouseScrollUnit, MouseWheel};
use bevy::prelude::*;

use crate::editor::{DraggedPiece, EditorTool, SelectedTool, pointer_over_ui};

/// Vista di partenza, quella a cui riporta il pulsante di reset.
const DEFAULT_ZOOM: f32 = 1.0;
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

/// Vero mentre e' in corso un trascinamento della vista. Serve ricordarlo:
/// altrimenti il clic che seleziona "Sposta" nella barra farebbe partire subito
/// una trascinata, visto che il tasto e' gia' premuto.
#[derive(Resource, Default)]
struct Panning(bool);

#[derive(Component)]
struct ResetViewButton;

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Panning>()
            .add_systems(Startup, (spawn_camera, setup_reset_button))
            .add_systems(Update, (zoom_with_ctrl_wheel, pan_view, reset_view));
    }
}

fn spawn_camera(mut commands: Commands) {
    commands.spawn(Camera2d);
}

fn setup_reset_button(mut commands: Commands) {
    commands.spawn((
        Button,
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(12.0),
            // Alla sinistra del play/pausa, che e' largo 90 e sta a 12 dal bordo.
            right: Val::Px(110.0),
            width: Val::Px(90.0),
            height: Val::Px(36.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(Color::srgb(0.20, 0.20, 0.24)),
        ResetViewButton,
        children![(
            Text::new("Reset vista"),
            TextFont {
                font_size: 14.0,
                ..default()
            },
            TextColor(Color::WHITE),
        )],
    ));
}

/// Riporta la vista a com'era all'avvio: zoom di partenza e layout centrato.
fn reset_view(
    buttons: Query<&Interaction, (Changed<Interaction>, With<ResetViewButton>)>,
    mut camera: Query<(&mut Projection, &mut Transform), With<Camera2d>>,
) {
    if !buttons
        .iter()
        .any(|interaction| *interaction == Interaction::Pressed)
    {
        return;
    }

    let Ok((mut projection, mut transform)) = camera.single_mut() else {
        return;
    };

    if let Projection::Orthographic(orthographic) = projection.as_mut() {
        orthographic.scale = DEFAULT_ZOOM;
    }

    // La z della camera decide cosa inquadra: si tocca solo il piano.
    transform.translation.x = 0.0;
    transform.translation.y = 0.0;
}

/// Trascinamento della vista col tasto sinistro, quando e' attivo il modo
/// "Sposta". La vista segue il mouse, quindi la camera va nel verso opposto.
fn pan_view(
    mouse: Res<ButtonInput<MouseButton>>,
    mut motion: MessageReader<MouseMotion>,
    selected: Res<SelectedTool>,
    dragged: Res<DraggedPiece>,
    ui_interactions: Query<&Interaction>,
    mut panning: ResMut<Panning>,
    mut camera: Query<(&Projection, &mut Transform), With<Camera2d>>,
) {
    let moved: Vec2 = motion.read().map(|motion| motion.delta).sum();

    if mouse.just_pressed(MouseButton::Left) {
        panning.0 = selected.0 == EditorTool::Pan && !pointer_over_ui(&ui_interactions);
    }
    if mouse.just_released(MouseButton::Left) {
        panning.0 = false;
    }

    // Se si sta trascinando un oggetto, e' lui a muoversi e non la vista.
    if !panning.0 || dragged.0.is_some() || moved == Vec2::ZERO {
        return;
    }

    let Ok((projection, mut transform)) = camera.single_mut() else {
        return;
    };
    let Projection::Orthographic(orthographic) = projection else {
        return;
    };

    // La y dello schermo cresce verso il basso, quella del mondo verso l'alto.
    let world = Vec2::new(-moved.x, moved.y) * orthographic.scale;
    transform.translation += world.extend(0.0);
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
