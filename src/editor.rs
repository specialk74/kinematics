use bevy::prelude::*;
use bevy::sprite_render::AlphaMode2d;

use crate::WORK_AREA_LEFT;
use crate::divert::{self, Divert, DivertAssets, DivertKind, spawn_divert, toggle_divert};
use crate::gate::{self, Gate, GateAssets, spawn_gate, toggle_gate};
use crate::grid;
use crate::source::{self, SourceAssets, spawn_source};

pub const PALETTE_WIDTH: f32 = 120.0;

const BUTTON_IDLE: Color = Color::srgb(0.20, 0.20, 0.24);
const BUTTON_SELECTED: Color = Color::srgb(0.25, 0.45, 0.80);
/// Davanti a tutto: l'anteprima deve restare leggibile anche sopra un oggetto
/// gia' piazzato, che e' proprio il caso in cui serve di piu'.
const GHOST_Z: f32 = 2.0;

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

/// Oggetto appoggiato sulla griglia. Tiene la cella e lo strumento che l'ha
/// creato: bastano a sapere cosa c'e' in una cella senza interrogare i singoli
/// moduli, e a decidere se un clic sostituisce o commuta.
#[derive(Component)]
struct Placed {
    tool: Tool,
    cell: IVec2,
}

/// Sagoma semitrasparente che mostra dove finirebbe l'oggetto se si cliccasse ora.
#[derive(Component)]
struct Ghost;

#[derive(Resource)]
struct GhostMaterial(Handle<ColorMaterial>);

/// Cella della griglia sotto il mouse, se il mouse e' sull'area di lavoro.
/// La usano sia l'anteprima sia il piazzamento: e' cosi' che l'oggetto finisce
/// per forza dove l'anteprima l'aveva mostrato.
fn cursor_cell(
    windows: &Query<&Window>,
    camera_query: &Query<(&Camera, &GlobalTransform)>,
) -> Option<IVec2> {
    let window = windows.single().ok()?;
    let cursor = window.cursor_position()?;
    let (camera, camera_transform) = camera_query.single().ok()?;
    let position = camera.viewport_to_world_2d(camera_transform, cursor).ok()?;

    // Sulla barra non si piazza niente: quei clic sono dei bottoni.
    (position.x >= WORK_AREA_LEFT).then(|| grid::cell(position))
}

fn tool_shape(
    tool: Tool,
    source_assets: &SourceAssets,
    gate_assets: &GateAssets,
    divert_assets: &DivertAssets,
) -> (Handle<Mesh>, Quat) {
    match tool {
        Tool::CarrierSource => source::shape(source_assets),
        Tool::Gate => gate::shape(gate_assets),
        Tool::Divert => divert::shape(divert_assets, DivertKind::Divert),
        Tool::Atr => divert::shape(divert_assets, DivertKind::Atr),
    }
}

pub struct EditorPlugin;

impl Plugin for EditorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SelectedTool>()
            .add_systems(Startup, (setup_palette, setup_ghost_material))
            .add_systems(
                Update,
                (
                    select_tool,
                    highlight_selected_tool,
                    update_ghost,
                    place_selected_tool,
                ),
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

fn setup_ghost_material(mut commands: Commands, mut materials: ResMut<Assets<ColorMaterial>>) {
    commands.insert_resource(GhostMaterial(materials.add(ColorMaterial {
        color: Color::srgba(1.0, 1.0, 1.0, 0.35),
        alpha_mode: AlphaMode2d::Blend,
        ..default()
    })));
}

/// Tiene l'anteprima agganciata alla cella sotto il mouse e le da' la forma
/// dello strumento selezionato. Sparisce quando il mouse esce dall'area di lavoro.
fn update_ghost(
    mut commands: Commands,
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform)>,
    selected: Res<SelectedTool>,
    ghost_material: Res<GhostMaterial>,
    source_assets: Res<SourceAssets>,
    gate_assets: Res<GateAssets>,
    divert_assets: Res<DivertAssets>,
    mut ghost: Query<(&mut Transform, &mut Visibility, &mut Mesh2d), With<Ghost>>,
) {
    let Some(cell) = cursor_cell(&windows, &camera_query) else {
        if let Ok((_, mut visibility, _)) = ghost.single_mut() {
            *visibility = Visibility::Hidden;
        }
        return;
    };

    let (mesh, rotation) = tool_shape(selected.0, &source_assets, &gate_assets, &divert_assets);
    let transform = Transform::from_translation(grid::cell_center(cell).extend(GHOST_Z))
        .with_rotation(rotation);

    match ghost.single_mut() {
        Ok((mut ghost_transform, mut visibility, mut ghost_mesh)) => {
            *ghost_transform = transform;
            *visibility = Visibility::Visible;
            if ghost_mesh.0 != mesh {
                ghost_mesh.0 = mesh;
            }
        }
        // Nasce al primo frame utile: negli Startup l'ordine fra i setup degli
        // asset non e' garantito, qui invece ci sono di sicuro.
        Err(_) => {
            commands.spawn((
                Mesh2d(mesh),
                MeshMaterial2d(ghost_material.0.clone()),
                transform,
                Ghost,
            ));
        }
    }
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

/// Clic nell'area di lavoro: l'oggetto viene appoggiato al centro della cella
/// puntata. Se la cella e' gia' occupata il nuovo oggetto prende il posto del
/// vecchio, tranne quando lo strumento e' lo stesso: in quel caso il clic serve
/// ad accendere e spegnere quello che c'e' gia'.
fn place_selected_tool(
    mut commands: Commands,
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform)>,
    placed: Query<(Entity, &Placed)>,
    mut gates: Query<(&mut Gate, &mut MeshMaterial2d<ColorMaterial>), Without<Divert>>,
    mut diverts: Query<(&mut Divert, &mut MeshMaterial2d<ColorMaterial>), Without<Gate>>,
    gate_assets: Res<GateAssets>,
    divert_assets: Res<DivertAssets>,
    source_assets: Res<SourceAssets>,
    selected: Res<SelectedTool>,
) {
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }

    let Some(cell) = cursor_cell(&windows, &camera_query) else {
        return;
    };

    let tool = selected.0;

    if let Some((entity, occupant)) = placed
        .iter()
        .find(|(_, occupant)| occupant.cell == cell)
        .map(|(entity, occupant)| (entity, occupant.tool))
    {
        if occupant == tool {
            if let Ok((mut gate, mut material)) = gates.get_mut(entity) {
                toggle_gate(&mut gate, &mut material, &gate_assets);
            } else if let Ok((mut divert, mut material)) = diverts.get_mut(entity) {
                toggle_divert(&mut divert, &mut material, &divert_assets);
            }
            return;
        }

        commands.entity(entity).despawn();
    }

    let position = grid::cell_center(cell).extend(1.0);
    let object = match tool {
        Tool::CarrierSource => spawn_source(&mut commands, &source_assets, position),
        Tool::Gate => spawn_gate(&mut commands, &gate_assets, position),
        Tool::Divert => spawn_divert(&mut commands, &divert_assets, position, DivertKind::Divert),
        Tool::Atr => spawn_divert(&mut commands, &divert_assets, position, DivertKind::Atr),
    };

    commands.entity(object).insert(Placed { tool, cell });
}
