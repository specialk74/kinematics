use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;

use crate::editor;
use crate::grid;
use crate::layout::Placed;
use crate::name::{NameRow, Naming, PieceName};
use crate::piece::{Facing, PIECE_SIZE, Tool};
use crate::simulation::Mode;
use crate::simulation::SimulationState;
use crate::ui::BUTTON_IDLE;

const PANEL_WIDTH: f32 = 190.0;
/// Sotto ai bottoni in alto a destra, sopra alla barra di riproduzione.
const PANEL_TOP: f32 = 56.0;
const PANEL_BACKGROUND: Color = Color::srgba(0.10, 0.10, 0.12, 0.92);
const ROW_HEIGHT: f32 = 22.0;
const ROW_FONT: f32 = 11.0;
const ROW_EDITING: Color = Color::srgb(0.25, 0.45, 0.80);
const ROW_REJECTED: Color = Color::srgb(0.70, 0.15, 0.15);
/// Quanto scorre l'elenco a ogni scatto di rotella.
const SCROLL_STEP: f32 = ROW_HEIGHT * 2.0;

/// Il riquadro dell'evidenziatore attorno all'oggetto in scrittura. Sta fermo:
/// lampeggiava, e un lampeggio in mezzo a un impianto che si muove e' rumore.
const HIGHLIGHT_SIZE: f32 = PIECE_SIZE + 16.0;
const HIGHLIGHT_COLOR: Color = Color::srgb(1.0, 0.85, 0.2);
/// Dietro agli oggetti ma davanti ai carrier: l'oggetto evidenziato deve
/// restare visibile, non finire coperto dal proprio evidenziatore.
const HIGHLIGHT_Z: f32 = 0.5;

/// L'etichetta che compare sull'oggetto sotto il puntatore.
const HOVER_FONT_SIZE: f32 = 32.0;
const HOVER_SCALE: f32 = 0.35;
const HOVER_Z: f32 = 3.0;

/// Il pannello dei nomi: l'elenco di tutto quello che c'e' in scena, con il
/// nome con cui ciascuno si presentera' su mqtt.
pub struct NamePanelPlugin;

impl Plugin for NamePanelPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, (setup_panel, setup_highlight, setup_hover))
            .add_systems(
                Update,
                (
                    refresh_rows,
                    scroll_panel,
                    show_highlight,
                    show_hovered_name,
                    only_in_the_editor,
                ),
            );
    }
}

#[derive(Component)]
struct NamePanel;

#[derive(Component)]
struct Highlight;

#[derive(Component)]
struct HoverLabel;

fn setup_panel(mut commands: Commands) {
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(PANEL_TOP),
            right: Val::Px(12.0),
            width: Val::Px(PANEL_WIDTH),
            // Meno di prima: sotto, in basso a destra, adesso c'e' il pannello
            // del collegamento, e due pannelli sovrapposti non si leggono.
            max_height: Val::Percent(45.0),
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(Val::Px(6.0)),
            row_gap: Val::Px(2.0),
            overflow: Overflow::scroll_y(),
            ..default()
        },
        BackgroundColor(PANEL_BACKGROUND),
        ScrollPosition::default(),
        NamePanel,
    ));
}

fn setup_highlight(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    commands.spawn((
        Mesh2d(meshes.add(Rectangle::new(HIGHLIGHT_SIZE, HIGHLIGHT_SIZE))),
        MeshMaterial2d(materials.add(HIGHLIGHT_COLOR)),
        Transform::from_xyz(0.0, 0.0, HIGHLIGHT_Z),
        Visibility::Hidden,
        Highlight,
    ));
}

fn setup_hover(mut commands: Commands) {
    commands.spawn((
        Text2d::new(""),
        TextFont {
            font_size: HOVER_FONT_SIZE,
            ..default()
        },
        TextColor(Color::WHITE),
        Transform::from_xyz(0.0, 0.0, HOVER_Z).with_scale(Vec3::splat(HOVER_SCALE)),
        Visibility::Hidden,
        HoverLabel,
    ));
}

/// L'elenco dei nomi serve mentre si costruisce l'impianto. In simulazione e
/// durante una riproduzione sparisce: li' gli oggetti si comandano, non si
/// battezzano, e trenta righe di nomi rubano soltanto spazio alla scena.
fn only_in_the_editor(
    mode: Res<State<Mode>>,
    state: Res<State<SimulationState>>,
    mut naming: ResMut<Naming>,
    mut panel: Query<&mut Node, With<NamePanel>>,
) {
    if !mode.is_changed() && !state.is_changed() {
        return;
    }

    let wanted = *mode.get() == Mode::Editing && *state.get() != SimulationState::Replaying;

    if !wanted {
        // Chi stava scrivendo un nome resta senza pannello: meglio chiudere la
        // scrittura che lasciarla aperta su una riga che non si vede piu'.
        naming.editing = None;
    }

    for mut node in panel.iter_mut() {
        node.display = if wanted { Display::Flex } else { Display::None };
    }
}

