use bevy::prelude::*;

use crate::carrier::{Carrier, NextCarrierId};
use crate::editor::{BUTTON_READY, BUTTON_UNAVAILABLE, Mode, button_label, top_button};
use crate::source::CarrierSource;
use crate::trace::Replay;

/// Mentre gira, il bottone dice "Pausa" ed e' premibile: verde come gli altri
/// comandi disponibili. In pausa resta l'arancio, che segnala uno stato in
/// corso invece della semplice disponibilita'.
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
        app.init_state::<SimulationState>()
            .init_resource::<HasRun>();
    }
}

/// Il bottone play/pausa, cioe' il modo umano di cambiare quello stato.
pub struct SimulationControlsPlugin;

impl Plugin for SimulationControlsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Startup,
            (
                setup_pause_button,
                setup_restart_button,
                hold_still_in_editor,
            ),
        )
        // Passando all'editor il tempo si ferma: si costruisce l'impianto, non
        // lo si guarda funzionare.
        .add_systems(
            OnEnter(Mode::Editing),
            (hold_still_in_editor, clear_the_belt),
        )
        // Basta che sia partita una volta perche' ci sia qualcosa da riavviare.
        .add_systems(OnEnter(SimulationState::Running), remember_it_ran)
        .add_systems(
            Update,
            (
                toggle_simulation,
                refresh_pause_button,
                restart_simulation,
                refresh_restart_button,
            ),
        );
    }
}

fn setup_pause_button(mut commands: Commands) {
    commands.spawn((
        top_button(0),
        BackgroundColor(BUTTON_UNAVAILABLE),
        PauseButton,
        children![(button_label("Pausa"), PauseLabel)],
    ));
}

fn setup_restart_button(mut commands: Commands) {
    commands.spawn((
        top_button(2),
        BackgroundColor(BUTTON_UNAVAILABLE),
        RestartButton,
        children![button_label("Riavvia")],
    ));
}

/// Se la simulazione e' stata avviata almeno una volta da quando e' stata
/// sgombrata. Prima di allora non c'e' niente da far ripartire.
#[derive(Resource, Default)]
pub struct HasRun(bool);

/// Riavviare ha senso solo dove c'e' traffico da svuotare: in simulazione, e
/// solo dopo che e' partita. In editor il tempo e' fermo, e durante una
/// riproduzione i carrier arrivano dal file, quindi toglierli non direbbe niente.
fn restart_available(mode: &State<Mode>, state: &State<SimulationState>, has_run: &HasRun) -> bool {
    has_run.0 && *mode.get() == Mode::Simulating && *state.get() != SimulationState::Replaying
}

fn refresh_restart_button(
    mode: Res<State<Mode>>,
    state: Res<State<SimulationState>>,
    has_run: Res<HasRun>,
    mut buttons: Query<&mut BackgroundColor, With<RestartButton>>,
) {
    if !mode.is_changed() && !state.is_changed() && !has_run.is_changed() {
        return;
    }

    let colour = if restart_available(&mode, &state, &has_run) {
        BUTTON_READY
    } else {
        BUTTON_UNAVAILABLE
    };

    for mut background in buttons.iter_mut() {
        background.0 = colour;
    }
}

/// Svuota il nastro e riparte da capo: spariscono i carrier, la numerazione
/// ricomincia da 1 e le sorgenti riazzerano l'attesa. Gli oggetti restano dove
/// sono e come sono, accesi o spenti: si riavvia il traffico, non l'impianto.
fn restart_simulation(
    mut commands: Commands,
    buttons: Query<&Interaction, (Changed<Interaction>, With<RestartButton>)>,
    mode: Res<State<Mode>>,
    state: Res<State<SimulationState>>,
    mut has_run: ResMut<HasRun>,
    carriers: Query<Entity, With<Carrier>>,
    mut sources: Query<&mut CarrierSource>,
    mut ids: ResMut<NextCarrierId>,
) {
    let pressed = buttons
        .iter()
        .any(|interaction| *interaction == Interaction::Pressed);

    if !pressed || !restart_available(&mode, &state, &has_run) {
        return;
    }

    // Sgombrata la scena non c'e' piu' niente da far ripartire, finche' non si
    // preme di nuovo Play.
    has_run.0 = false;

    for entity in carriers.iter() {
        commands.entity(entity).despawn();
    }
    for mut source in sources.iter_mut() {
        source.restart();
    }
    *ids = NextCarrierId::default();

    info!("simulazione riavviata");
}

