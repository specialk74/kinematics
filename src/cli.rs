use clap::Parser;

use crate::layout::LayoutFile;
use crate::mqtt::MqttSettings;

/// Come e' stato lanciato il programma. Le opzioni si riconoscono dal nome, non
/// dalla posizione: l'ordine in cui si scrivono non conta.
// Niente `Eq`: l'andatura e' un numero con la virgola, e fra due di quelli
// l'uguaglianza esatta non e' una relazione su cui si possa contare.
#[derive(Parser, Debug, PartialEq)]
#[command(version, about = "Simulatore di flusso carrier")]
pub struct Options {
    /// File di layout da aprire all'avvio. Diventa anche il bersaglio dei
    /// bottoni Salva e Carica. Senza, si parte a scena vuota su layout.ron.
    #[arg(short, long, value_name = "FILE")]
    layout: Option<String>,

    /// Comincia subito a registrare le posizioni dei carrier.
    #[arg(long)]
    pub record: bool,

    /// Riproduce una registrazione invece di simulare. Il file porta con se'
    /// anche l'impianto, quindi non serve indicare un layout.
    // Riprodurre serve a guardare cosa e' successo: senza finestra non
    // resterebbe niente da guardare. La combinazione viene rifiutata subito
    // invece di girare a vuoto.
    #[arg(long, value_name = "FILE", conflicts_with = "hide_gui")]
    pub replay: Option<String>,

    /// Fa girare la simulazione senza finestra, fino a Ctrl+C.
    // Il nome con trattino basso e' quello richiesto; clap lo trasformerebbe in
    // `--hide-gui`, che teniamo come alias perche' e' la forma che uno si aspetta.
    #[arg(long = "hide_gui", alias = "hide-gui")]
    pub hide_gui: bool,

    /// Quante volte piu' veloce del tempo vero deve andare la simulazione.
    /// Con la finestra si cambia anche dal bottone; senza, questo e' l'unico
    /// modo - ed e' li' che serve di piu', perche' un impianto lungo si fa
    /// percorrere tutto in un minuto invece che in dieci.
    // Il valore viene comunque riportato entro il massimo: oltre, il carrier
    // farebbe passi piu' lunghi degli oggetti che deve incontrare.
    #[arg(long, value_name = "VOLTE", default_value_t = 1.0)]
    pub speed: f32,

    /// Si collega al broker mqtt all'avvio. Senza, il filo resta chiuso e con
    /// la finestra lo si apre dal pannello: e' l'unico modo che ha chi lancia
    /// senza finestra, ed e' li' che serve, perche' headless il simulatore
    /// esiste per farsi comandare da fuori.
    #[arg(long)]
    pub mqtt: bool,

    /// Dove sta il broker.
    #[arg(long = "mqtt-host", value_name = "HOST", default_value = "127.0.0.1")]
    pub mqtt_host: String,

    #[arg(long = "mqtt-port", value_name = "PORTA", default_value_t = 1883)]
    pub mqtt_port: u16,

    /// Con che nome il simulatore si presenta al broker. Due client che si
    /// presentano uguali si buttano fuori a vicenda, quindi va cambiato se se ne
    /// lanciano due sulla stessa rete.
    #[arg(long = "mqtt-id", value_name = "NOME", default_value = "simulatore")]
    pub mqtt_id: String,

    /// La radice dei topic sotto cui sta tutto l'impianto.
    #[arg(
        long = "mqtt-prefix",
        value_name = "PREFISSO",
        default_value = "impianto"
    )]
    pub mqtt_prefix: String,

    /// Credenziali, se il broker le chiede. Quello di prova no.
    #[arg(long = "mqtt-user", value_name = "UTENTE", default_value = "")]
    pub mqtt_user: String,

    #[arg(long = "mqtt-password", value_name = "PAROLA", default_value = "")]
    pub mqtt_password: String,
}

impl Options {
    pub fn layout_file(&self) -> LayoutFile {
        LayoutFile::new(self.layout.clone())
    }

