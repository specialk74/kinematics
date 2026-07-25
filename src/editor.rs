use bevy::prelude::*;
use bevy::sprite_render::AlphaMode2d;
use serde::{Deserialize, Serialize};

use crate::carrier::Carrier;
use crate::grid;
use crate::layout::{self, LayoutFile, Placed, Switches, place_in_cell, spawn_layout};
use crate::piece::{self, Facing, PieceShapes};

pub const PALETTE_WIDTH: f32 = 120.0;

pub const BUTTON_IDLE: Color = Color::srgb(0.20, 0.20, 0.24);
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
    Despawner,
    /// Svolta a destra rispetto alla marcia del carrier. Il nome serializzato
    /// resta quello di prima per non invalidare i layout gia' salvati.
    #[serde(alias = "Riser")]
    Turner,
    Reverser,
}

impl Tool {
    fn label(self) -> &'static str {
        match self {
            Tool::CarrierSource => "Sorgente",
            Tool::Gate => "Gate",
            Tool::Divert => "Divert",
            Tool::Atr => "ATR",
            Tool::Despawner => "Despawn",
            Tool::Turner => "Svolta",
            Tool::Reverser => "Inversione",
        }
    }
}

/// Cosa fa il prossimo clic nella scena. `Pan` non e' un oggetto piazzabile, per
/// questo sta qui e non in `Tool`: quell'enum e' il vocabolario del file di
/// layout, e nel file non puo' finire qualcosa che non e' un oggetto.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EditorTool {
    /// Nessun oggetto: il trascinamento sposta la vista.
    Pan,
    /// Il clic rimuove l'oggetto su cui cade. I carrier non si toccano: sono
    /// merce di passaggio, non pezzi del layout.
    Erase,
    Place(Tool),
}

impl EditorTool {
    fn label(self) -> &'static str {
        match self {
            EditorTool::Pan => "Sposta",
            EditorTool::Erase => "Cancella",
            EditorTool::Place(tool) => tool.label(),
        }
    }
}

/// Ordine dei bottoni nella barra.
const MODES: [EditorTool; 9] = [
    EditorTool::Pan,
    EditorTool::Erase,
    EditorTool::Place(Tool::CarrierSource),
    EditorTool::Place(Tool::Gate),
    EditorTool::Place(Tool::Divert),
    EditorTool::Place(Tool::Atr),
    EditorTool::Place(Tool::Despawner),
    EditorTool::Place(Tool::Turner),
    EditorTool::Place(Tool::Reverser),
];

/// Modo attivo.
#[derive(Resource)]
pub struct SelectedTool(pub EditorTool);

impl Default for SelectedTool {
    fn default() -> Self {
        SelectedTool(EditorTool::Place(Tool::CarrierSource))
    }
}

#[derive(Component)]
struct ToolButton(EditorTool);

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

/// L'etichetta dentro al bottone, per poterla riscrivere dopo un salvataggio.
#[derive(Component)]
struct LayoutButtonLabel(LayoutAction);

/// Per quanto tempo il bottone Salva mostra com'e' andata.
const NOTICE_SECONDS: f32 = 2.0;
const BUTTON_DONE: Color = Color::srgb(0.15, 0.55, 0.25);
const BUTTON_FAILED: Color = Color::srgb(0.70, 0.15, 0.15);

/// Esito dell'ultimo salvataggio, finche' va mostrato. Senza, l'unico segnale
/// era una riga di log che l'utente non ha davanti agli occhi.
#[derive(Resource, Default)]
struct SaveNotice(Option<(bool, Timer)>);

/// Spostamento sotto il quale una pressione conta come clic e non come
/// trascinata: serve a non commutare un oggetto quando ci si appoggia sopra per
/// spostare la vista.
const CLICK_SLOP: f32 = 4.0;

/// Dove si trovava il puntatore quando e' stato premuto il tasto.
#[derive(Resource, Default)]
struct PressOrigin(Option<Vec2>);