/// Mette in pausa se si e' in editor. Vale anche all'avvio, che in editor ci
/// nasce: un layout caricato da riga di comando resta immobile finche' non si
/// passa in simulazione.
fn hold_still_in_editor(
    mode: Res<State<Mode>>,
    state: Res<State<SimulationState>>,
    mut next_state: ResMut<NextState<SimulationState>>,
) {
    // Una riproduzione in corso non la si tocca: quella ha il suo Stop.
    if *mode.get() == Mode::Editing && *state.get() == SimulationState::Running {
        next_state.set(SimulationState::Paused);
    }
}

fn remember_it_ran(mut has_run: ResMut<HasRun>) {
    has_run.0 = true;
}

/// Tornando all'editor la scena si sgombra: i carrier in giro sono il risultato
/// di una simulazione, e mentre si rimette mano all'impianto non hanno piu'
/// niente a che vedere con quello che si sta costruendo.
fn clear_the_belt(
    mut commands: Commands,
    mut has_run: ResMut<HasRun>,
    carriers: Query<Entity, With<Carrier>>,
    mut sources: Query<&mut CarrierSource>,
) {
    for entity in carriers.iter() {
        commands.entity(entity).despawn();
    }
    for mut source in sources.iter_mut() {
        source.restart();
    }

    has_run.0 = false;
}

fn toggle_simulation(
    interactions: Query<&Interaction, (Changed<Interaction>, With<PauseButton>)>,
    mode: Res<State<Mode>>,
    state: Res<State<SimulationState>>,
    mut next_state: ResMut<NextState<SimulationState>>,
    mut replay: ResMut<Replay>,
) {
    // In editor il Play non fa niente: per far muovere i carrier si passa in
    // simulazione. Una riproduzione invece si puo' fermare da qualunque modo.
    if *mode.get() == Mode::Editing && *state.get() != SimulationState::Replaying {
        return;
    }

    for interaction in interactions.iter() {
        if *interaction != Interaction::Pressed {
            continue;
        }

        match state.get() {
            // Durante una riproduzione il tasto ferma il nastro dov'e', non la
            // simulazione: per uscire dalla riproduzione c'e' il suo Stop.
            SimulationState::Replaying => replay.toggle_pause(),
            SimulationState::Running => next_state.set(SimulationState::Paused),
            SimulationState::Paused => next_state.set(SimulationState::Running),
        }
    }
}

/// Il bottone mostra l'azione che compie, non lo stato in cui si trova.
fn refresh_pause_button(
    mode: Res<State<Mode>>,
    state: Res<State<SimulationState>>,
    replay: Res<Replay>,
    mut buttons: Query<&mut BackgroundColor, With<PauseButton>>,
    mut labels: Query<&mut Text, With<PauseLabel>>,
) {
    if !state.is_changed() && !replay.is_changed() && !mode.is_changed() {
        return;
    }

    // In editor il bottone resta spento: dice che li' non c'e' niente da far
    // partire, invece di far credere a un Play che non parte.
    if *mode.get() == Mode::Editing && *state.get() != SimulationState::Replaying {
        for mut background in buttons.iter_mut() {
            background.0 = BUTTON_UNAVAILABLE;
        }
        for mut label in labels.iter_mut() {
            label.0 = "Play".to_string();
        }
        return;
    }

    let (colour, text) = match state.get() {
        SimulationState::Running => (BUTTON_READY, "Pausa"),
        SimulationState::Paused => (PAUSED_COLOR, "Play"),
        SimulationState::Replaying if replay.is_paused() => (PAUSED_COLOR, "Riprendi"),
        SimulationState::Replaying => (BUTTON_READY, "Pausa"),
    };

    for mut background in buttons.iter_mut() {
        background.0 = colour;
    }
    for mut label in labels.iter_mut() {
        label.0 = text.to_string();
    }
}