/// Rifa' l'elenco quando c'e' motivo: un oggetto in piu' o in meno, un nome
/// cambiato, o una riga entrata in scrittura. Rifarlo a ogni frame sarebbe
/// sprecato e farebbe sparire il cursore da sotto le dita.
fn refresh_rows(
    mut commands: Commands,
    naming: Res<Naming>,
    objects: Query<(Entity, &Placed, &PieceName)>,
    added: Query<(), Added<Placed>>,
    renamed: Query<(), Changed<PieceName>>,
    mut removed: RemovedComponents<Placed>,
    panel: Query<(Entity, Option<&Children>), With<NamePanel>>,
) {
    let gone = removed.read().count() > 0;
    let changed = naming.is_changed() || !added.is_empty() || !renamed.is_empty() || gone;
    if !changed {
        return;
    }

    let Ok((panel, children)) = panel.single() else {
        return;
    };

    for child in children.into_iter().flatten() {
        commands.entity(*child).despawn();
    }

    // In ordine di nome: e' l'unico ordine che non cambia sotto gli occhi
    // quando si sposta un oggetto nella scena.
    // I pezzi passivi non hanno un nome, quindi la query non li trova: restano
    // fuori dall'elenco da soli, senza un filtro che lo ricordi.
    let mut rows: Vec<(Entity, Tool, String)> = objects
        .iter()
        .map(|(entity, placed, name)| (entity, placed.tool, name.0.clone()))
        .collect();
    rows.sort_by(|left, right| left.2.cmp(&right.2));

    commands.entity(panel).with_children(|panel| {
        panel.spawn((
            Text::new(format!("Nomi ({})", rows.len())),
            TextFont {
                font_size: 10.0,
                ..default()
            },
            TextColor(Color::srgb(0.55, 0.55, 0.62)),
        ));

        for (entity, tool, name) in rows {
            let editing = naming.editing == Some(entity);
            let (text, background) = match (editing, naming.rejected) {
                // Il cursore in coda dice dove si sta scrivendo; il rosso che
                // quel nome e' gia' di un altro, o non e' scrivibile.
                (true, false) => (format!("{}_", naming.draft), ROW_EDITING),
                (true, true) => (format!("{}_", naming.draft), ROW_REJECTED),
                (false, _) => (name, BUTTON_IDLE),
            };

            panel.spawn((
                Button,
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Px(ROW_HEIGHT),
                    flex_shrink: 0.0,
                    align_items: AlignItems::Center,
                    padding: UiRect::horizontal(Val::Px(4.0)),
                    ..default()
                },
                BackgroundColor(background),
                NameRow(entity),
                children![(
                    Text::new(format!("{} {text}", tool.label())),
                    TextFont {
                        font_size: ROW_FONT,
                        ..default()
                    },
                    TextColor(Color::WHITE),
                )],
            ));
        }
    });
}

/// La rotella scorre l'elenco quando il puntatore e' sul pannello. Lo zoom
/// resta su Ctrl+rotella, quindi i due non si pestano i piedi.
fn scroll_panel(
    mut wheel: MessageReader<MouseWheel>,
    keys: Res<ButtonInput<KeyCode>>,
    mut panel: Query<(&Interaction, &mut ScrollPosition), With<NamePanel>>,
) {
    let steps: f32 = wheel
        .read()
        .map(|event| match event.unit {
            MouseScrollUnit::Line => event.y,
            MouseScrollUnit::Pixel => event.y / SCROLL_STEP,
        })
        .sum();

    if steps == 0.0 || keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]) {
        return;
    }

    for (interaction, mut scroll) in panel.iter_mut() {
        if *interaction != Interaction::None {
            scroll.0.y = (scroll.0.y - steps * SCROLL_STEP).max(0.0);
        }
    }
}

/// L'oggetto che si sta rinominando resta evidenziato: il pannello dice il
/// nome, la scena dice quale dei tanti e'.
fn show_highlight(
    naming: Res<Naming>,
    objects: Query<&Transform, (With<Placed>, Without<Highlight>)>,
    mut highlight: Query<(&mut Transform, &mut Visibility), With<Highlight>>,
) {
    let Ok((mut transform, mut visibility)) = highlight.single_mut() else {
        return;
    };

    let Some(at) = naming
        .editing
        .and_then(|entity| objects.get(entity).ok())
        .map(|object| object.translation)
    else {
        *visibility = Visibility::Hidden;
        return;
    };

    transform.translation = at.truncate().extend(HIGHLIGHT_Z);
    *visibility = Visibility::Visible;
}

/// Il nome dell'oggetto sotto il puntatore, scritto sopra di lui. Compare solo
/// li': trenta nomi tutti insieme coprirebbero l'impianto invece di spiegarlo.
fn show_hovered_name(
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform)>,
    ui_interactions: Query<&Interaction>,
    objects: Query<(Entity, &Placed, &Facing)>,
    names: Query<&PieceName>,
    mut label: Query<(&mut Text2d, &mut Transform, &mut Visibility), With<HoverLabel>>,
) {
    let Ok((mut text, mut transform, mut visibility)) = label.single_mut() else {
        return;
    };

    let hovered =
        editor::cursor_world(&windows, &camera_query, &ui_interactions).and_then(|point| {
            let cell = grid::cell(point);
            editor::clicked_piece(point, cell, || {
                objects
                    .iter()
                    .map(|(entity, placed, facing)| (entity, placed, facing))
            })
            .map(|(entity, _)| (entity, cell))
        });

    let Some((entity, cell)) = hovered else {
        *visibility = Visibility::Hidden;
        return;
    };
    let Ok(name) = names.get(entity) else {
        *visibility = Visibility::Hidden;
        return;
    };

    text.0 = name.0.clone();
    transform.translation =
        (grid::cell_center(cell) + Vec2::Y * (PIECE_SIZE / 2.0 + 10.0)).extend(HOVER_Z);
    *visibility = Visibility::Visible;
}
