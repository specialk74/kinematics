use bevy::prelude::*;
use bevy::sprite_render::AlphaMode2d;
use serde::{Deserialize, Serialize};

use crate::WORK_AREA_LEFT;
use crate::carrier::Carrier;
use crate::divert::{self, Divert, DivertAssets, DivertKind};
use crate::gate::{self, Gate, GateAssets};
use crate::grid;
use crate::layout::{self, Layout, LayoutFile, LayoutObject, Placed, place_in_cell, spawn_layout};
use crate::source::{self, SourceAssets};

pub const PALETTE_WIDTH: f32 = 120.0;

const BUTTON_IDLE: Color = Color::srgb(0.20, 0.20, 0.24);
const BUTTON_SELECTED: Color = Color::srgb(0.25, 0.45, 0.80);
const CAPTION_COLOR: Color = Color::srgb(0.55, 0.55, 0.62);
/// Davanti a tutto: l'anteprima deve restare leggibile anche sopra un oggetto
/// gia' piazzato, che e' proprio il caso in cui serve di piu'.
const GHOST_Z: f32 = 2.0;

/// Gli oggetti che si possono piazzare nella scena. E' anche il vocabolario del
/// file di layout, quindi rinominare una variante invalida i file gia' salvati.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
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

/// I due comandi sul file di layout, in fondo alla barra.
#[derive(Clone, Copy, PartialEq, Eq)]
enum LayoutAction {
    Save,
    Load,
}

impl LayoutAction {
    fn label(self) -> &'static str {
        match self {
            LayoutAction::Save => "Salva",
            LayoutAction::Load => "Carica",
        }
    }
}

#[derive(Component)]
struct LayoutButton(LayoutAction);

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
    ui_interactions: &Query<&Interaction>,
) -> Option<IVec2> {
    // I bottoni galleggiano sopra la scena (play/pausa in alto a destra), quindi
    // non basta escludere la barra: se il mouse e' su un bottone quel clic e' suo.
    if ui_interactions
        .iter()
        .any(|interaction| *interaction != Interaction::None)
    {
        return None;
    }

    let window = windows.single().ok()?;
    let cursor = window.cursor_position()?;
    let (camera, camera_transform) = camera_query.single().ok()?;
    let position = camera.viewport_to_world_2d(camera_transform, cursor).ok()?;

    // Sulla barra degli strumenti non si piazza niente.
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
                    handle_layout_buttons,
                ),
            );
    }
}

fn setup_palette(mut commands: Commands, layout_file: Res<LayoutFile>) {
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
                    button_node(),
                    BackgroundColor(BUTTON_IDLE),
                    ToolButton(tool),
                    children![button_label(tool.label())],
                ));
            }

            // Spinge i comandi sul file in fondo, staccati dagli strumenti.
            palette.spawn(Node {
                flex_grow: 1.0,
                ..default()
            });

            palette.spawn((
                Text::new("File"),
                TextFont {
                    font_size: 10.0,
                    ..default()
                },
                TextColor(CAPTION_COLOR),
            ));
            palette.spawn((
                Text::new(layout_file.display_name()),
                TextFont {
                    font_size: 12.0,
                    ..default()
                },
                TextColor(Color::WHITE),
                Node {
                    margin: UiRect::bottom(Val::Px(4.0)),
                    ..default()
                },
            ));

            for action in [LayoutAction::Save, LayoutAction::Load] {
                palette.spawn((
                    button_node(),
                    BackgroundColor(BUTTON_IDLE),
                    LayoutButton(action),
                    children![button_label(action.label())],
                ));
            }
        });
}

fn button_node() -> (Button, Node) {
    (
        Button,
        Node {
            width: Val::Percent(100.0),
            height: Val::Px(40.0),
            flex_shrink: 0.0,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
    )
}

fn button_label(text: &str) -> (Text, TextFont, TextColor) {
    (
        Text::new(text),
        TextFont {
            font_size: 14.0,
            ..default()
        },
        TextColor(Color::WHITE),
    )
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
    ui_interactions: Query<&Interaction>,
    mut ghost: Query<(&mut Transform, &mut Visibility, &mut Mesh2d), With<Ghost>>,
) {
    let Some(cell) = cursor_cell(&windows, &camera_query, &ui_interactions) else {
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
    ui_interactions: Query<&Interaction>,
    mut gates: Query<&mut Gate>,
    mut diverts: Query<&mut Divert>,
    selected: Res<SelectedTool>,
) {
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }

    let Some(cell) = cursor_cell(&windows, &camera_query, &ui_interactions) else {
        return;
    };

    let tool = selected.0;

    if let Some((entity, occupant)) = placed
        .iter()
        .find(|(_, occupant)| occupant.cell == cell)
        .map(|(entity, occupant)| (entity, occupant.tool))
    {
        // Stesso strumento: si accende o si spegne quello che c'e'. Il colore lo
        // aggiorna il modulo dell'oggetto guardando lo stato.
        if occupant == tool {
            if let Ok(mut gate) = gates.get_mut(entity) {
                gate.active = !gate.active;
            } else if let Ok(mut divert) = diverts.get_mut(entity) {
                divert.active = !divert.active;
            }
            return;
        }

        commands.entity(entity).despawn();
    }

    place_in_cell(&mut commands, tool, cell);
}

/// I due bottoni sul file di layout. Il salvataggio raccoglie quello che c'e' in
/// scena; il caricamento la sostituisce, carrier in volo compresi: lasciarli
/// vivi vorrebbe dire vederli percorrere corsie che non esistono piu'.
fn handle_layout_buttons(
    mut commands: Commands,
    buttons: Query<(&Interaction, &LayoutButton), Changed<Interaction>>,
    placed: Query<(Entity, &Placed)>,
    carriers: Query<Entity, With<Carrier>>,
    layout_file: Res<LayoutFile>,
) {
    for (interaction, button) in buttons.iter() {
        if *interaction != Interaction::Pressed {
            continue;
        }

        match button.0 {
            LayoutAction::Save => {
                let layout = Layout {
                    objects: placed
                        .iter()
                        .map(|(_, placed)| LayoutObject {
                            tool: placed.tool,
                            cell: (placed.cell.x, placed.cell.y),
                        })
                        .collect(),
                };

                match layout::save(&layout, &layout_file.path) {
                    Ok(()) => info!("layout salvato in {}", layout_file.path),
                    Err(error) => error!("salvataggio fallito: {error}"),
                }
            }

            LayoutAction::Load => match layout::load(&layout_file.path) {
                Ok(layout) => {
                    for (entity, _) in placed.iter() {
                        commands.entity(entity).despawn();
                    }
                    for entity in carriers.iter() {
                        commands.entity(entity).despawn();
                    }

                    spawn_layout(&mut commands, &layout);
                }
                Err(error) => error!("caricamento fallito: {error}"),
            },
        }
    }
}
