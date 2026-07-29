//! Il pannello del collegamento: dove sta il broker e se ci si e' agganciati.
//!
//! Gli stessi parametri arrivano dalla riga di comando (`cli::Options`), e non
//! sono due verita' diverse: `MqttSettings` e' una sola, la riga di comando
//! decide da dove si parte e qui la si cambia. Chi lancia senza finestra ha solo
//! la riga di comando, ed e' giusto - headless non c'e' nessuno che possa
//! cliccare.
//!
//! Il collegamento non si riapre da solo quando si cambia un parametro: sarebbe
//! comodo per una lettera e insopportabile per un indirizzo, che si scrive un
//! carattere per volta e per meta' del tempo non e' ancora un indirizzo. Si
//! preme Riconnetti, e finche' non lo si fa il pannello dice che quello che c'e'
//! scritto non e' quello che si sta usando.

use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;

use crate::mqtt::{self, MqttLink, MqttSettings, MqttStatus};
use crate::ui::BUTTON_IDLE;

const PANEL_WIDTH: f32 = 190.0;
/// Sopra ai tasti della registrazione, che stanno in basso a destra.
const PANEL_BOTTOM: f32 = 76.0;
const PANEL_BACKGROUND: Color = Color::srgba(0.10, 0.10, 0.12, 0.92);
const ROW_HEIGHT: f32 = 20.0;
const ROW_FONT: f32 = 11.0;
const CAPTION_FONT: f32 = 10.0;
const CAPTION_COLOR: Color = Color::srgb(0.55, 0.55, 0.62);
/// La riga che si sta scrivendo, e quella che ha rifiutato quello che c'era
/// scritto: gli stessi due colori del pannello dei nomi, perche' e' lo stesso
/// gesto e chi lo ha imparato la' non deve impararlo due volte.
const ROW_EDITING: Color = Color::srgb(0.25, 0.45, 0.80);
const ROW_REJECTED: Color = Color::srgb(0.70, 0.15, 0.15);
const CONNECTED_COLOR: Color = Color::srgb(0.16, 0.52, 0.24);
const FAILED_COLOR: Color = Color::srgb(0.70, 0.15, 0.15);
const WORKING_COLOR: Color = Color::srgb(0.55, 0.45, 0.15);

/// Un parametro del collegamento, cioe' una riga del pannello.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Field {
    Host,
    Port,
    ClientId,
    Prefix,
    User,
    Password,
}

impl Field {
    /// L'ordine in cui stanno nel pannello: prima dove si va, poi con che nome
    /// ci si presenta, infine le credenziali, che di solito non servono.
    const ALL: [Field; 6] = [
        Field::Host,
        Field::Port,
        Field::ClientId,
        Field::Prefix,
        Field::User,
        Field::Password,
    ];

