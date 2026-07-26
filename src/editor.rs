use bevy::prelude::*;
use bevy::sprite_render::AlphaMode2d;
use bevy::window::{CursorIcon, SystemCursorIcon};
use serde::{Deserialize, Serialize};

use crate::carrier::Carrier;
use crate::grid;
use crate::layout::{self, LayoutFile, Placed, Switches, place_in_cell, spawn_layout};
use crate::name::{self, Identity, PieceId, PieceName};
use crate::piece::{self, Facing, PieceShapes};
use crate::simulation::SimulationState;

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
    /// Lettore sotto la linea. Non tocca il flusso: guarda e basta.
    Antenna,
    /// Fotocellula su una parete della cella: vede passare le provette.
    TubeSensor,
    /// La stessa cosa, ma conta qualunque carrier.
    CarrierSensor,
}

impl Tool {
    pub fn label(self) -> &'static str {
        match self {
            Tool::CarrierSource => "Sorgente",
            Tool::Gate => "Gate",
            Tool::Divert => "Divert",
            Tool::Atr => "ATR",
            Tool::Despawner => "Despawn",
            Tool::Turner => "Svolta",
            Tool::Reverser => "Inversione",
            Tool::Antenna => "Antenna",
            Tool::TubeSensor => "Sens. tubo",
            Tool::CarrierSensor => "Sens. carrier",
        }
    }

    pub fn layer(self) -> Layer {
        match self {
            Tool::Antenna => Layer::Under,
            Tool::TubeSensor | Tool::CarrierSensor => Layer::Side,
            _ => Layer::Track,
        }
    }
}

/// Su che piano vive un oggetto. Serve perche' l'antenna sta sotto la linea:
/// non contende la cella agli altri, e una cella puo' benissimo avere un gate
/// con un'antenna sotto. Due oggetti dello stesso piano invece si escludono,
/// come e' sempre stato.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layer {
    /// Sulla linea: sorgenti, gate, deviatori, uscite.
    Track,
    /// Su una parete della cella: i sensori. Uno solo per cella, ma convive con
    /// l'oggetto di linea, perche' sta su un lato che quello lascia libero.
    Side,
    /// Sotto la linea: ci passano sopra sia i carrier sia gli oggetti.
    Under,
}

impl Layer {
    /// Quota a cui nasce l'oggetto. I carrier viaggiano a zero, quindi il piano
    /// di sotto deve stare in negativo per finire davvero sotto di loro.
    pub fn z(self) -> f32 {
        match self {
            Layer::Track => 1.0,
            Layer::Side => 0.9,
            Layer::Under => -1.0,
        }
    }
}

/// Chi occupa la cella su un certo piano. Il piazzamento guarda solo il proprio:
/// appoggiare un'antenna sotto un gate non tocca il gate, e rimettere il gate
/// non porta via l'antenna.
fn occupant_on<'a>(
    cell: IVec2,
    layer: Layer,
    objects: impl Iterator<Item = (Entity, &'a Placed)>,
) -> Option<(Entity, Tool)> {
    objects
        .filter(|(_, placed)| placed.cell == cell && placed.tool.layer() == layer)
        .map(|(entity, placed)| (entity, placed.tool))
        .next()
}

/// L'oggetto che un clic in quel punto colpisce. Sulla figura dell'oggetto di
/// linea il clic e' suo; altrove nella cella e' di quello che gli sta sotto, che
/// li' e' scoperto e si vede. E' cosi' che si accende l'antenna al centro della
/// cella di un gate, la cui sbarra occupa solo un lato.
pub fn clicked_piece<'a, I>(
    point: Vec2,
    cell: IVec2,
    objects: impl Fn() -> I,
) -> Option<(Entity, Tool)>
where
    I: Iterator<Item = (Entity, &'a Placed, &'a Facing)>,
{
    let in_cell = |layer: Layer| {
        objects().find(move |(_, placed, _)| placed.cell == cell && placed.tool.layer() == layer)
    };
    let named = |found: Option<(Entity, &Placed, &Facing)>| {
        found.map(|(entity, placed, _)| (entity, placed.tool))
    };

    // Prima chi ha una figura sotto il puntatore: fra l'oggetto di linea e il
    // sensore sulla parete non c'e' sovrapposizione, quindi al piu' uno risponde.
    let on_the_figure = objects().find(|(_, placed, facing)| {
        placed.cell == cell
            && placed.tool.layer() != Layer::Under
            && piece::covers(placed.tool, **facing, grid::cell_center(cell), point)
    });

    if on_the_figure.is_some() {
        return named(on_the_figure);
    }

    // Nel resto della cella risponde l'antenna, se c'e'; altrimenti si torna a
    // quello che la cella contiene.
    named(
        in_cell(Layer::Under)
            .or(in_cell(Layer::Track))
            .or(in_cell(Layer::Side)),
    )
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
    /// Gate e antenna in un colpo solo, girati allo stesso modo: l'antenna
    /// finisce esattamente dove la sbarra ferma il carrier. Restano due oggetti
    /// distinti, quindi si accendono e si spengono ognuno per conto suo, e nel
    /// file salvato compaiono separati come sempre.
    GateWithAntenna,
}

