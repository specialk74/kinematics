use bevy::prelude::*;
use bevy::sprite_render::AlphaMode2d;
use bevy::window::{CursorIcon, SystemCursorIcon};

use crate::carrier::Carrier;
use crate::grid;
use crate::layout::{self, LayoutFile, Placed, place_in_cell, spawn_layout};
use crate::name::{self, Identity, PieceId, PieceName};
use crate::piece::{self, Facing, Layer, PieceShapes, Tool};
use crate::simulation::{Mode, SimulationState};
use crate::switch::Switch;
use crate::ui::{
    BUTTON_IDLE, BUTTON_READY, PALETTE_WIDTH, button_label, button_node, pointer_over_ui,
    top_button,
};

const BUTTON_SELECTED: Color = Color::srgb(0.25, 0.45, 0.80);
const CAPTION_COLOR: Color = Color::srgb(0.55, 0.55, 0.62);
/// Davanti a tutto: l'anteprima deve restare leggibile anche sopra un oggetto
/// gia' piazzato, che e' proprio il caso in cui serve di piu'.
const GHOST_Z: f32 = 2.0;

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

    // Prima chi ha una figura sotto il puntatore, e in ordine di piano: un
    // pezzo di guida vale come area tutta la sua cella, quindi senza un ordine
    // finirebbe per rubare il clic a chiunque ci stia sopra - e non avendo
    // niente da comandare, il clic sembrerebbe non fare niente. L'antenna sta
    // in mezzo: sul suo cerchio risponde lei, fuori risponde la guida.
    for layer in [Layer::Track, Layer::Side, Layer::Under, Layer::Rail] {
        let on_the_figure = objects().find(|(_, placed, facing)| {
            placed.cell == cell
                && placed.tool.layer() == layer
                && piece::covers(placed.tool, **facing, grid::cell_center(cell), point)
        });

        if on_the_figure.is_some() {
            return named(on_the_figure);
        }
    }

    // Nel resto della cella risponde l'antenna, se c'e'; altrimenti si torna a
    // quello che la cella contiene, guida compresa - che sta sotto a tutti.
    named(
        in_cell(Layer::Under)
            .or(in_cell(Layer::Track))
            .or(in_cell(Layer::Side))
            .or(in_cell(Layer::Rail)),
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
const MODES: [EditorTool; 14] = [
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
    EditorTool::Place(Tool::Guide),
    EditorTool::GateWithAntenna,
];

#[derive(Component)]
struct ModeButton;

#[derive(Component)]
struct ModeLabel;

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

/// La barra degli strumenti: in simulazione sparisce.
#[derive(Component)]
struct Palette;

/// Il riquadro con il nome del file e i suoi due comandi.
#[derive(Component)]
struct FileBox;

/// La cornice attorno alla finestra: dice in che modo si e' senza doverlo
/// leggere da nessuna parte. E' un nodo senza `Button` e senza `Interaction`,
/// quindi non intercetta i clic: si limita a colorare il bordo.
#[derive(Component)]
struct ModeFrame;

/// Spessore della cornice e i tre colori, tenui e trasparenti perche' devono
/// dire in che modo si e' senza rubare l'occhio all'impianto.
const FRAME_THICKNESS: f32 = 6.0;
const FRAME_EDITING: Color = Color::srgba(0.85, 0.20, 0.20, 0.30);
const FRAME_SIMULATING: Color = Color::srgba(0.20, 0.80, 0.30, 0.30);
const FRAME_REPLAYING: Color = Color::srgba(0.25, 0.45, 0.95, 0.30);

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

/// Oggetto che si sta trascinando. Lo legge anche la camera: mentre si sposta
/// un oggetto la vista deve restare ferma.
#[derive(Resource, Default)]
pub struct DraggedPiece(pub Option<Entity>);

/// Come sara' girato il prossimo oggetto piazzato. Resta com'e' fra un
/// piazzamento e l'altro: chi sta costruendo una fila di guide orizzontali le
/// vuole tutte orizzontali, e rigirarle una per una sarebbe un lavoro inutile.
#[derive(Resource, Default)]
pub struct PendingFacing(pub Facing);

/// Sagoma semitrasparente che mostra dove finirebbe l'oggetto se si cliccasse
/// ora, e come sarebbe girato. Porta con se' il tipo che sta mostrando: se
/// cambia, la sagoma va rifatta.
#[derive(Component)]
struct Ghost(Tool);

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
            .init_resource::<PendingFacing>()
            .init_resource::<SaveNotice>()
            .init_resource::<DraggedPiece>()
            .add_systems(
                Startup,
                (setup_palette, setup_file_box, setup_ghost_material),
            )
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
                (place_selected_tool, rotate_piece, drag_piece)
                    .run_if(in_state(Mode::Editing))
                    .run_if(not(in_state(SimulationState::Replaying))),
            )
            // In simulazione i due tasti comandano gli oggetti invece di
            // costruirli: sinistro il servizio, destro l'azione.
            .add_systems(
                Update,
                (enable_by_click, activate_by_click)
                    .run_if(in_state(Mode::Simulating))
                    .run_if(not(in_state(SimulationState::Replaying))),
            )
            .init_state::<Mode>()
            .add_systems(Startup, (setup_mode_button, setup_mode_frame))
            .add_systems(Update, (switch_mode, show_mode, refresh_mode_frame));
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
                row_gap: Val::Px(6.0),
                ..default()
            },
            BackgroundColor(Color::srgb(0.10, 0.10, 0.12)),
            Palette,
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
        });
}