/// Oggetto che si sta trascinando. Lo legge anche la camera: mentre si sposta
/// un oggetto la vista deve restare ferma.
#[derive(Resource, Default)]
pub struct DraggedPiece(pub Option<Entity>);

/// Sagoma semitrasparente che mostra dove finirebbe l'oggetto se si cliccasse ora.
#[derive(Component)]
struct Ghost;

#[derive(Resource)]
struct GhostMaterial(Handle<ColorMaterial>);

/// Vero se il puntatore e' su un elemento dell'interfaccia. I bottoni
/// galleggiano sopra la scena (play/pausa e reset in alto a destra), quindi non
/// basta escludere la barra: se il mouse e' su un bottone, quel clic e' suo.
pub fn pointer_over_ui(ui_interactions: &Query<&Interaction>) -> bool {
    ui_interactions
        .iter()
        .any(|interaction| *interaction != Interaction::None)
}

/// Cella della griglia sotto il mouse, se il mouse e' sull'area di lavoro.
/// La usano sia l'anteprima sia il piazzamento: e' cosi' che l'oggetto finisce
/// per forza dove l'anteprima l'aveva mostrato.
fn cursor_cell(
    windows: &Query<&Window>,
    camera_query: &Query<(&Camera, &GlobalTransform)>,
    ui_interactions: &Query<&Interaction>,
) -> Option<IVec2> {
    if pointer_over_ui(ui_interactions) {
        return None;
    }

    let window = windows.single().ok()?;
    let cursor = window.cursor_position()?;

    // Sulla barra degli strumenti non si piazza niente. Il confronto e' sullo
    // schermo e non sul mondo: con lo zoom e lo spostamento della vista la barra
    // copre ogni volta una porzione diversa di mondo, quindi un confine in
    // coordinate mondo smetterebbe di corrispondere alla barra che si vede.
    if cursor.x < PALETTE_WIDTH {
        return None;
    }

    let (camera, camera_transform) = camera_query.single().ok()?;
    let position = camera.viewport_to_world_2d(camera_transform, cursor).ok()?;

    Some(grid::cell(position))
}

pub struct EditorPlugin;

impl Plugin for EditorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SelectedTool>()
            .init_resource::<PressOrigin>()
            .init_resource::<SaveNotice>()
            .init_resource::<DraggedPiece>()
            .add_systems(Startup, (setup_palette, setup_ghost_material))
            .add_systems(
                Update,
                (
                    select_tool,
                    highlight_selected_tool,
                    update_ghost,
                    place_selected_tool,
                    toggle_by_click,
                    rotate_piece,
                    drag_piece,
                    handle_layout_buttons,
                    show_save_outcome,
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
            for mode in MODES {
                palette.spawn((
                    button_node(),
                    BackgroundColor(BUTTON_IDLE),
                    ToolButton(mode),
                    children![button_label(mode.label())],
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
                    children![(button_label(action.label()), LayoutButtonLabel(action))],
                ));
            }
        });
}