    /// I parametri di connessione come li vuole il modulo mqtt. Sono gli stessi
    /// che il pannello modifica: la riga di comando decide da dove si parte, non
    /// una verita' separata.
    pub fn mqtt_settings(&self) -> MqttSettings {
        MqttSettings {
            host: self.mqtt_host.clone(),
            port: self.mqtt_port,
            client_id: self.mqtt_id.clone(),
            prefix: self.mqtt_prefix.clone(),
            username: self.mqtt_user.clone(),
            password: self.mqtt_password.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::DEFAULT_LAYOUT_PATH;

    fn parse(args: &[&str]) -> Options {
        Options::parse_from(std::iter::once("chapter1").chain(args.iter().copied()))
    }

    /// L'andatura si puo' chiedere dalla riga di comando, e di norma e' quella
    /// vera. Un valore assurdo non viene rifiutato ma riportato entro il
    /// massimo: chi scrive `--speed 100` vuole "il piu' veloce possibile", e
    /// fermarsi con un errore non lo aiuterebbe.
    #[test]
    fn the_pace_can_be_asked_for_from_the_command_line() {
        assert_eq!(parse(&["--speed", "8"]).speed, 8.0);
        assert_eq!(parse(&[]).speed, 1.0, "di norma si va al tempo vero");
    }

    #[test]
    fn recording_can_be_asked_for_from_the_start() {
        assert!(parse(&["--record"]).record);
        assert!(!parse(&[]).record, "di norma non si registra");
    }

    #[test]
    fn a_recording_can_be_replayed_from_the_start() {
        let options = parse(&["--replay", "registrazione-1.ron"]);

        assert_eq!(options.replay.as_deref(), Some("registrazione-1.ron"));
        assert_eq!(parse(&[]).replay, None);
    }

    /// Registrare senza finestra ha senso: la simulazione gira lo stesso e il
    /// file resta. Riprodurre senza finestra no, e viene detto invece di
    /// lasciare il programma a girare per niente.
    #[test]
    fn replaying_without_a_window_is_refused_while_recording_is_not() {
        assert!(Options::try_parse_from(["chapter1", "--hide_gui", "--record"]).is_ok());
        assert!(
            Options::try_parse_from(["chapter1", "--hide_gui", "--replay", "r.ron"]).is_err(),
            "senza finestra non c'e' niente da guardare"
        );
    }

    #[test]
    fn the_layout_is_named_not_positional() {
        let options = parse(&["--layout", "impianto.ron"]);

        assert_eq!(options.layout_file().path, "impianto.ron");
        assert!(!options.hide_gui);
    }

    /// Il punto di tutto l'esercizio: l'ordine non conta piu'.
    #[test]
    fn the_order_of_the_options_does_not_matter() {
        let first = parse(&["--hide_gui", "--layout", "impianto.ron"]);
        let then = parse(&["--layout", "impianto.ron", "--hide_gui"]);

        assert_eq!(first, then);
        assert!(first.hide_gui);
        assert_eq!(first.layout_file().path, "impianto.ron");
    }

    #[test]
    fn the_short_form_and_the_dashed_alias_work_too() {
        let options = parse(&["-l", "impianto.ron", "--hide-gui"]);

        assert!(options.hide_gui);
        assert_eq!(options.layout_file().path, "impianto.ron");
    }

    #[test]
    fn without_options_the_scene_starts_empty() {
        let layout = parse(&[]).layout_file();

        assert_eq!(layout.path, DEFAULT_LAYOUT_PATH);
        assert!(!layout.load_at_startup);
    }

    /// La vecchia forma posizionale non e' piu' accettata, e viene segnalata
    /// invece di essere ignorata in silenzio.
    #[test]
    fn a_bare_file_name_is_rejected() {
        let refused = Options::try_parse_from(["chapter1", "impianto.ron"]);

        assert!(refused.is_err());
    }
}