    fn label(self) -> &'static str {
        match self {
            Field::Host => "Broker",
            Field::Port => "Porta",
            Field::ClientId => "Nome",
            Field::Prefix => "Prefisso",
            Field::User => "Utente",
            Field::Password => "Parola",
        }
    }

    fn read(self, settings: &MqttSettings) -> String {
        match self {
            Field::Host => settings.host.clone(),
            Field::Port => settings.port.to_string(),
            Field::ClientId => settings.client_id.clone(),
            Field::Prefix => settings.prefix.clone(),
            Field::User => settings.username.clone(),
            Field::Password => settings.password.clone(),
        }
    }

    /// Quello che si vede nella riga. La parola d'ordine si copre: il pannello
    /// resta aperto mentre si mostra l'impianto a qualcuno.
    fn shown(self, settings: &MqttSettings) -> String {
        let value = self.read(settings);

        match self {
            Field::Password => "*".repeat(value.chars().count()),
            _ => value,
        }
    }

    /// Prova a scrivere quello che si e' battuto. Torna `false` quando non e' un
    /// valore possibile: la porta e' un numero, e non tutti i numeri.
    fn write(self, settings: &mut MqttSettings, draft: &str) -> bool {
        match self {
            Field::Port => match draft.parse::<u16>() {
                // La porta zero vuol dire "una qualsiasi" per il sistema
                // operativo, che per un indirizzo a cui collegarsi non e' una
                // risposta.
                Ok(port) if port > 0 => {
                    settings.port = port;
                    true
                }
                _ => false,
            },
            // Un indirizzo vuoto non porta da nessuna parte, e un nome vuoto
            // non e' un nome con cui presentarsi.
            Field::Host if draft.is_empty() => false,
            Field::ClientId if draft.is_empty() => false,
            Field::Prefix if draft.is_empty() => false,
            Field::Host => {
                settings.host = draft.to_string();
                true
            }
            Field::ClientId => {
                settings.client_id = draft.to_string();
                true
            }
            Field::Prefix => {
                settings.prefix = draft.to_string();
                true
            }
            // Le credenziali possono essere vuote: e' il caso normale, e vuol
            // dire "il broker non le chiede".
            Field::User => {
                settings.username = draft.to_string();
                true
            }
            Field::Password => {
                settings.password = draft.to_string();
                true
            }
        }
    }

    /// Vero se quel carattere puo' entrare in questo campo. La porta prende
    /// cifre, gli altri qualunque cosa si veda: uno spazio in un indirizzo e' un
    /// errore di battitura, non un indirizzo.
    fn accepts(self, c: char) -> bool {
        match self {
            Field::Port => c.is_ascii_digit(),
            _ => !c.is_whitespace() && !c.is_control(),
        }
    }
}

/// Quale parametro si sta scrivendo e cosa si e' battuto finora. Il valore vero
/// cambia solo alla conferma, come per i nomi: finche' si scrive, il
/// collegamento continua a usare quello di prima.
#[derive(Resource, Default)]
struct Typing {
    field: Option<Field>,
    draft: String,
    /// Vero quando l'ultima conferma e' stata rifiutata.
    rejected: bool,
}

/// Il pannello del collegamento. Si monta solo con la finestra: senza, i
/// parametri li porta la riga di comando e non c'e' niente da mostrare.
pub struct MqttPanelPlugin;

impl Plugin for MqttPanelPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Typing>()
            .add_systems(Startup, setup_panel)
            .add_systems(
                Update,
                // In fila: si raccoglie il gesto, poi si rifa' il pannello con
                // quello che il gesto ha cambiato. Al contrario si vedrebbe
                // sempre lo stato del frame prima.
                (pick_row, type_field, press_link_button, refresh_panel).chain(),
            );
    }
}

#[derive(Component)]
struct MqttPanel;

/// Il tasto che apre e chiude il filo. Uno solo: quello che fa dipende da com'e'
/// il collegamento adesso, e due tasti di cui uno sempre inutile sono peggio.
#[derive(Component)]
struct LinkButton;

fn setup_panel(mut commands: Commands) {
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(PANEL_BOTTOM),
            right: Val::Px(12.0),
            width: Val::Px(PANEL_WIDTH),
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(Val::Px(6.0)),
            row_gap: Val::Px(2.0),
            ..default()
        },
        BackgroundColor(PANEL_BACKGROUND),
        MqttPanel,
    ));
}

/// Clic su una riga: da li' in poi si scrive quel parametro. Un secondo clic
/// sulla stessa riga chiude la scrittura senza confermare, che e' quello che uno
/// si aspetta quando ci ripensa.
fn pick_row(
    rows: Query<(&Interaction, &Field), Changed<Interaction>>,
    settings: Res<MqttSettings>,
    mut typing: ResMut<Typing>,
) {
    for (interaction, field) in rows.iter() {
        if *interaction != Interaction::Pressed {
            continue;
        }

        if typing.field == Some(*field) {
            *typing = Typing::default();
        } else {
            *typing = Typing {
                field: Some(*field),
                draft: field.read(&settings),
                rejected: false,
            };
        }
    }
}

