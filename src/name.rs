use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;

use crate::editor::Tool;

/// Il numero con cui l'oggetto e' identificato dal programma. E' lui, e non il
/// nome ne' la cella, a comparire nelle registrazioni: il nome si puo' cambiare
/// e la cella si sposta trascinando l'oggetto, l'id no.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct PieceId(pub u32);

/// Il nome con cui l'oggetto si presenta fuori dal programma: sara' lui a
/// comparire nei messaggi mqtt, quindi due oggetti non possono chiamarsi allo
/// stesso modo, e nessuno puo' restare senza.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub struct PieceName(pub String);

/// Chi e' un oggetto: il numero per il programma, il nome per le persone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    pub id: u32,
    pub name: String,
}

/// Il prossimo id: uno piu' del piu' alto in giro. Non si riempiono i buchi
/// lasciati da chi e' stato cancellato, perche' un id riusato farebbe puntare
/// una vecchia registrazione a un oggetto che non e' quello di allora.
pub fn next_free_id(taken: &[u32]) -> u32 {
    taken.iter().copied().max().unwrap_or(0) + 1
}

/// Caratteri ammessi: lettere, cifre, trattino e trattino basso. Un nome
/// finira' in un topic mqtt, dove spazi e segni strani danno solo guai.
fn allowed(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '_'
}

/// Quanto puo' essere lungo un nome. Il limite serve a non ritrovarsi righe
/// illeggibili nel pannello e topic sterminati.
pub const NAME_MAX_LEN: usize = 24;

/// La radice del nome automatico, una per tipo di oggetto.
fn prefix(tool: Tool) -> &'static str {
    match tool {
        Tool::CarrierSource => "sorgente",
        Tool::Gate => "gate",
        Tool::Divert => "divert",
        Tool::Atr => "atr",
        Tool::Despawner => "uscita",
        Tool::Turner => "svolta",
        Tool::Reverser => "inversione",
        Tool::Antenna => "antenna",
        Tool::TubeSensor => "sens-tubo",
        Tool::CarrierSensor => "sens-carrier",
    }
}

/// Il primo nome libero per un oggetto di quel tipo: `gate-1`, `gate-2`, e via
/// cosi'. Nessun oggetto nasce senza nome: senza, non potrebbe parlare.
pub fn next_free(tool: Tool, taken: &[String]) -> String {
    let prefix = prefix(tool);

    (1..)
        .map(|number| format!("{prefix}-{number}"))
        .find(|name| !taken.iter().any(|used| used == name))
        .expect("i numeri non finiscono")
}

/// Vero se il nome si puo' accettare: scritto bene e non gia' di qualcun altro.
pub fn is_acceptable(name: &str, taken: &[String]) -> bool {
    !name.is_empty()
        && name.chars().count() <= NAME_MAX_LEN
        && name.chars().all(allowed)
        && !taken.iter().any(|used| used == name)
}

/// Chi si sta rinominando e che cosa si e' scritto finora. Il nome vero cambia
/// solo alla conferma: finche' si scrive, l'oggetto tiene quello di prima.
#[derive(Resource, Default)]
pub struct Naming {
    pub editing: Option<Entity>,
    pub draft: String,
    /// Vero quando l'ultimo tentativo di conferma e' stato rifiutato.
    pub rejected: bool,
}

impl Naming {
    fn start(&mut self, entity: Entity, current: &str) {
        self.editing = Some(entity);
        self.draft = current.to_string();
        self.rejected = false;
    }

    fn stop(&mut self) {
        self.editing = None;
        self.draft.clear();
        self.rejected = false;
    }
}

/// La scrittura dei nomi. Sta con l'interfaccia perche' senza tastiera e senza
/// pannello non c'e' niente da scrivere; i nomi in se' invece vivono nel
/// layout, e quindi anche senza finestra.
pub struct NamePlugin;

impl Plugin for NamePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Naming>()
            .add_systems(Update, (pick_row, type_name).chain());
    }
}