impl EditorTool {
    fn label(self) -> &'static str {
        match self {
            EditorTool::Pan => "Sposta",
            EditorTool::Erase => "Cancella",
            EditorTool::Place(tool) => tool.label(),
            EditorTool::GateWithAntenna => "Gate+Ant.",
        }
    }

    /// Che cosa piazza questo strumento.
    fn places(self) -> Vec<Tool> {
        match self {
            EditorTool::Pan | EditorTool::Erase => Vec::new(),
            EditorTool::Place(tool) => vec![tool],
            EditorTool::GateWithAntenna => vec![Tool::Gate, Tool::Antenna],
        }
    }
}

/// Ordine dei bottoni nella barra.
const MODES: [EditorTool; 13] = [
    EditorTool::Pan,
    EditorTool::Erase,
    EditorTool::Place(Tool::CarrierSource),
    EditorTool::Place(Tool::Gate),
    EditorTool::Place(Tool::Divert),
    EditorTool::Place(Tool::Atr),
    EditorTool::Place(Tool::Despawner),
    EditorTool::Place(Tool::Turner),
    EditorTool::Place(Tool::Reverser),
    EditorTool::Place(Tool::Antenna),
    EditorTool::Place(Tool::TubeSensor),
    EditorTool::Place(Tool::CarrierSensor),
    EditorTool::GateWithAntenna,
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
    cursor_world(windows, camera_query, ui_interactions).map(grid::cell)
}

