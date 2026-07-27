use bevy::prelude::*;

use crate::carrier::{Carrier, NextCarrierId};
use crate::source::CarrierSource;
use crate::trace::Replay;
use crate::ui::{BUTTON_READY, BUTTON_UNAVAILABLE, button_label, top_button};

/// Mentre gira, il bottone dice "Pausa" ed e' premibile: verde come gli altri
/// comandi disponibili. In pausa resta l'arancio, che segnala uno stato in
/// corso invece della semplice disponibilita'. Lo usa anche la velocita', per
/// dire che l'orologio non e' piu' quello vero.
const PAUSED_COLOR: Color = Color::srgb(0.75, 0.45, 0.10);

/// Le andature fra cui gira il bottone. Non e' una scala continua: servono
/// pochi passi riconoscibili, non una manopola.
const SPEEDS: [f32; 4] = [1.0, 2.0, 4.0, 8.0];

/// Piu' veloce di cosi' non si va, e non e' un limite di comodo. In un frame il
/// carrier percorre `BELT_SPEED * delta * velocita'`: a sedici volte sono 27 px,
/// ancora meno della fascia in cui un gate lo ferma (38) e della finestra in cui
/// un deviatore lo aggancia (48). Piu' su comincerebbe a scavalcare gli oggetti
/// invece di incontrarli, e un impianto che si guarda al doppio della velocita'
/// non varrebbe un carrier che attraversa una sbarra chiusa.
pub const MAX_SPEED: f32 = 16.0;

/// Quanto tempo simulato puo' passare in un frame solo. Bevy ne ha gia' un
/// limite (un quarto di secondo) che serve a non far esplodere il mondo dopo
/// una pausa del sistema operativo; qui lo si divide per l'andatura, cosi' il
/// **passo** piu' lungo possibile resta lo stesso a qualunque velocita'. Senza,
/// accelerare moltiplicherebbe anche i salti, che e' proprio cio' che il limite
/// esisteva per evitare.
const LONGEST_STEP: f32 = 0.25;

/// Cambia l'andatura dell'orologio simulato. Tutto qui dentro si muove su
/// `Res<Time>` - carrier, sorgenti, registrazione, riproduzione - quindi si
/// accelera in un punto solo, e niente ha bisogno di sapere che sta correndo.
pub fn set_speed(clock: &mut Time<Virtual>, speed: f32) {
    let wanted = speed;
    let speed = speed.clamp(1.0, MAX_SPEED);
    if speed != wanted {
        warn!("velocita' {wanted}x riportata a {speed}x");
    }

    clock.set_relative_speed(speed);
    clock.set_max_delta(std::time::Duration::from_secs_f32(LONGEST_STEP / speed));
}

/// In che modalita' e' il programma. Sono due mestieri diversi con gli stessi
/// due tasti del mouse: nell'editor si costruisce l'impianto, in simulazione lo
/// si comanda. Tenerli separati e' quello che libera il tasto destro, altrimenti
/// occupato dalla rotazione.
///
/// Sta qui, accanto all'altro stato del programma, per due motivi: i due si
/// intrecciano (passando in editor il tempo si ferma e il nastro si svuota), e
/// meta' dei moduli deve sapere in che modo si e' senza per questo dover
/// dipendere dall'editor. A registrarlo e' pero' `EditorPlugin`: senza finestra
/// non c'e' niente da costruire, quindi non c'e' nemmeno un modo in cui stare.
#[derive(States, Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum Mode {
    #[default]
    Editing,
    Simulating,
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Mode::Editing => "Editor",
            Mode::Simulating => "Simulazione",
        }
    }

    pub fn other(self) -> Self {
        match self {
            Mode::Editing => Mode::Simulating,
            Mode::Simulating => Mode::Editing,
        }
    }
}

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
                setup_speed_button,
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
                change_speed,
                refresh_speed_button,
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

#[derive(Component)]
struct SpeedButton;

