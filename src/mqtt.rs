//! Il filo con il programma di comando.
//!
//! E' il "domani" che ricorre nei commenti di tutto il repo, e adesso c'e': gli
//! oggetti dicono su mqtt come stanno, e da mqtt si lasciano comandare. Per
//! questo il modulo sta fra i sistemi di logica e non fra quelli del disegno -
//! senza finestra il filo serve piu' che con.
//!
//! Il traffico e' fatto di tre discorsi, e non vanno confusi:
//!
//! - **stato**, `<prefisso>/<nome>/stato`, dal simulatore in fuori. Ritenuto,
//!   cosi' un programma di comando che si collega dopo trova subito com'e'
//!   messo l'impianto invece di dover aspettare che qualcosa cambi.
//! - **lettura**, `<prefisso>/<nome>/lettura`, dal simulatore in fuori. Non
//!   ritenuto: non dice come stanno le cose adesso, annuncia un carrier appena
//!   arrivato fra le mani di un oggetto. Lo stato da solo non basterebbe proprio
//!   perche' e' ritenuto, e un ritenuto tiene solo l'ultimo valore: due carrier
//!   che si susseguono piu' in fretta di quanto il programma di comando legga
//!   diventerebbero uno, e con `--speed 8` non e' un caso di scuola.
//! - **comando**, `<prefisso>/<nome>/comando`, da fuori verso il simulatore.
//!
//! Il nome di un oggetto e' unico e valido come topic - e' la regola che
//! `name::is_acceptable` fa rispettare da prima che mqtt esistesse - quindi non
//! serve nessuna tabella di conversione: il topic *e'* il nome.
//!
//! Il client vive su un thread suo e parla con la simulazione attraverso due
//! canali. Non e' un dettaglio di comodo: un frame non deve mai fermarsi ad
//! aspettare la rete, quindi da qui si pubblica senza bloccare e si legge quello
//! che nel frattempo e' arrivato.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use bevy::prelude::*;
use rumqttc::{Client, Event, Incoming, LastWill, MqttOptions, QoS};
use serde::{Deserialize, Serialize};

use crate::engagement::Engagement;
use crate::name::{PieceId, PieceName};
use crate::switch::Switch;

/// Quanto sta in coda verso la rete prima che si cominci a buttare via. Un
/// impianto grosso che si collega pubblica tutti i suoi oggetti in un frame
/// solo, ed e' quello il momento in cui la coda si riempie.
const OUTGOING_QUEUE: usize = 256;

/// Ogni quanto il client manda un battito quando non ha niente da dire. Sotto il
/// minuto il broker si accorge in fretta di un simulatore caduto.
const KEEP_ALIVE: Duration = Duration::from_secs(30);

/// Il sotto-topic su cui un oggetto racconta come sta, e quello da cui si lascia
/// comandare. Stanno qui come costanti perche' li scrivono in due posti - chi
/// pubblica e chi si iscrive - e due stringhe uguali scritte a mano prima o poi
/// smettono di esserlo.
const STATE: &str = "stato";
const READING: &str = "lettura";
const COMMAND: &str = "comando";

/// Il topic su cui il simulatore dice di esserci. Il "non ci sono" lo scrive il
/// broker per conto suo se il collegamento cade: e' il testamento, ed e' l'unico
/// modo perche' un programma di comando distingua un impianto fermo da un
/// impianto che non risponde piu'.
const PRESENCE: &str = "simulatore/presenza";
const ONLINE: &str = "online";
const OFFLINE: &str = "offline";

/// Come si raggiunge il broker. Arrivano dalla riga di comando e, con la
/// finestra, si cambiano anche dal pannello: e' lo stesso oggetto nei due casi,
/// cosi' non esistono due verita' su dove sia il broker.
#[derive(Resource, Clone, PartialEq, Eq, Debug)]
pub struct MqttSettings {
    pub host: String,
    pub port: u16,
    /// Come il simulatore si presenta al broker. Deve essere unico fra i client
    /// collegati: due che si presentano uguali si buttano fuori a vicenda.
    pub client_id: String,
    /// La radice sotto cui sta tutto l'impianto, senza barra finale.
    pub prefix: String,
    /// Vuoti quando il broker non chiede credenziali, che e' il caso del broker
    /// di prova.
    pub username: String,
    pub password: String,
}