/// La tastiera mentre una riga e' in scrittura. Invio conferma, Esc lascia tutto
/// com'era.
fn type_field(
    mut keys: MessageReader<KeyboardInput>,
    mut typing: ResMut<Typing>,
    mut settings: ResMut<MqttSettings>,
) {
    let Some(field) = typing.field else {
        // Senza una riga aperta i tasti non sono nostri: si scartano, o
        // resterebbero in coda e comparirebbero tutti insieme al primo clic.
        keys.clear();
        return;
    };

    for key in keys.read() {
        if key.state != ButtonState::Pressed {
            continue;
        }

        match &key.logical_key {
            Key::Enter => {
                if field.write(&mut settings, &typing.draft) {
                    *typing = Typing::default();
                } else {
                    typing.rejected = true;
                }
            }
            Key::Escape => *typing = Typing::default(),
            Key::Backspace => {
                typing.draft.pop();
                typing.rejected = false;
            }
            Key::Character(text) => {
                let addition: String = text.chars().filter(|c| field.accepts(*c)).collect();

                typing.draft.push_str(&addition);
                typing.rejected = false;
            }
            _ => {}
        }
    }
}

/// Il tasto del collegamento: apre il filo, o lo chiude se e' aperto.
fn press_link_button(
    mut commands: Commands,
    buttons: Query<&Interaction, (With<LinkButton>, Changed<Interaction>)>,
    link: Option<Res<MqttLink>>,
    settings: Res<MqttSettings>,
) {
    for interaction in buttons.iter() {
        if *interaction != Interaction::Pressed {
            continue;
        }

        if link.is_some() {
            mqtt::disconnect(&mut commands);
        } else {
            mqtt::connect(&mut commands, &settings);
        }
    }
}

/// Cosa dire dello stato del filo, e di che colore.
fn told(status: &MqttStatus) -> (String, Color) {
    match status {
        MqttStatus::Off => ("non collegato".to_string(), BUTTON_IDLE),
        MqttStatus::Connecting => ("in collegamento...".to_string(), WORKING_COLOR),
        MqttStatus::Connected => ("collegato".to_string(), CONNECTED_COLOR),
        // Il motivo per intero, anche se e' lungo: quando non si collega e'
        // l'unica cosa che serve, e riassumerlo vorrebbe dire nasconderlo.
        MqttStatus::Failed(why) => (why.clone(), FAILED_COLOR),
    }
}