#[derive(Component)]
struct SpeedLabel;

fn setup_speed_button(mut commands: Commands) {
    commands.spawn((
        top_button(6),
        BackgroundColor(BUTTON_READY),
        SpeedButton,
        children![(button_label("1x"), SpeedLabel)],
    ));
}

/// Il bottone gira sulle andature, e da quella piu' alta torna al passo vero.
/// Non si ferma sull'ultima: chi ha accelerato per arrivare in fondo a un giro
/// vuole quasi sempre tornare a guardare a velocita' normale.
fn change_speed(
    buttons: Query<&Interaction, (Changed<Interaction>, With<SpeedButton>)>,
    mut clock: ResMut<Time<Virtual>>,
) {
    let pressed = buttons
        .iter()
        .any(|interaction| *interaction == Interaction::Pressed);

    if !pressed {
        return;
    }

    let now = clock.relative_speed();
    let next = SPEEDS
        .iter()
        .find(|speed| **speed > now + 0.01)
        .copied()
        .unwrap_or(SPEEDS[0]);

    set_speed(&mut clock, next);
    info!("velocita' della simulazione: {next}x");
}

/// L'andatura si legge sul bottone, e il colore dice se l'orologio e' quello
/// vero: accelerare e' uno stato in corso, come la pausa.
fn refresh_speed_button(
    clock: Res<Time<Virtual>>,
    mut buttons: Query<&mut BackgroundColor, With<SpeedButton>>,
    mut labels: Query<&mut Text, With<SpeedLabel>>,
    // Quella gia' scritta sul bottone. Non si puo' guardare `is_changed`:
    // l'orologio cambia a ogni frame per mestiere, e si riscriverebbe
    // un'etichetta identica sessanta volte al secondo. Parte da zero, che non e'
    // un'andatura possibile, quindi la prima volta scrive comunque - ed e' cosi'
    // che il bottone nasce gia' giusto quando la velocita' arriva da `--speed`.
    mut shown: Local<f32>,
) {
    let speed = clock.relative_speed();
    if *shown == speed {
        return;
    }
    *shown = speed;

    let colour = if speed > 1.0 {
        PAUSED_COLOR
    } else {
        BUTTON_READY
    };

    for mut background in buttons.iter_mut() {
        background.0 = colour;
    }
    for mut label in labels.iter_mut() {
        label.0 = format!("{speed}x");
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Accelerare non deve allungare il singolo passo: il limite di Bevy sul
    /// tempo che puo' passare in un frame viene diviso per l'andatura, cosi' il
    /// tratto piu' lungo che un carrier percorre in un colpo resta quello di
    /// sempre. Senza, andare veloci vorrebbe dire anche saltare piu' lontano, e
    /// un carrier potrebbe scavalcare la sbarra che dovrebbe fermarlo.
    #[test]
    fn going_faster_never_makes_a_single_step_longer() {
        let mut clock = Time::<Virtual>::default();

        for speed in SPEEDS {
            set_speed(&mut clock, speed);

            assert_eq!(clock.relative_speed(), speed);
            assert!(
                (clock.max_delta().as_secs_f32() * speed - LONGEST_STEP).abs() < 0.001,
                "a {speed}x il passo massimo e' cambiato"
            );
        }
    }

    /// Un'andatura assurda non viene rifiutata ma riportata nei limiti: chi
    /// scrive un numero enorme vuole "il piu' veloce possibile", e fermarsi con
    /// un errore non lo aiuterebbe. Sotto, il tempo non va all'indietro.
    #[test]
    fn an_absurd_pace_is_brought_back_within_the_limits() {
        let mut clock = Time::<Virtual>::default();

        set_speed(&mut clock, 1000.0);
        assert_eq!(clock.relative_speed(), MAX_SPEED);

        set_speed(&mut clock, -3.0);
        assert_eq!(clock.relative_speed(), 1.0);
    }
}