/// I comandi sul file stanno in un riquadro loro, staccato dalla barra degli
/// strumenti: in simulazione la barra sparisce ma Carica serve ancora, perche'
/// si puo' voler provare un altro impianto.
fn setup_file_box(mut commands: Commands, layout_file: Res<LayoutFile>) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                bottom: Val::Px(0.0),
                width: Val::Px(PALETTE_WIDTH),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(8.0)),
                row_gap: Val::Px(6.0),
                ..default()
            },
            BackgroundColor(Color::srgb(0.10, 0.10, 0.12)),
            // Sopra alla barra degli strumenti, che occupa tutta la colonna ed
            // e' opaca. Senza dirlo, a decidere chi copre chi sarebbe l'ordine
            // con cui i due sistemi di avvio si trovano a girare, che non e'
            // garantito: e infatti il riquadro finiva sotto.
            GlobalZIndex(1),
            FileBox,
        ))
        .with_children(|box_| {
            box_.spawn((
                Text::new("File"),
                TextFont {
                    font_size: 10.0,
                    ..default()
                },
                TextColor(CAPTION_COLOR),
            ));
            box_.spawn((
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
                box_.spawn((
                    button_node(),
                    BackgroundColor(BUTTON_READY),
                    LayoutButton(action),
                    children![(button_label(action.label()), LayoutButtonLabel(action))],
                ));
            }
        });
}

