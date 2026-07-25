use bevy::prelude::*;

const RUNNING_COLOR: Color = Color::srgb(0.20, 0.20, 0.24);
const PAUSED_COLOR: Color = Color::srgb(0.75, 0.45, 0.10);

/// Stato del mondo simulato. In pausa i carrier non si muovono e le sorgenti non
/// emettono: si ferma il tempo, non l'editor, cosi' si puo' preparare il layout
/// a scena immobile.
#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SimulationState {
    #[default]
    Running,
    Paused,
}

#[derive(Component)]
struct PauseButton;

#[derive(Component)]
struct PauseLabel;

/// Solo lo stato: serve anche senza interfaccia, perche' e' quello che i sistemi
/// di simulazione interrogano per sapere se devono girare.
pub struct SimulationPlugin;

impl Plugin for SimulationPlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<SimulationState>();
    }
}

/// Il bottone play/pausa, cioe' il modo umano di cambiare quello stato.
pub struct SimulationControlsPlugin;

impl Plugin for SimulationControlsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_pause_button)
            .add_systems(Update, (toggle_simulation, refresh_pause_button));
    }
}

fn setup_pause_button(mut commands: Commands) {
    commands.spawn((
        Button,
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(12.0),
            right: Val::Px(12.0),
            width: Val::Px(90.0),
            height: Val::Px(36.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(RUNNING_COLOR),
        PauseButton,
        children![(
            Text::new("Pausa"),
            TextFont {
                font_size: 14.0,
                ..default()
            },
            TextColor(Color::WHITE),
            PauseLabel,
        )],
    ));
}

fn toggle_simulation(
    interactions: Query<&Interaction, (Changed<Interaction>, With<PauseButton>)>,
    state: Res<State<SimulationState>>,
    mut next_state: ResMut<NextState<SimulationState>>,
) {
    for interaction in interactions.iter() {
        if *interaction == Interaction::Pressed {
            next_state.set(match state.get() {
                SimulationState::Running => SimulationState::Paused,
                SimulationState::Paused => SimulationState::Running,
            });
        }
    }
}

/// Il bottone mostra l'azione che compie, non lo stato in cui si trova.
fn refresh_pause_button(
    state: Res<State<SimulationState>>,
    mut buttons: Query<&mut BackgroundColor, With<PauseButton>>,
    mut labels: Query<&mut Text, With<PauseLabel>>,
) {
    if !state.is_changed() {
        return;
    }

    let running = *state.get() == SimulationState::Running;

    for mut background in buttons.iter_mut() {
        background.0 = if running { RUNNING_COLOR } else { PAUSED_COLOR };
    }

    for mut label in labels.iter_mut() {
        label.0 = if running { "Pausa" } else { "Play" }.to_string();
    }
}