/// Il punto del mondo sotto il mouse. Serve a chi non si accontenta della cella:
/// dentro una cella ci sono due oggetti sovrapposti, e per sapere quale si sta
/// puntando bisogna guardare dove esattamente e' caduto il clic.
pub fn cursor_world(
    windows: &Query<&Window>,
    camera_query: &Query<(&Camera, &GlobalTransform)>,
    ui_interactions: &Query<&Interaction>,
) -> Option<Vec2> {
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

    camera.viewport_to_world_2d(camera_transform, cursor).ok()
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
                    follow_tool_with_cursor,
                    update_ghost,
                    handle_layout_buttons,
                    show_save_outcome,
                ),
            )
            // Durante una riproduzione gli oggetti sono inerti: il loro stato
            // arriva dal file, e un clic verrebbe sovrascritto dall'istante
            // successivo. Meglio non rispondere affatto che rispondere per
            // mezzo secondo.
            .add_systems(
                Update,
                (
                    place_selected_tool,
                    toggle_by_click,
                    rotate_piece,
                    drag_piece,
                )
                    .run_if(not(in_state(SimulationState::Replaying))),
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
                row_gap: Val::Px(6.0),
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
            height: Val::Px(34.0),
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

/// Il cursore dice in che modo si e' senza doverlo andare a leggere nella barra.
/// Si usano solo i cursori di sistema: quelli disegnati a mano non sarebbero
/// coerenti con il resto della scrivania dell'utente, e soprattutto non
/// seguirebbero il tema e la scala di chi usa il programma.
fn follow_tool_with_cursor(
    mut commands: Commands,
    selected: Res<SelectedTool>,
    windows: Query<Entity, With<Window>>,
) {
    if !selected.is_changed() {
        return;
    }

    let icon = match selected.0 {
        // La manina aperta e' il cursore di sistema per "questo si afferra e si
        // trascina", che e' proprio quello che fa il modo Sposta.
        EditorTool::Pan => CursorIcon::System(SystemCursorIcon::Grab),
        // Il divieto: fra i cursori standard non esiste una X, e questo e' il
        // piu' vicino come significato. Si preferisce a NoDrop perche' sui tre
        // sistemi e' identico a lui tranne su Linux, dove pero' "not-allowed"
        // c'e' in ogni tema mentre "no-drop" in qualcuno manca.
        EditorTool::Erase => CursorIcon::System(SystemCursorIcon::NotAllowed),
        _ => CursorIcon::default(),
    };

    for window in windows.iter() {
        commands.entity(window).insert(icon.clone());
    }
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
    state: Res<State<SimulationState>>,
    mut ghost: Query<(&mut Transform, &mut Visibility, &mut Mesh2d), With<Ghost>>,
) {
    // Niente anteprima in modo "Sposta" ne' durante una riproduzione: in
    // entrambi i casi il clic non piazzerebbe nulla, e mostrare dove finirebbe
    // sarebbe una promessa falsa.
    let replaying = *state.get() == SimulationState::Replaying;
    let target = match (
        selected.0,
        cursor_cell(&windows, &camera_query, &ui_interactions),
    ) {
        (EditorTool::Place(tool), Some(cell)) if !replaying => Some((tool, cell)),
        // Dello strumento doppio si mostra la sbarra: e' la parte che si vede.
        (EditorTool::GateWithAntenna, Some(cell)) if !replaying => Some((Tool::Gate, cell)),
        _ => None,
    };

    let Some((tool, cell)) = target else {
        if let Ok((_, mut visibility, _)) = ghost.single_mut() {
            *visibility = Visibility::Hidden;
        }
        return;
    };

    // Gli oggetti di linea sono quadrati tutti uguali, quindi per loro
    // l'anteprima dice dove finira' il prossimo e non che aspetto avra'.
    // L'antenna e' un cerchio, ed e' l'unica forma diversa: mostrarla quadrata
    // sarebbe una promessa sbagliata.
    let mesh = match tool {
        Tool::Antenna => piece::circle(&shapes),
        Tool::Gate | Tool::TubeSensor | Tool::CarrierSensor => piece::bar(&shapes),
        _ => piece::square(&shapes),
    };
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
    placed: Query<(Entity, &Placed, &Facing)>,
    ui_interactions: Query<&Interaction>,
    mut switches: Switches,
    selected: Res<SelectedTool>,
    identities: Query<(&PieceId, &PieceName)>,
) {
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }

    let Some(point) = cursor_world(&windows, &camera_query, &ui_interactions) else {
        return;
    };
    let cell = grid::cell(point);

    // In modo "Sposta" il tasto sinistro trascina la vista; in modo "Cancella"
    // toglie di mezzo l'oggetto puntato. In nessuno dei due si piazza qualcosa.
    if selected.0 == EditorTool::Pan {
        return;
    }
    if selected.0 == EditorTool::Erase {
        // Si toglie quello che si sta puntando: sulla figura l'oggetto di linea,
        // altrove l'antenna che gli sta sotto.
        if let Some((entity, _)) = clicked_piece(point, cell, || placed.iter()) {
            commands.entity(entity).despawn();
        }
        return;
    }

    // Chi si aggiunge a una cella gia' abitata si mette d'accordo con
    // l'oggetto di linea che ci trova: l'antenna guarda dalla sua stessa parte,
    // il sensore di traverso. Poi il tasto destro gira tutta la cella insieme,
    // quindi il rapporto non si perde piu'.
    let host = placed
        .iter()
        .find(|(_, placed, _)| placed.cell == cell && placed.tool.layer() == Layer::Track)
        .map(|(_, _, facing)| *facing);

    for tool in selected.0.places() {
        // Si guarda solo il proprio piano: un'antenna si appoggia sotto un
        // oggetto gia' piazzato senza portarlo via, e viceversa.
        let same_layer = occupant_on(
            cell,
            tool.layer(),
            placed.iter().map(|(entity, placed, _)| (entity, placed)),
        );

        if let Some((entity, occupant)) = same_layer {
            // Stesso strumento: si accende o si spegne quello che c'e', e con lo
            // strumento doppio si accende solo la parte che si sta puntando.
            // Rimpiazzarli tutti e due azzererebbe i loro interruttori.
            if occupant == tool {
                let pointed = clicked_piece(point, cell, || placed.iter());

                if pointed.is_none_or(|(pointed, _)| pointed == entity) {
                    switches.toggle(entity);
                }
                continue;
            }

            commands.entity(entity).despawn();
        }

        let facing = match (tool.layer(), host) {
            (Layer::Side, Some(host)) => Facing(host.0.turn_right()),
            (_, Some(host)) => host,
            (_, None) => Facing::default(),
        };

        // Ogni oggetto nasce gia' identificato: il numero per le registrazioni,
        // il nome per mqtt. Il nome si cambia dal pannello, ma non resta mai
        // senza, e l'id non cambia mai.
        let names: Vec<String> = identities.iter().map(|(_, name)| name.0.clone()).collect();
        let ids: Vec<u32> = identities.iter().map(|(id, _)| id.0).collect();

        place_in_cell(
            &mut commands,
            tool,
            cell,
            facing,
            Identity {
                id: name::next_free_id(&ids),
                name: name::next_free(tool, &names),
            },
        );
    }
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
    placed: Query<(Entity, &Placed, &Facing)>,
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

    let Some(point) = cursor_world(&windows, &camera_query, &ui_interactions) else {
        return;
    };
    let cell = grid::cell(point);
    let Some((entity, _)) = clicked_piece(point, cell, || placed.iter()) else {
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
    // Si gira tutto quello che sta nella cella, non solo l'oggetto in cima: un
    // gate e l'antenna che gli sta sotto devono restare d'accordo, altrimenti
    // la sbarra finirebbe da un lato e l'antenna dall'altro.
    let in_cell: Vec<Entity> = placed
        .iter()
        .filter(|(_, placed)| placed.cell == cell)
        .map(|(entity, _)| entity)
        .collect();

    for entity in in_cell {
        if let Ok(mut facing) = facings.get_mut(entity) {
            facing.0 = facing.0.turn_right();
        }
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
    mut placed: Query<(Entity, &mut Placed, &mut Transform, &Facing)>,
) {
    if mouse.just_released(MouseButton::Left) {
        dragged.0 = None;
        return;
    }

    let Some(point) = cursor_world(&windows, &camera_query, &ui_interactions) else {
        return;
    };
    let cell = grid::cell(point);

    if mouse.just_pressed(MouseButton::Left) {
        // Si prende quello che si sta puntando, con la stessa regola del clic:
        // trascinare per la corona sposta l'antenna e non il gate che la copre.
        dragged.0 = (selected.0 == EditorTool::Pan)
            .then(|| {
                clicked_piece(point, cell, || {
                    placed
                        .iter()
                        .map(|(entity, placed, _, facing)| (entity, placed, facing))
                })
                .map(|(entity, _)| entity)
            })
            .flatten();
        return;
    }

    let Some(entity) = dragged.0 else {
        return;
    };

    // Una cella gia' occupata non si sovrascrive trascinandoci sopra: sarebbe
    // una perdita silenziosa dell'oggetto che c'era. Occupata pero' vuol dire
    // sul piano di chi si sta trascinando: un'antenna passa sotto un gate.
    let Ok((_, dragging, _, _)) = placed.get(entity) else {
        return;
    };
    let layer = dragging.tool.layer();
    let occupied = placed.iter().any(|(other, placed, _, _)| {
        other != entity && placed.cell == cell && placed.tool.layer() == layer
    });
    if occupied {
        return;
    }

    if let Ok((_, mut placed, mut transform, _)) = placed.get_mut(entity)
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
    pieces: Query<(&Placed, &Facing, &PieceId, &PieceName)>,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::piece::{BAR_OFFSET, PIECE_SIZE};

    fn cell_with(tools: [Tool; 2]) -> Vec<(Entity, Placed, Facing)> {
        tools
            .into_iter()
            .map(|tool| {
                (
                    Entity::PLACEHOLDER,
                    Placed {
                        tool,
                        cell: IVec2::new(2, -3),
                    },
                    Facing::default(),
                )
            })
            .collect()
    }

    /// Il punto dell'antenna: sta sotto, quindi convive con l'oggetto di linea
    /// invece di prenderne il posto.
    #[test]
    fn an_antenna_and_an_object_share_the_same_cell() {
        let cell = IVec2::new(2, -3);

        // In tutti e due gli ordini di inserimento: chi c'era prima non conta.
        for order in [[Tool::Antenna, Tool::Gate], [Tool::Gate, Tool::Antenna]] {
            let objects = cell_with(order);
            let entries = || objects.iter().map(|(entity, placed, _)| (*entity, placed));

            assert_eq!(
                occupant_on(cell, Layer::Track, entries()).map(|(_, tool)| tool),
                Some(Tool::Gate),
                "un gate vede solo il gate"
            );
            assert_eq!(
                occupant_on(cell, Layer::Under, entries()).map(|(_, tool)| tool),
                Some(Tool::Antenna),
                "un'antenna vede solo l'antenna"
            );
            let triples = || {
                objects
                    .iter()
                    .map(|(entity, placed, facing)| (*entity, placed, facing))
            };
            let on_the_bar = grid::cell_center(cell) + Vec2::new(-BAR_OFFSET, 0.0);
            assert_eq!(
                clicked_piece(on_the_bar, cell, triples).map(|(_, tool)| tool),
                Some(Tool::Gate),
                "il clic sulla sbarra prende quello che si vede"
            );
        }
    }

    /// Su una cella con la sola antenna non c'e' niente da cui difenderla: il
    /// clic la prende ovunque cada nella cella.
    #[test]
    fn an_antenna_alone_is_the_one_you_click() {
        let cell = IVec2::ZERO;
        let objects = vec![(
            Entity::PLACEHOLDER,
            Placed {
                tool: Tool::Antenna,
                cell,
            },
            Facing::default(),
        )];
        let triples = || {
            objects
                .iter()
                .map(|(entity, placed, facing)| (*entity, placed, facing))
        };

        assert_eq!(
            clicked_piece(grid::cell_center(cell), cell, triples).map(|(_, tool)| tool),
            Some(Tool::Antenna)
        );
        assert!(
            occupant_on(
                cell,
                Layer::Track,
                objects.iter().map(|(entity, placed, _)| (*entity, placed))
            )
            .is_none()
        );
    }

    /// Il punto della modifica alla sbarra: nella cella di un gate il centro
    /// resta libero, quindi ci sta un'antenna e la si puo' accendere cliccandoci
    /// sopra. Sulla sbarra invece il clic e' del gate.
    #[test]
    fn in_a_gate_cell_the_middle_belongs_to_the_antenna() {
        let cell = IVec2::new(2, -3);
        let centre = grid::cell_center(cell);
        let objects = cell_with([Tool::Gate, Tool::Antenna]);
        let entries = || {
            objects
                .iter()
                .map(|(entity, placed, facing)| (*entity, placed, facing))
        };

        // Il gate e' girato a sinistra: la sbarra sta sul lato sinistro.
        let on_the_bar = centre + Vec2::new(-BAR_OFFSET, 0.0);
        assert_eq!(
            clicked_piece(on_the_bar, cell, entries).map(|(_, tool)| tool),
            Some(Tool::Gate)
        );

        assert_eq!(
            clicked_piece(centre, cell, entries).map(|(_, tool)| tool),
            Some(Tool::Antenna),
            "al centro della cella c'e' l'antenna, scoperta"
        );

        // Sul lato opposto non c'e' sbarra: li' non c'e' nemmeno l'antenna, ma
        // il clic resta della cella, quindi torna al gate.
        let far_side = centre + Vec2::new(BAR_OFFSET, 0.0);
        assert_eq!(
            clicked_piece(far_side, cell, entries).map(|(_, tool)| tool),
            Some(Tool::Antenna),
            "fuori dalla figura del gate risponde chi gli sta sotto"
        );
    }

    /// Sotto un oggetto quadrato l'antenna e' coperta del tutto, e il quadrato
    /// si prende i clic che gli cadono sopra.
    #[test]
    fn under_a_square_piece_the_square_takes_the_click() {
        let cell = IVec2::new(2, -3);
        let centre = grid::cell_center(cell);
        let objects = cell_with([Tool::Atr, Tool::Antenna]);
        let entries = || {
            objects
                .iter()
                .map(|(entity, placed, facing)| (*entity, placed, facing))
        };

        assert_eq!(
            clicked_piece(centre, cell, entries).map(|(_, tool)| tool),
            Some(Tool::Atr)
        );
        // Negli angoli della cella, fuori dal quadrato, si arriva all'antenna.
        let corner = centre + Vec2::splat(PIECE_SIZE / 2.0 + 2.0);
        assert_eq!(
            clicked_piece(corner, cell, entries).map(|(_, tool)| tool),
            Some(Tool::Antenna)
        );
    }

    /// L'antenna deve finire sotto ai carrier, che viaggiano a quota zero, e
    /// sotto agli oggetti di linea.
    #[test]
    fn the_antenna_lies_below_carriers_and_objects() {
        assert!(Tool::Antenna.layer().z() < 0.0);
        assert!(Tool::Antenna.layer().z() < Tool::Gate.layer().z());
    }
}
