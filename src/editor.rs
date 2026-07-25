use bevy::prelude::*;

use crate::WORK_AREA_LEFT;
use crate::divert::{Divert, DivertAssets, DivertKind, spawn_divert, toggle_divert_at};
use crate::gate::{Gate, GateAssets, spawn_gate, toggle_gate_at};
use crate::source::{SourceAssets, spawn_source};

pub const PALETTE_WIDTH: f32 = 120.0;

const BUTTON_IDLE: Color = Color::srgb(0.20, 0.20, 0.24);
const BUTTON_SELECTED: Color = Color::srgb(0.25, 0.45, 0.80);

/// Gli oggetti che si possono piazzare nella scena.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    CarrierSource,
    Gate,
    Divert,
    Atr,
}

/// Ordine dei bottoni nella barra.
const TOOLS: [Tool; 4] = [Tool::CarrierSource, Tool::Gate, Tool::Divert, Tool::Atr];

impl Tool {
    fn label(self) -> &'static str {
        match self {
            Tool::CarrierSource => "Sorgente",
            Tool::Gate => "Gate",
            Tool::Divert => "Divert",
            Tool::Atr => "ATR",
        }
    }
}

/// Strumento attivo: il prossimo clic nella scena piazza questo oggetto.
#[derive(Resource)]
pub struct SelectedTool(pub Tool);

impl Default for SelectedTool {
    fn default() -> Self {
        SelectedTool(Tool::CarrierSource)
    }
}

#[derive(Component)]
struct ToolButton(Tool);

pub struct EditorPlugin;

impl Plugin for EditorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SelectedTool>()
            .add_systems(Startup, setup_palette)
            .add_systems(
                Update,
                (select_tool, highlight_selected_tool, place_selected_tool),
            );
    }
}

fn setup_palette(mut commands: Commands) {
    commands
        .spawn((
            Node {
                width: Val::Px(PALETTE_WIDTH),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(8.0)),
                row_gap: Val::Px(8.0),
                ..default()
            },
            BackgroundColor(Color::srgb(0.10, 0.10, 0.12)),
        ))
        .with_children(|palette| {
            for tool in TOOLS {
                palette.spawn((
                    Button,
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Px(40.0),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BackgroundColor(BUTTON_IDLE),
                    ToolButton(tool),
                    children![(
                        Text::new(tool.label()),
                        TextFont {
                            font_size: 14.0,
                            ..default()
                        },
                        TextColor(Color::WHITE),
                    )],
                ));
            }
        });
}

fn select_tool(
    buttons: Query<(&Interaction, &ToolButton), Changed<Interaction>>,
    mut selected: ResMut<SelectedTool>,
) {
    for (interaction, button) in buttons.iter() {
        if *interaction == Interaction::Pressed {
            selected.0 = button.0;
        }
    }
}

fn highlight_selected_tool(
    selected: Res<SelectedTool>,
    mut buttons: Query<(&ToolButton, &mut BackgroundColor)>,
) {
    if !selected.is_changed() {
        return;
    }

    for (button, mut background) in buttons.iter_mut() {
        background.0 = if button.0 == selected.0 {
            BUTTON_SELECTED
        } else {
            BUTTON_IDLE
        };
    }
}

/// Clic nell'area di lavoro: piazza lo strumento attivo. Fanno eccezione i clic
/// su un gate o su un divert gia' esistenti, che ne commutano lo stato invece di
/// sovrapporne un altro: e' l'unico modo per accenderli e spegnerli.
fn place_selected_tool(
    mut commands: Commands,
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform)>,
    mut gates: Query<(&mut Gate, &Transform, &mut MeshMaterial2d<ColorMaterial>), Without<Divert>>,
    mut diverts: Query<
        (&mut Divert, &Transform, &mut MeshMaterial2d<ColorMaterial>),
        Without<Gate>,
    >,
    gate_assets: Res<GateAssets>,
    divert_assets: Res<DivertAssets>,
    source_assets: Res<SourceAssets>,
    selected: Res<SelectedTool>,
) {
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }

    let Ok(window) = windows.single() else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    let Ok((camera, camera_transform)) = camera_query.single() else {
        return;
    };
    let Ok(position) = camera.viewport_to_world_2d(camera_transform, cursor) else {
        return;
    };

    // Il clic sulla barra e' gia' gestito dai bottoni: qui si lavora solo sulla scena.
    if position.x < WORK_AREA_LEFT {
        return;
    }

    if toggle_gate_at(position, &mut gates, &gate_assets)
        || toggle_divert_at(position, &mut diverts, &divert_assets)
    {
        return;
    }

    let position = position.extend(1.0);
    match selected.0 {
        Tool::CarrierSource => spawn_source(&mut commands, &source_assets, position),
        Tool::Gate => spawn_gate(&mut commands, &gate_assets, position),
        Tool::Divert => spawn_divert(&mut commands, &divert_assets, position, DivertKind::Divert),
        Tool::Atr => spawn_divert(&mut commands, &divert_assets, position, DivertKind::Atr),
    }
}