/// Il cursore dice in che modo si e' senza doverlo andare a leggere nella barra.
/// Si usano solo i cursori di sistema: quelli disegnati a mano non sarebbero
/// coerenti con il resto della scrivania dell'utente, e soprattutto non
/// seguirebbero il tema e la scala di chi usa il programma.
fn follow_tool_with_cursor(
    mut commands: Commands,
    mode: Res<State<Mode>>,
    selected: Res<SelectedTool>,
    windows: Query<Entity, With<Window>>,
) {
    if !selected.is_changed() && !mode.is_changed() {
        return;
    }

    // In simulazione lo strumento scelto nell'editor non conta piu': si comanda
    // e si trascina, quindi la manina. Senza questo restava il cursore
    // dell'ultimo strumento usato, che prometteva un'azione che li' non esiste.
    if *mode.get() == Mode::Simulating {
        for window in windows.iter() {
            commands
                .entity(window)
                .insert(CursorIcon::System(SystemCursorIcon::Grab));
        }
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
    mode: Res<State<Mode>>,
    state: Res<State<SimulationState>>,
    pending: Res<PendingFacing>,
    mut ghost: Query<(Entity, &Ghost, &mut Transform, &mut Visibility)>,
) {
    // Niente anteprima in modo "Sposta", fuori dall'editor o durante una
    // riproduzione: in tutti questi casi il clic non piazzerebbe nulla, e
    // mostrare dove finirebbe sarebbe una promessa falsa. Restava in scena la
    // sagoma dell'ultimo oggetto che si stava piazzando.
    let replaying = *state.get() == SimulationState::Replaying || *mode.get() != Mode::Editing;
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
        if let Ok((_, _, _, mut visibility)) = ghost.single_mut() {
            *visibility = Visibility::Hidden;
        }
        return;
    };

    // L'anteprima e' l'oggetto vero, in trasparenza: stessa figura, stessa
    // freccia, stesso orientamento. Mostrarne uno diverso da quello che poi
    // compare e' peggio che non mostrarlo affatto.
    let transform = Transform::from_translation(grid::cell_center(cell).extend(GHOST_Z))
        .with_rotation(pending.0.0.rotation());

    match ghost.single_mut() {
        // La figura dipende dal tipo: finche' resta lo stesso basta spostare la
        // sagoma, che e' quello che succede a ogni movimento del mouse.
        Ok((entity, showing, mut ghost_transform, mut visibility)) if showing.0 == tool => {
            *ghost_transform = transform;
            *visibility = Visibility::Visible;
            let _ = entity;
        }
        // Tipo cambiato, o prima sagoma della sessione: si rifa'.
        outcome => {
            if let Ok((entity, _, _, _)) = outcome {
                commands.entity(entity).despawn();
            }

            let (shape, arrow) = piece::dressing(&shapes, tool);
            let ghost_entity = commands
                .spawn((transform, Visibility::Visible, Ghost(tool)))
                .id();

            piece::dress_shape(
                &mut commands,
                ghost_entity,
                &shapes,
                shape,
                ghost_material.0.clone(),
                arrow,
            );
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
    selected: Res<SelectedTool>,
    pending: Res<PendingFacing>,
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

    for tool in selected.0.places() {
        // L'orientamento e' quello scelto prima di piazzare, quello che
        // l'anteprima sta mostrando: niente correzioni automatiche, altrimenti
        // l'anteprima direbbe una cosa e il piazzamento ne farebbe un'altra.
        let facing = pending.0;

        // Guida e deviatore possono benissimo dividersi la cella: il fianco
        // che il deviatore apre lo lascia aperto la guida stessa, che dove
        // qualcuno devia smette di disegnare quel bordo. Prima si cancellava il
        // tratto di guida, e non bastava: quello che chiudeva il passaggio
        // poteva essere il tratto della cella accanto, e cancellare anche
        // quello avrebbe lasciato un buco nella parete della corsia.
        // Chi occupa la cella. Si guarda solo il proprio piano: un'antenna si
        // appoggia sotto un oggetto gia' piazzato senza portarlo via, e viceversa.
        let same_layer = occupant_on(
            cell,
            tool.layer(),
            placed.iter().map(|(entity, placed, _)| (entity, placed)),
        );

        if let Some((entity, occupant)) = same_layer {
            // Stesso strumento sulla stessa cella: non si rifa' niente. Rifare
            // l'oggetto gli cambierebbe id e nome, e accenderlo o spegnerlo e'
            // un mestiere della simulazione, non dell'editor.
            if occupant == tool {
                continue;
            }

            commands.entity(entity).despawn();
        }

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

/// Clic sinistro su un oggetto, in simulazione: lo mette in servizio o lo toglie.
/// Fuori servizio l'oggetto non fa la sua funzione e non manda niente, che e' il
/// modo in cui un tester lo fa sparire dal programma sotto collaudo.
fn enable_by_click(
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform)>,
    ui_interactions: Query<&Interaction>,
    placed: Query<(Entity, &Placed, &Facing)>,
    mut switches: Query<&mut Switch>,
) {
    let Some(entity) = pointed_piece(
        MouseButton::Left,
        &mouse,
        &windows,
        &camera_query,
        &ui_interactions,
        &placed,
    ) else {
        return;
    };

    if let Ok(mut switch) = switches.get_mut(entity) {
        switch.enabled = !switch.enabled;
    }
}

/// Clic destro su un oggetto, in simulazione: lo comanda. Per gate, deviatori,
/// svolte, inversioni e sorgenti vuol dire fare o non fare la propria azione;
/// per i sensori e l'antenna vuol dire dichiarare presenza anche a vuoto, che e'
/// come si provano gli scenari che nella realta' non dovrebbero capitare.
fn activate_by_click(
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform)>,
    ui_interactions: Query<&Interaction>,
    placed: Query<(Entity, &Placed, &Facing)>,
    mut switches: Query<&mut Switch>,
) {
    let Some(entity) = pointed_piece(
        MouseButton::Right,
        &mouse,
        &windows,
        &camera_query,
        &ui_interactions,
        &placed,
    ) else {
        return;
    };

    if let Ok(mut switch) = switches.get_mut(entity) {
        switch.active = !switch.active;
    }
}

/// L'oggetto appena cliccato con quel tasto, se il clic e' caduto su uno.
fn pointed_piece(
    button: MouseButton,
    mouse: &ButtonInput<MouseButton>,
    windows: &Query<&Window>,
    camera_query: &Query<(&Camera, &GlobalTransform)>,
    ui_interactions: &Query<&Interaction>,
    placed: &Query<(Entity, &Placed, &Facing)>,
) -> Option<Entity> {
    if !mouse.just_pressed(button) {
        return None;
    }

    let point = cursor_world(windows, camera_query, ui_interactions)?;

    clicked_piece(point, grid::cell(point), || placed.iter()).map(|(entity, _)| entity)
}

/// Il bottone che passa da un mestiere all'altro, e la barra degli strumenti che
/// compare solo quando serve: in simulazione non si piazza niente.
/// Un elemento si nasconde togliendolo dal calcolo dello spazio, non rendendolo
/// invisibile: cosi' non lascia un buco dove stava.
fn shown(visible: bool) -> Display {
    if visible {
        Display::Flex
    } else {
        Display::None
    }
}

fn setup_mode_frame(mut commands: Commands) {
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            border: UiRect::all(Val::Px(FRAME_THICKNESS)),
            ..default()
        },
        BorderColor::all(FRAME_EDITING),
        // Senza questo la cornice si prenderebbe tutti i clic: un nodo che non
        // dichiara niente, per l'interfaccia, blocca quello che ha sotto - e
        // questa copre l'intera finestra.
        Pickable::IGNORE,
        ModeFrame,
    ));
}