impl Default for MqttSettings {
    fn default() -> Self {
        MqttSettings {
            host: "127.0.0.1".to_string(),
            port: 1883,
            client_id: "simulatore".to_string(),
            prefix: "impianto".to_string(),
            username: String::new(),
            password: String::new(),
        }
    }
}

impl MqttSettings {
    /// Il topic di un oggetto. Il nome non viene ritoccato: e' gia' valido come
    /// topic perche' l'editor non accetta nomi che non lo siano.
    fn topic(&self, name: &str, leaf: &str) -> String {
        format!("{}/{}/{}", self.prefix, name, leaf)
    }

    /// I comandi di chiunque, in un'iscrizione sola: `+` sta per un livello
    /// qualsiasi, cioe' un nome qualsiasi.
    fn command_filter(&self) -> String {
        format!("{}/+/{}", self.prefix, COMMAND)
    }

    fn presence(&self) -> String {
        format!("{}/{}", self.prefix, PRESENCE)
    }

    /// Il nome dell'oggetto dentro un topic di comando, se quel topic e' nostro.
    /// Torna `None` per qualunque cosa non abbia la forma attesa, invece di
    /// tirare a indovinare su un messaggio che non ci riguarda.
    fn named_by(&self, topic: &str) -> Option<String> {
        let rest = topic.strip_prefix(&self.prefix)?.strip_prefix('/')?;
        let name = rest.strip_suffix(COMMAND)?.strip_suffix('/')?;

        (!name.is_empty() && !name.contains('/')).then(|| name.to_string())
    }
}

/// Come sta un oggetto, nella forma in cui viaggia sul filo.
///
/// Porta il nome e l'id anche se il nome e' gia' nel topic: chi riceve puo'
/// tenere da parte il solo messaggio senza doversi ricordare da dove veniva, e
/// l'id e' la cosa stabile - i nomi si possono cambiare dall'editor, gli id no.
#[derive(Serialize, Deserialize, PartialEq, Eq, Debug)]
struct State {
    id: u32,
    name: String,
    /// In servizio. Fuori servizio un oggetto non agisce e non vede niente.
    enabled: bool,
    /// Comandato. Per gli attuatori vuol dire "fai la tua azione", per i sensori
    /// "dichiara una presenza che non c'e'".
    active: bool,
    /// Se in questo istante ha un carrier fra le mani.
    engaged: bool,
    /// Quale. Vuoto quando non ne ha nessuno, ma anche quando `engaged` e' vero
    /// perche' un collaudatore ha acceso l'interruttore per far dichiarare una
    /// presenza che non c'e': li' non esiste nessun carrier da nominare. La
    /// coppia `"engaged": true, "carrier_id": null` e' l'unico modo che il
    /// programma di comando ha di distinguere una lettura vera da una simulata.
    carrier_id: Option<u32>,
}

/// Un carrier appena arrivato fra le mani di un oggetto.
///
/// Non e' uno stato ridetto in altra forma: e' un fatto avvenuto, e per questo
/// `carrier_id` non e' facoltativo come in `State`. Una lettura senza carrier
/// non esiste - se non c'e' nessuno da nominare non c'e' niente da annunciare.
#[derive(Serialize, Deserialize, PartialEq, Eq, Debug)]
struct Reading {
    id: u32,
    name: String,
    carrier_id: u32,
}

/// Quello che il programma di comando puo' cambiare. I due campi sono
/// facoltativi apposta: si comanda un interruttore senza dover ripetere l'altro,
/// e un messaggio vuoto non fa niente invece di azzerare tutto.
#[derive(Serialize, Deserialize, Default, PartialEq, Eq, Debug)]
struct Command {
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    active: Option<bool>,
}

/// Quello che il thread del client ha da dire alla simulazione.
enum FromBroker {
    Connected,
    Disconnected(String),
    /// Un comando per l'oggetto che si chiama cosi'.
    Command(String, Command),
}

/// Come sta il filo, per chi lo guarda dal pannello.
#[derive(Resource, Clone, PartialEq, Eq, Debug, Default)]
pub enum MqttStatus {
    /// Nessuno ha chiesto di collegarsi.
    #[default]
    Off,
    /// Si sta provando, o si e' persa la linea e rumqttc sta riprovando da solo.
    Connecting,
    Connected,
    /// L'ultimo motivo per cui e' caduta. Resta scritto: sapere *perche'* non si
    /// collega e' l'unica cosa che serve davvero quando non si collega.
    Failed(String),
}

