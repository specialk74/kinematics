use bevy::prelude::*;

use crate::carrier::{Carrier, NextCarrierId};
use crate::editor::{BUTTON_IDLE, button_label, top_button};
use crate::source::CarrierSource;

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
    /// Sta rigiocando una registrazione: le posizioni arrivano dal file, non
    /// dal movimento, quindi la simulazione vera deve stare ferma.
    Replaying,
}

#[derive(Component)]
struct PauseButton;

#[derive(Component)]
struct PauseLabel;

#[derive(Component)]
struct RestartButton;

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
        app.add_systems(Startup, (setup_pause_button, setup_restart_button))
            .add_systems(
                Update,
                (toggle_simulation, refresh_pause_button, restart_simulation),
            );
    }
}

fn setup_pause_button(mut commands: Commands) {
    commands.spawn((
        top_button(0),
        BackgroundColor(RUNNING_COLOR),
        PauseButton,
        children![(button_label("Pausa"), PauseLabel)],
    ));
}

fn setup_restart_button(mut commands: Commands) {
    commands.spawn((
        top_button(2),
        BackgroundColor(BUTTON_IDLE),
        RestartButton,
        children![button_label("Riavvia")],
    ));
}

/// Svuota il nastro e riparte da capo: spariscono i carrier, la numerazione
/// ricomincia da 1 e le sorgenti riazzerano l'attesa. Gli oggetti restano dove
/// sono e come sono, accesi o spenti: si riavvia il traffico, non l'impianto.
fn restart_simulation(
    mut commands: Commands,
    buttons: Query<&Interaction, (Changed<Interaction>, With<RestartButton>)>,
    carriers: Query<Entity, With<Carrier>>,
    mut sources: Query<&mut CarrierSource>,
    mut ids: ResMut<NextCarrierId>,
) {
    if !buttons
        .iter()
        .any(|interaction| *interaction == Interaction::Pressed)
    {
        return;
    }

    for entity in carriers.iter() {
        commands.entity(entity).despawn();
    }
    for mut source in sources.iter_mut() {
        source.restart();
    }
    *ids = NextCarrierId::default();

    info!("simulazione riavviata");
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
                // Durante una riproduzione il tasto rimette in moto la
                // simulazione vera, interrompendola.
                SimulationState::Paused | SimulationState::Replaying => SimulationState::Running,
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