/// La cornice segue la modalita', e la riproduzione ha la precedenza: mentre
/// scorre un file quello che si vede non e' ne' l'impianto che si sta
/// costruendo ne' quello che si sta comandando.
fn refresh_mode_frame(
    mode: Res<State<Mode>>,
    state: Res<State<SimulationState>>,
    mut frames: Query<&mut BorderColor, With<ModeFrame>>,
) {
    if !mode.is_changed() && !state.is_changed() {
        return;
    }

    let colour = match (state.get(), mode.get()) {
        (SimulationState::Replaying, _) => FRAME_REPLAYING,
        (_, Mode::Editing) => FRAME_EDITING,
        (_, Mode::Simulating) => FRAME_SIMULATING,
    };

    for mut border in frames.iter_mut() {
        *border = BorderColor::all(colour);
    }
}

fn setup_mode_button(mut commands: Commands) {
    commands.spawn((
        // Sempre verde: cambiare mestiere si puo' sempre.
        top_button(5),
        BackgroundColor(BUTTON_READY),
        ModeButton,
        // Il bottone dice dove porta, non dove si e': e' quello che si sta per
        // fare premendolo, come gia' fa il play/pausa.
        children![(button_label(Mode::default().other().label()), ModeLabel)],
    ));
}

fn switch_mode(
    buttons: Query<&Interaction, (Changed<Interaction>, With<ModeButton>)>,
    mode: Res<State<Mode>>,
    mut next: ResMut<NextState<Mode>>,
) {
    let pressed = buttons
        .iter()
        .any(|interaction| *interaction == Interaction::Pressed);

    if pressed {
        next.set(mode.get().other());
    }
}