/// Il filo aperto: il client con cui si pubblica e la coda di quello che arriva.
/// Assente finche' non ci si collega.
#[derive(Resource)]
pub struct MqttLink {
    client: Client,
    /// Sotto chiave perche' una risorsa Bevy deve poter essere guardata da piu'
    /// thread, e la coda di serie non lo permette. A leggerla e' un sistema
    /// solo, quindi la chiave non e' mai contesa: e' una formalita' del tipo,
    /// non un costo.
    incoming: Mutex<Receiver<FromBroker>>,
    /// Il thread lo guarda per sapere se deve smettere. Senza, chi si scollega
    /// lascerebbe dietro un thread che continua a riprovare per sempre.
    running: Arc<AtomicBool>,
    settings: MqttSettings,
}

impl MqttLink {
    /// I parametri con cui questo filo e' stato aperto. Il pannello li confronta
    /// con quelli scritti adesso: cambiarne uno non sposta un collegamento gia'
    /// aperto, e chi guarda deve poterlo sapere.
    pub fn settings(&self) -> &MqttSettings {
        &self.settings
    }
}

impl Drop for MqttLink {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        // Un saluto prima di sparire: senza, il broker aspetterebbe il keep
        // alive e per un po' direbbe che il simulatore c'e' ancora.
        let _ = self.client.try_publish(
            self.settings.presence(),
            QoS::AtLeastOnce,
            true,
            OFFLINE.as_bytes(),
        );
        let _ = self.client.disconnect();
    }
}

/// Il filo con il programma di comando. Si monta sempre: senza finestra e'
/// l'unico modo che l'impianto ha di farsi comandare.
pub struct MqttPlugin;

impl Plugin for MqttPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MqttStatus>().add_systems(
            Update,
            // In fila: prima si guarda cos'e' arrivato - un comando cambia un
            // interruttore, e il collegamento appena aperto chiede di ridire
            // tutto - poi si pubblica quello che e' cambiato. Al contrario, un
            // comando resterebbe fermo un frame prima di essere raccontato.
            (receive, publish).chain(),
        );
    }
}

/// Apre il filo. Chiude quello di prima, se ce n'era uno.
pub fn connect(commands: &mut Commands, settings: &MqttSettings) {
    let mut options = MqttOptions::new(
        settings.client_id.clone(),
        settings.host.clone(),
        settings.port,
    );
    options.set_keep_alive(KEEP_ALIVE);
    options.set_last_will(LastWill::new(
        settings.presence(),
        OFFLINE,
        QoS::AtLeastOnce,
        true,
    ));
    if !settings.username.is_empty() {
        options.set_credentials(settings.username.clone(), settings.password.clone());
    }

    let (client, mut connection) = Client::new(options, OUTGOING_QUEUE);
    let (sender, incoming) = channel();
    let running = Arc::new(AtomicBool::new(true));

    let stop = running.clone();
    thread::spawn(move || {
        // `iter()` non finisce mai da solo: quando la linea cade rumqttc
        // riprova, ed e' quello che si vuole da un impianto che deve restare
        // agganciato. A farlo smettere e' solo chi si scollega.
        for event in connection.iter() {
            if !stop.load(Ordering::Relaxed) {
                return;
            }

            if relay(&sender, event).is_err() {
                // Dall'altra parte non c'e' piu' nessuno che ascolti: la
                // simulazione e' finita, o il filo e' stato sostituito.
                return;
            }
        }
    });

    commands.insert_resource(MqttLink {
        client,
        incoming: Mutex::new(incoming),
        running,
        settings: settings.clone(),
    });
    commands.insert_resource(MqttStatus::Connecting);
}

/// Chiude il filo. Il `Drop` di `MqttLink` fa il resto.
pub fn disconnect(commands: &mut Commands) {
    commands.remove_resource::<MqttLink>();
    commands.insert_resource(MqttStatus::Off);
}

/// Porta un fatto della rete dentro la coda della simulazione. Separata perche'
/// e' l'unico punto in cui si traduce il vocabolario di rumqttc nel nostro, ed
/// e' anche l'unico da provare.
fn relay(
    sender: &Sender<FromBroker>,
    event: Result<Event, rumqttc::ConnectionError>,
) -> Result<(), std::sync::mpsc::SendError<FromBroker>> {
    match event {
        Ok(Event::Incoming(Incoming::ConnAck(_))) => sender.send(FromBroker::Connected),
        Ok(Event::Incoming(Incoming::Publish(publish))) => {
            // Un messaggio che non si capisce si butta: sul bus puo' arrivare
            // di tutto, e un impianto non si ferma perche' qualcuno ha
            // pubblicato una virgola di troppo.
            let Ok(command) = serde_json::from_slice::<Command>(&publish.payload) else {
                return Ok(());
            };

            sender.send(FromBroker::Command(publish.topic.clone(), command))
        }
        Err(why) => sender.send(FromBroker::Disconnected(why.to_string())),
        _ => Ok(()),
    }
}