/// Posto in fila in alto a destra. Le posizioni stanno qui e non sparse nei
/// moduli: cosi' aggiungere un bottone non ne sovrappone un altro.
pub fn top_button(slot: u32) -> (Button, Node) {
    const WIDTH: f32 = 96.0;
    const GAP: f32 = 8.0;

    (
        Button,
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(12.0),
            right: Val::Px(12.0 + slot as f32 * (WIDTH + GAP)),
            width: Val::Px(WIDTH),
            height: Val::Px(36.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
    )
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

pub fn button_label(text: &str) -> (Text, TextFont, TextColor) {
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
    shapes: Res<PieceShapes>,
    ui_interactions: Query<&Interaction>,
    mut ghost: Query<(&mut Transform, &mut Visibility, &mut Mesh2d), With<Ghost>>,
) {
    // In modo "Sposta" non si piazza niente, quindi non c'e' niente da mostrare.
    let target = match (
        selected.0,
        cursor_cell(&windows, &camera_query, &ui_interactions),
    ) {
        (EditorTool::Place(tool), Some(cell)) => Some((tool, cell)),
        _ => None,
    };

    let Some((_, cell)) = target else {
        if let Ok((_, mut visibility, _)) = ghost.single_mut() {
            *visibility = Visibility::Hidden;
        }
        return;
    };

    // Tutti gli oggetti sono quadrati uguali: l'anteprima mostra dove finira'
    // il prossimo, non che aspetto avra'.
    let mesh = piece::square(&shapes);
    let transform = Transform::from_translation(grid::cell_center(cell).extend(GHOST_Z));

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
    mut switches: Switches,
    selected: Res<SelectedTool>,
) {
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }

    let Some(cell) = cursor_cell(&windows, &camera_query, &ui_interactions) else {
        return;
    };

    let occupant = placed
        .iter()
        .find(|(_, occupant)| occupant.cell == cell)
        .map(|(entity, occupant)| (entity, occupant.tool));

    // In modo "Sposta" il tasto sinistro trascina la vista; in modo "Cancella"
    // toglie di mezzo l'oggetto puntato. In nessuno dei due si piazza qualcosa.
    let tool = match selected.0 {
        EditorTool::Pan => return,
        EditorTool::Erase => {
            if let Some((entity, _)) = occupant {
                commands.entity(entity).despawn();
            }
            return;
        }
        EditorTool::Place(tool) => tool,
    };

    if let Some((entity, occupant)) = occupant {
        // Stesso strumento: si accende o si spegne quello che c'e'. Il colore lo
        // aggiorna il modulo dell'oggetto guardando lo stato.
        if occupant == tool {
            switches.toggle(entity);
            return;
        }

        commands.entity(entity).despawn();
    }

    place_in_cell(&mut commands, tool, cell, Facing::default());
}

/// In modo "Sposta" il tasto sinistro trascina la vista, ma una pressione che
/// non si sposta e' un clic: serve ad accendere e spegnere gli oggetti senza
/// dover tornare allo strumento con cui erano stati piazzati.
fn toggle_by_click(
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform)>,
    ui_interactions: Query<&Interaction>,
    selected: Res<SelectedTool>,
    placed: Query<(Entity, &Placed)>,
    mut switches: Switches,
    mut press: ResMut<PressOrigin>,
) {
    let cursor = windows
        .single()
        .ok()
        .and_then(|window| window.cursor_position());

    if mouse.just_pressed(MouseButton::Left) {
        press.0 = cursor;
    }

    if !mouse.just_released(MouseButton::Left) {
        return;
    }

    let Some(origin) = press.0.take() else {
        return;
    };
    if selected.0 != EditorTool::Pan {
        return;
    }

    // Se il puntatore si e' mosso, quella era una trascinata della vista.
    let Some(cursor) = cursor else {
        return;
    };
    if origin.distance(cursor) > CLICK_SLOP {
        return;
    }

    let Some(cell) = cursor_cell(&windows, &camera_query, &ui_interactions) else {
        return;
    };
    let Some((entity, _)) = placed.iter().find(|(_, placed)| placed.cell == cell) else {
        return;
    };

    switches.toggle(entity);
}

/// Mostra sul bottone Salva com'e' andata, e dopo qualche secondo lo rimette
/// com'era. E' il riscontro che prima mancava: il log lo vede solo chi lo guarda.
fn show_save_outcome(
    time: Res<Time>,
    mut notice: ResMut<SaveNotice>,
    mut buttons: Query<(&LayoutButton, &mut BackgroundColor)>,
    mut labels: Query<(&LayoutButtonLabel, &mut Text)>,
) {
    let Some((saved, timer)) = notice.0.as_mut() else {
        return;
    };

    timer.tick(time.delta());
    let saved = *saved;
    let expired = timer.is_finished();

    let (colour, text) = match (expired, saved) {
        (true, _) => (BUTTON_IDLE, LayoutAction::Save.label()),
        (false, true) => (BUTTON_DONE, "Salvato"),
        (false, false) => (BUTTON_FAILED, "Errore"),
    };

    for (button, mut background) in buttons.iter_mut() {
        if button.0 == LayoutAction::Save {
            background.0 = colour;
        }
    }
    for (label, mut content) in labels.iter_mut() {
        if label.0 == LayoutAction::Save {
            content.0 = text.to_string();
        }
    }

    if expired {
        notice.0 = None;
    }
}