fn show_mode(
    mode: Res<State<Mode>>,
    state: Res<State<SimulationState>>,
    mut labels: Query<&mut Text, With<ModeLabel>>,
    mut palette: Query<&mut Node, (With<Palette>, Without<FileBox>, Without<LayoutButton>)>,
    mut file_box: Query<&mut Node, (With<FileBox>, Without<LayoutButton>)>,
    mut file_buttons: Query<(&LayoutButton, &mut Node)>,
    mut selected: ResMut<SelectedTool>,
) {
    if !mode.is_changed() && !state.is_changed() {
        return;
    }

    let replaying = *state.get() == SimulationState::Replaying;
    let editing = *mode.get() == Mode::Editing && !replaying;

    for mut label in labels.iter_mut() {
        label.0 = mode.get().other().label().to_string();
    }
    // Gli strumenti servono solo a costruire: in simulazione e durante una
    // riproduzione non c'e' niente da piazzare.
    for mut node in palette.iter_mut() {
        node.display = shown(editing);
    }
    // Il riquadro del file sparisce solo durante una riproduzione: li' il
    // layout arriva dal file della registrazione, non da quello scelto.
    for mut node in file_box.iter_mut() {
        node.display = shown(!replaying);
    }
    // Salvare ha senso solo dove si modifica l'impianto; caricarlo anche in
    // simulazione, per provarne un altro.
    for (button, mut node) in file_buttons.iter_mut() {
        node.display = match button.0 {
            LayoutAction::Save => shown(editing),
            LayoutAction::Load => shown(!replaying),
        };
    }

    // Tornando all'editor si riparte dallo spostamento: cosi' il primo clic non
    // piazza per sbaglio l'oggetto che era selezionato prima.
    if *mode.get() == Mode::Editing {
        selected.0 = EditorTool::Pan;
    }
}

/// Mostra sul bottone Salva com'e' andata, e dopo qualche secondo lo rimette
/// com'era. E' il riscontro che prima mancava: il log lo vede solo chi lo guarda.
fn show_save_outcome(
    // L'orologio vero, non quello della simulazione: un messaggio a schermo
    // dura due secondi di quelli dell'utente anche mentre l'impianto corre.
    time: Res<Time<Real>>,
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

/// Tasto destro. Con uno strumento in mano gira l'oggetto che si sta per
/// piazzare - cioe' l'anteprima - e l'orientamento resta per i successivi. In
/// modo Sposta gira invece quello che c'e' gia' nella cella puntata.
fn rotate_piece(
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform)>,
    ui_interactions: Query<&Interaction>,
    selected: Res<SelectedTool>,
    mut pending: ResMut<PendingFacing>,
    placed: Query<(Entity, &Placed)>,
    mut facings: Query<&mut Facing>,
) {
    if !mouse.just_pressed(MouseButton::Right) {
        return;
    }

    if selected.0 != EditorTool::Pan && selected.0 != EditorTool::Erase {
        pending.0.0 = pending.0.0.turn_right();
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
    let tool = dragging.tool;
    let layer = tool.layer();
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
    pieces: Query<(&Placed, &Facing, Option<&PieceId>, Option<&PieceName>)>,
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
                let layout = layout::collect(pieces.iter());

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

    /// Un pezzo di guida vale come area tutta la sua cella, ma non deve rubare
    /// il clic all'antenna che gli sta sopra: sul cerchio dell'antenna risponde
    /// lei, fuori risponde la guida. Prima, in una cella con una guida,
    /// l'antenna era irraggiungibile - non si poteva ne' disabilitare ne'
    /// leggerne il nome passandoci sopra.
    #[test]
    fn a_guide_does_not_steal_the_click_from_the_antenna() {
        let cell = IVec2::new(2, -3);
        let centre = grid::cell_center(cell);
        let objects = cell_with([Tool::Guide, Tool::Antenna]);
        let entries = || {
            objects
                .iter()
                .map(|(entity, placed, facing)| (*entity, placed, facing))
        };

        // Sul cerchio dell'antenna: e' suo. L'antenna nasce scostata verso il
        // lato in cui e' girata, quindi la si cerca li'.
        let on_the_antenna = centre + Vec2::new(-crate::piece::ANTENNA_OFFSET, 0.0);
        assert_eq!(
            clicked_piece(on_the_antenna, cell, entries).map(|(_, tool)| tool),
            Some(Tool::Antenna)
        );

        // In un angolo della cella, lontano dal cerchio: risponde la guida.
        let corner = centre + Vec2::splat(grid::GRID_STEP / 2.0 - 2.0);
        assert_eq!(
            clicked_piece(corner, cell, entries).map(|(_, tool)| tool),
            Some(Tool::Guide)
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
}