/// Di chi era l'ultimo stato pubblicato, e quale. Senza, un oggetto che non
/// cambia verrebbe ripubblicato a ogni frame; con, si scrive sul bus solo cio'
/// che e' successo davvero - la stessa regola delle registrazioni.
///
/// La chiave e' l'entita' e non l'id, che pure sarebbe il nome proprio
/// dell'oggetto: un layout puo' contenere due pezzi con lo stesso id - il file
/// li porta e `layout::fill_identities` non li rinumera - e con quella chiave i
/// due si scavalcavano a vicenda, ripubblicando lo stesso stato per sempre.
#[derive(Resource, Default)]
struct Published(std::collections::HashMap<Entity, State>);

fn receive(
    mut commands: Commands,
    link: Option<Res<MqttLink>>,
    mut status: ResMut<MqttStatus>,
    mut objects: Query<(&PieceName, &mut Switch)>,
) {
    let Some(link) = link else {
        return;
    };

    let Ok(incoming) = link.incoming.lock() else {
        return;
    };

    loop {
        match incoming.try_recv() {
            Ok(FromBroker::Connected) => {
                *status = MqttStatus::Connected;

                if let Err(why) = link
                    .client
                    .try_subscribe(link.settings.command_filter(), QoS::AtLeastOnce)
                {
                    *status = MqttStatus::Failed(why.to_string());
                }
                let _ = link.client.try_publish(
                    link.settings.presence(),
                    QoS::AtLeastOnce,
                    true,
                    ONLINE.as_bytes(),
                );

                // Un collegamento nuovo e' un interlocutore nuovo: quello che
                // sapeva il vecchio non conta piu', e tutto va ridetto. La
                // risorsa si rimette da zero anche se c'era gia', e `publish`
                // la trova svuotata nello stesso frame perche' i due sistemi
                // sono in fila.
                commands.insert_resource(Published::default());
            }
            Ok(FromBroker::Disconnected(why)) => *status = MqttStatus::Failed(why),
            Ok(FromBroker::Command(topic, command)) => {
                let Some(name) = link.settings.named_by(&topic) else {
                    continue;
                };

                for (piece, mut switch) in objects.iter_mut() {
                    if piece.0 != name {
                        continue;
                    }

                    apply(&command, &mut switch);
                }
            }
            Err(TryRecvError::Empty) => return,
            // Il thread se n'e' andato: il filo c'e' ancora come risorsa ma non
            // porta piu' niente.
            Err(TryRecvError::Disconnected) => {
                *status = MqttStatus::Failed("il collegamento si e' chiuso".to_string());
                return;
            }
        }
    }
}

/// Applica un comando a un interruttore, senza toccare quello che il comando non
/// ha nominato. Sta fuori dal sistema per poterlo provare da solo.
fn apply(command: &Command, switch: &mut Switch) {
    if let Some(enabled) = command.enabled
        && switch.enabled != enabled
    {
        switch.enabled = enabled;
    }
    if let Some(active) = command.active
        && switch.active != active
    {
        switch.active = active;
    }
}

fn publish(
    link: Option<Res<MqttLink>>,
    mut published: Option<ResMut<Published>>,
    objects: Query<(Entity, &PieceId, &PieceName, &Switch, Option<&Engagement>)>,
) {
    let (Some(link), Some(published)) = (link, published.as_mut()) else {
        return;
    };

    for (entity, id, name, switch, engagement) in objects.iter() {
        let state = State {
            id: id.0,
            name: name.0.clone(),
            enabled: switch.enabled,
            active: switch.active,
            engaged: engagement.is_some_and(|engagement| engagement.engaged),
            carrier_id: engagement.and_then(|engagement| engagement.carrier_id),
        };

        let previous = published.0.get(&entity);
        if previous == Some(&state) {
            continue;
        }

        let reading = read_by(previous, &state);

        let Ok(payload) = serde_json::to_vec(&state) else {
            continue;
        };

        // `try_` e non `publish`: quello aspetterebbe che si liberi posto in
        // coda, e un frame non si ferma per la rete. Se la coda e' piena il
        // messaggio si perde, ma lo stato e' ritenuto e il prossimo cambiamento
        // lo rimette a posto.
        if link
            .client
            .try_publish(
                link.settings.topic(&name.0, STATE),
                QoS::AtMostOnce,
                true,
                payload,
            )
            .is_ok()
        {
            published.0.insert(entity, state);

            // Solo dopo che lo stato e' passato: se fallisse, `Published`
            // resterebbe ferma a quello di prima e il frame dopo troveremmo di
            // nuovo lo stesso fronte, annunciando due volte una lettura sola.
            if let Some(carrier_id) = reading {
                announce(&link, id.0, &name.0, carrier_id);
            }
        }
    }
}