/// Clic su una riga del pannello: da li' in poi si scrive quel nome.
fn pick_row(
    rows: Query<(&Interaction, &NameRow), Changed<Interaction>>,
    names: Query<&PieceName>,
    mut naming: ResMut<Naming>,
) {
    for (interaction, row) in rows.iter() {
        if *interaction != Interaction::Pressed {
            continue;
        }

        let current = names
            .get(row.0)
            .map(|name| name.0.clone())
            .unwrap_or_default();

        naming.start(row.0, &current);
    }
}

/// La tastiera mentre una riga e' in scrittura. Invio conferma, Esc lascia
/// tutto com'era, e un nome gia' di qualcun altro viene rifiutato invece di
/// creare due oggetti che rispondono allo stesso nome.
fn type_name(
    mut keys: MessageReader<KeyboardInput>,
    mut naming: ResMut<Naming>,
    mut names: Query<(Entity, &mut PieceName)>,
) {
    let Some(editing) = naming.editing else {
        keys.clear();
        return;
    };

    for key in keys.read() {
        if key.state != ButtonState::Pressed {
            continue;
        }

        match &key.logical_key {
            Key::Enter => {
                let taken: Vec<String> = names
                    .iter()
                    .filter(|(entity, _)| *entity != editing)
                    .map(|(_, name)| name.0.clone())
                    .collect();

                if is_acceptable(&naming.draft, &taken) {
                    if let Ok((_, mut name)) = names.get_mut(editing) {
                        name.0 = naming.draft.clone();
                    }
                    naming.stop();
                } else {
                    naming.rejected = true;
                }
            }
            Key::Escape => naming.stop(),
            Key::Backspace => {
                naming.draft.pop();
                naming.rejected = false;
            }
            Key::Character(text) => {
                let room = NAME_MAX_LEN.saturating_sub(naming.draft.chars().count());
                let addition: String = text.chars().filter(|c| allowed(*c)).take(room).collect();

                naming.draft.push_str(&addition);
                naming.rejected = false;
            }
            _ => {}
        }
    }
}

/// Una riga del pannello: porta con se' l'oggetto che rappresenta.
#[derive(Component)]
pub struct NameRow(pub Entity);

#[cfg(test)]
mod tests {
    use super::*;

    /// Gli id non si riusano: uno cancellato lascia il suo numero libero per
    /// sempre, altrimenti una registrazione vecchia finirebbe per riferirsi a
    /// un oggetto diverso da quello che c'era.
    #[test]
    fn ids_are_never_handed_out_twice() {
        assert_eq!(next_free_id(&[]), 1);
        assert_eq!(next_free_id(&[1, 2, 3]), 4);
        assert_eq!(next_free_id(&[1, 3]), 4, "il buco resta un buco");
    }

    /// Ogni oggetto nasce con un nome suo, e il numero e' il primo libero: non
    /// si riparte da capo dopo aver cancellato qualcosa in mezzo.
    #[test]
    fn a_new_object_gets_the_first_free_name() {
        assert_eq!(next_free(Tool::Gate, &[]), "gate-1");

        let taken = vec!["gate-1".to_string(), "gate-3".to_string()];
        assert_eq!(next_free(Tool::Gate, &taken), "gate-2");

        // I conti sono per tipo: un gate non consuma i numeri dell'atr.
        assert_eq!(next_free(Tool::Atr, &taken), "atr-1");
    }

    /// Il nome finisce in un topic mqtt: niente vuoti, niente spazi, niente
    /// doppioni.
    #[test]
    fn a_name_that_could_not_be_a_topic_is_refused() {
        let taken = vec!["gate-1".to_string()];

        assert!(is_acceptable("gate-2", &taken));
        assert!(is_acceptable("uscita_nord", &taken));

        assert!(!is_acceptable("", &taken), "vuoto");
        assert!(!is_acceptable("gate 2", &taken), "con lo spazio");
        assert!(!is_acceptable("gate/2", &taken), "con la barra");
        assert!(!is_acceptable("gate-1", &taken), "gia' di un altro");
        assert!(
            !is_acceptable(&"a".repeat(NAME_MAX_LEN + 1), &taken),
            "lungo"
        );
    }
}