/// Tasto destro su un oggetto: lo gira di un quarto di giro. E' la freccia a
/// dire dove finisce il carrier, quindi girarla cambia davvero il percorso.
fn rotate_piece(
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform)>,
    ui_interactions: Query<&Interaction>,
    placed: Query<(Entity, &Placed)>,
    mut facings: Query<&mut Facing>,
) {
    if !mouse.just_pressed(MouseButton::Right) {
        return;
    }

    let Some(cell) = cursor_cell(&windows, &camera_query, &ui_interactions) else {
        return;
    };
    let Some((entity, _)) = placed.iter().find(|(_, placed)| placed.cell == cell) else {
        return;
    };

    if let Ok(mut facing) = facings.get_mut(entity) {
        facing.0 = facing.0.turn_right();
    }
}

/// In modo "Sposta" il trascinamento porta con se' l'oggetto su cui si e'
/// premuto; se sotto non c'era niente, a muoversi e' la vista.
fn drag_piece(
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform)>,
    ui_interactions: Query<&Interaction>,
    selected: Res<SelectedTool>,
    mut dragged: ResMut<DraggedPiece>,
    mut placed: Query<(Entity, &mut Placed, &mut Transform)>,
) {
    if mouse.just_released(MouseButton::Left) {
        dragged.0 = None;
        return;
    }

    let Some(cell) = cursor_cell(&windows, &camera_query, &ui_interactions) else {
        return;
    };

    if mouse.just_pressed(MouseButton::Left) {
        dragged.0 = (selected.0 == EditorTool::Pan)
            .then(|| {
                placed
                    .iter()
                    .find(|(_, placed, _)| placed.cell == cell)
                    .map(|(entity, _, _)| entity)
            })
            .flatten();
        return;
    }

    let Some(entity) = dragged.0 else {
        return;
    };

    // Una cella gia' occupata non si sovrascrive trascinandoci sopra: sarebbe
    // una perdita silenziosa dell'oggetto che c'era.
    let occupied = placed
        .iter()
        .any(|(other, placed, _)| other != entity && placed.cell == cell);
    if occupied {
        return;
    }

    if let Ok((_, mut placed, mut transform)) = placed.get_mut(entity)
        && placed.cell != cell
    {
        placed.cell = cell;
        transform.translation = grid::cell_center(cell).extend(transform.translation.z);
    }
}

/// I due bottoni sul file di layout. Il salvataggio raccoglie quello che c'e' in
/// scena; il caricamento la sostituisce, carrier in volo compresi: lasciarli
/// vivi vorrebbe dire vederli percorrere corsie che non esistono piu'.
fn handle_layout_buttons(
    mut commands: Commands,
    buttons: Query<(&Interaction, &LayoutButton), Changed<Interaction>>,
    placed: Query<(Entity, &Placed)>,
    pieces: Query<(&Placed, &Facing)>,
    carriers: Query<Entity, With<Carrier>>,
    layout_file: Res<LayoutFile>,
    mut notice: ResMut<SaveNotice>,
) {
    for (interaction, button) in buttons.iter() {
        if *interaction != Interaction::Pressed {
            continue;
        }

        match button.0 {
            LayoutAction::Save => {
                let layout = layout::collect(&pieces);

                let saved = match layout::save(&layout, &layout_file.path) {
                    Ok(()) => {
                        info!("layout salvato in {}", layout_file.path);
                        true
                    }
                    Err(error) => {
                        error!("salvataggio fallito: {error}");
                        false
                    }
                };

                notice.0 = Some((saved, Timer::from_seconds(NOTICE_SECONDS, TimerMode::Once)));
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