/// Il carrier di cui annunciare la lettura, se ce n'e' uno.
///
/// Regola, non travaso: sta fuori dal sistema perche' e' l'unica parte di
/// `publish` che si puo' provare senza un broker, ed e' quella dove si sbaglia.
///
/// Lo stato di prima deve esistere. A filo appena aperto `Published` viene
/// svuotata apposta, e senza questa condizione ogni riconnessione spaccerebbe
/// per lettura appena avvenuta il carrier che in quel momento sta fermo
/// sull'antenna - una lettura che non e' successa adesso e forse nemmeno oggi.
fn read_by(previous: Option<&State>, state: &State) -> Option<u32> {
    match (previous, state.carrier_id) {
        (Some(previous), Some(carrier_id)) if previous.carrier_id != Some(carrier_id) => {
            Some(carrier_id)
        }
        _ => None,
    }
}

/// Manda in giro una lettura. `AtLeastOnce` e senza ritenuta, al contrario dello
/// stato: un fatto avvenuto che si perde non lo rimette a posto nessuno, e
/// lasciarlo ritenuto farebbe credere a chi si collega domani che sia appena
/// successo.
fn announce(link: &MqttLink, id: u32, name: &str, carrier_id: u32) {
    let reading = Reading {
        id,
        name: name.to_string(),
        carrier_id,
    };

    let Ok(payload) = serde_json::to_vec(&reading) else {
        return;
    };

    let _ = link.client.try_publish(
        link.settings.topic(name, READING),
        QoS::AtLeastOnce,
        false,
        payload,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> MqttSettings {
        MqttSettings {
            prefix: "impianto".to_string(),
            ..Default::default()
        }
    }

    /// Il topic e' il nome, senza tabelle di mezzo: e' il motivo per cui
    /// l'editor pretende nomi validi come topic fin da prima che mqtt ci fosse.
    #[test]
    fn the_topic_of_an_object_is_its_name() {
        let settings = settings();

        assert_eq!(settings.topic("gate-1", STATE), "impianto/gate-1/stato");
        assert_eq!(settings.topic("gate-1", READING), "impianto/gate-1/lettura");
        assert_eq!(settings.command_filter(), "impianto/+/comando");
    }

    /// La strada di ritorno: dal topic di un comando si ricava di chi era.
    #[test]
    fn the_name_comes_back_out_of_a_command_topic() {
        let settings = settings();

        assert_eq!(
            settings.named_by("impianto/gate-1/comando").as_deref(),
            Some("gate-1")
        );
        assert_eq!(
            settings.named_by("impianto/gate-1/stato"),
            None,
            "lo stato lo scriviamo noi: non e' un comando"
        );
        assert_eq!(
            settings.named_by("altro/gate-1/comando"),
            None,
            "un altro impianto sullo stesso broker non ci riguarda"
        );
        assert_eq!(
            settings.named_by("impianto/comando"),
            None,
            "senza nome in mezzo non si sa a chi obbedire"
        );
    }

    /// Un comando nomina solo quello che vuole cambiare: comandare un gate non
    /// deve rimetterlo in servizio per il fatto di non aver detto niente.
    #[test]
    fn a_command_only_touches_what_it_names() {
        let mut switch = Switch {
            enabled: false,
            active: false,
        };

        apply(
            &Command {
                active: Some(true),
                enabled: None,
            },
            &mut switch,
        );

        assert!(switch.active, "l'ha nominato");
        assert!(!switch.enabled, "e questo no, quindi resta com'era");
    }

    /// Il messaggio vuoto e' legittimo e non fa niente: `{}` non e' un errore da
    /// rifiutare, e' un comando che non chiede nulla.
    #[test]
    fn an_empty_command_is_understood_and_does_nothing() {
        let command: Command = serde_json::from_str("{}").expect("e' un comando valido");

        assert_eq!(command, Command::default());
    }

    /// La forma dello stato sul filo. E' un contratto con un programma che non
    /// e' scritto in Rust: cambiarlo di nascosto vuol dire romperlo.
    #[test]
    fn the_state_travels_as_flat_json() {
        let state = State {
            id: 7,
            name: "gate-1".to_string(),
            enabled: true,
            active: false,
            engaged: true,
            carrier_id: None,
        };

        assert_eq!(
            serde_json::to_string(&state).expect("si scrive"),
            r#"{"id":7,"name":"gate-1","enabled":true,"active":false,"engaged":true,"carrier_id":null}"#
        );
    }

    /// L'altra meta' del contratto: con un carrier fra le mani l'id viaggia come
    /// numero e non come stringa. Il test sopra non lo copre, perche' li' il
    /// campo e' vuoto e `null` non dice di che tipo sarebbe stato.
    #[test]
    fn the_state_carries_the_id_of_the_carrier_it_is_holding() {
        let state = State {
            id: 7,
            name: "gate-1".to_string(),
            enabled: true,
            active: false,
            engaged: true,
            carrier_id: Some(1),
        };

        assert_eq!(
            serde_json::to_string(&state).expect("si scrive"),
            r#"{"id":7,"name":"gate-1","enabled":true,"active":false,"engaged":true,"carrier_id":1}"#
        );
    }

    /// La forma di una lettura sul filo, contratto quanto quella dello stato.
    /// Qui `carrier_id` e' un numero e basta: una lettura senza carrier non
    /// esiste, e il tipo lo dice al posto di un commento.
    #[test]
    fn a_reading_travels_as_flat_json() {
        let reading = Reading {
            id: 7,
            name: "antenna-1".to_string(),
            carrier_id: 3,
        };

        assert_eq!(
            serde_json::to_string(&reading).expect("si scrive"),
            r#"{"id":7,"name":"antenna-1","carrier_id":3}"#
        );
    }

    fn holding(carrier_id: Option<u32>) -> State {
        State {
            id: 7,
            name: "antenna-1".to_string(),
            enabled: true,
            active: false,
            engaged: carrier_id.is_some(),
            carrier_id,
        }
    }

    /// Si annuncia il fronte di salita, non lo stato: un carrier che arriva, e
    /// una volta sola per quanto a lungo resti li' sotto.
    #[test]
    fn a_reading_is_announced_when_a_carrier_arrives() {
        let empty = holding(None);
        let first = holding(Some(1));

        assert_eq!(
            read_by(Some(&empty), &first),
            Some(1),
            "e' appena arrivato: questa e' la lettura"
        );
        assert_eq!(
            read_by(Some(&first), &first),
            None,
            "e' sempre lo stesso: la lettura e' gia' stata annunciata"
        );
        assert_eq!(
            read_by(Some(&first), &empty),
            None,
            "andarsene non e' una lettura"
        );
    }

    /// Due carrier che si succedono senza un istante di vuoto in mezzo: e' il
    /// caso per cui il topic esiste. Lo stato ritenuto da solo terrebbe solo
    /// l'ultimo, e chi legge piano vedrebbe passare un carrier invece di due.
    #[test]
    fn a_carrier_replacing_another_is_a_reading_of_its_own() {
        assert_eq!(read_by(Some(&holding(Some(1))), &holding(Some(2))), Some(2));
    }

    /// A filo appena aperto non si annuncia niente. `Published` viene svuotata a
    /// ogni collegamento, e senza questa regola una riconnessione spaccerebbe
    /// per lettura appena avvenuta il carrier fermo sull'antenna da un'ora.
    #[test]
    fn a_fresh_link_announces_no_readings() {
        assert_eq!(read_by(None, &holding(Some(1))), None);
    }

    /// Un comando che nomina un campo solo si legge lo stesso: e' la forma piu'
    /// probabile in arrivo da un programma di comando vero.
    #[test]
    fn a_partial_command_is_read_without_the_other_field() {
        let command: Command =
            serde_json::from_str(r#"{"active":false}"#).expect("e' un comando valido");

        assert_eq!(command.active, Some(false));
        assert_eq!(command.enabled, None);
    }
}