fn refresh_panel(
    mut commands: Commands,
    settings: Res<MqttSettings>,
    typing: Res<Typing>,
    status: Res<MqttStatus>,
    link: Option<Res<MqttLink>>,
    panel: Query<(Entity, Option<&Children>), With<MqttPanel>>,
) {
    // Il pannello cambia solo quando cambia qualcosa: rifarlo a ogni frame
    // farebbe sparire il cursore da sotto le dita, ed e' lo stesso motivo per
    // cui l'elenco dei nomi non si rifa' sempre.
    if !settings.is_changed() && !typing.is_changed() && !status.is_changed() {
        return;
    }

    let Ok((panel, children)) = panel.single() else {
        return;
    };

    for child in children.into_iter().flatten() {
        commands.entity(*child).despawn();
    }

    let connected = link.is_some();
    // Quello che c'e' scritto non e' quello che si sta usando: il filo porta con
    // se' i parametri con cui e' stato aperto, e cambiarne uno nel pannello non
    // lo sposta. Dirlo evita il classico "ho cambiato l'indirizzo e non e'
    // successo niente".
    let stale = link.is_some_and(|link| *link.settings() != *settings);
    let (state, state_colour) = told(&status);

    commands.entity(panel).with_children(|panel| {
        panel.spawn((
            Text::new("Collegamento"),
            TextFont {
                font_size: CAPTION_FONT,
                ..default()
            },
            TextColor(CAPTION_COLOR),
        ));

        for field in Field::ALL {
            let editing = typing.field == Some(field);
            let (value, background) = match (editing, typing.rejected) {
                (true, false) => (format!("{}_", typing.draft), ROW_EDITING),
                (true, true) => (format!("{}_", typing.draft), ROW_REJECTED),
                (false, _) => (field.shown(&settings), BUTTON_IDLE),
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
                field,
                children![(
                    Text::new(format!("{} {value}", field.label())),
                    TextFont {
                        font_size: ROW_FONT,
                        ..default()
                    },
                    TextColor(Color::WHITE),
                )],
            ));
        }

        panel.spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(ROW_HEIGHT),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(Val::Px(4.0)),
                ..default()
            },
            BackgroundColor(state_colour),
            children![(
                Text::new(state),
                TextFont {
                    font_size: ROW_FONT,
                    ..default()
                },
                TextColor(Color::WHITE),
            )],
        ));

        panel.spawn((
            Button,
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(ROW_HEIGHT + 4.0),
                flex_shrink: 0.0,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(BUTTON_IDLE),
            LinkButton,
            children![(
                Text::new(match (connected, stale) {
                    (false, _) => "Collega",
                    (true, false) => "Scollega",
                    // Con i parametri cambiati sotto, "scollega" direbbe una
                    // mezza verita': quello che serve e' riaprire il filo.
                    (true, true) => "Riconnetti",
                }),
                TextFont {
                    font_size: ROW_FONT,
                    ..default()
                },
                TextColor(Color::WHITE),
            )],
        ));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// La porta e' un numero, e non tutti: quello che non lo e' viene rifiutato
    /// invece di lasciare il collegamento puntato su niente.
    #[test]
    fn the_port_only_takes_a_real_port() {
        let mut settings = MqttSettings::default();

        assert!(Field::Port.write(&mut settings, "1884"));
        assert_eq!(settings.port, 1884);

        assert!(!Field::Port.write(&mut settings, "0"), "la porta zero no");
        assert!(
            !Field::Port.write(&mut settings, "99999"),
            "oltre il massimo"
        );
        assert!(!Field::Port.write(&mut settings, ""), "e nemmeno il vuoto");
        assert_eq!(settings.port, 1884, "e il valore di prima resta");
    }

    /// Le credenziali vuote sono il caso normale - il broker di prova non le
    /// chiede - mentre un indirizzo vuoto non porta da nessuna parte.
    #[test]
    fn only_the_credentials_may_be_empty() {
        let mut settings = MqttSettings::default();

        assert!(Field::User.write(&mut settings, ""));
        assert!(Field::Password.write(&mut settings, ""));
        assert!(!Field::Host.write(&mut settings, ""));
        assert!(!Field::ClientId.write(&mut settings, ""));
        assert!(!Field::Prefix.write(&mut settings, ""));
    }

    /// Nella porta si battono cifre; negli altri campi tutto tranne gli spazi,
    /// che in un indirizzo sono un errore di battitura e non un indirizzo.
    #[test]
    fn each_field_takes_what_it_can_hold() {
        assert!(Field::Port.accepts('8'));
        assert!(!Field::Port.accepts('a'));
        assert!(Field::Host.accepts('.'));
        assert!(Field::Host.accepts('a'));
        assert!(!Field::Host.accepts(' '));
    }

    /// La parola d'ordine non si legge dal pannello, ma si vede che c'e'.
    #[test]
    fn the_password_is_covered_but_not_hidden() {
        let settings = MqttSettings {
            password: "segreto".to_string(),
            ..Default::default()
        };

        assert_eq!(Field::Password.shown(&settings), "*******");
        assert_eq!(
            Field::Password.read(&settings),
            "segreto",
            "coperta a schermo, non nei parametri"
        );
    }
}
