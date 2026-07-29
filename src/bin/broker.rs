//! Il broker di prova: un server MQTT che si lancia e basta.
//!
//! Serve a non dover installare mosquitto per provare il simulatore. Sta in un
//! binario suo e non dentro al simulatore per una ragione sola: il simulatore
//! deve parlare con il broker vero dello stabilimento senza sapere che questo
//! esiste. Se ce lo avesse dentro, prima o poi qualcosa comincerebbe a
//! funzionare solo perche' il broker era in casa.
//!
//! Non condivide codice con il simulatore, e nemmeno il vocabolario dei topic:
//! qui si stampa quello che passa, qualunque cosa sia. E' anche il modo di
//! accorgersi che il simulatore sta pubblicando su un topic diverso da quello
//! che si credeva.
//!
//! Il `link` con cui legge il traffico e' un client interno al broker, e da
//! quello stesso link si puo' anche pubblicare: e' li' che andranno le decisioni,
//! quando il server dovra' comandare l'impianto invece di guardarlo.

use std::thread;

use clap::Parser;
use rumqttd::{Broker, Config, ConnectionSettings, RouterConfig, ServerSettings};

/// Il nome del client interno con cui il broker ascolta se stesso. Compare nei
/// log come un client qualsiasi, e sapere qual e' evita di scambiarlo per il
/// simulatore che si e' appena collegato.
const CONSOLE: &str = "console";

#[derive(Parser, Debug)]
#[command(version, about = "Broker MQTT di prova per il simulatore")]
struct Options {
    /// Su quale indirizzo mettersi in ascolto. Di norma tutti, cosi' ci si
    /// arriva anche da un'altra macchina della rete.
    #[arg(long, default_value = "0.0.0.0")]
    host: String,

    /// La porta. 1883 e' quella di MQTT senza cifratura.
    #[arg(long, default_value_t = 1883)]
    port: u16,

    /// Non stampare i messaggi che passano, solo gli avvisi del broker. Serve
    /// quando l'impianto e' grande e il traffico copre tutto il resto.
    #[arg(long)]
    quiet: bool,
}

fn main() {
    let options = Options::parse();
    let listen = format!("{}:{}", options.host, options.port);

    let Ok(address) = listen.parse() else {
        eprintln!("indirizzo non valido: {listen}");
        std::process::exit(2);
    };

    let mut broker = Broker::new(configuration(address));

    // Il link si prende **prima** di far partire il broker: dopo, `start`
    // non torna piu' indietro.
    let (mut commands, mut traffic) = match broker.link(CONSOLE) {
        Ok(link) => link,
        Err(why) => {
            eprintln!("il broker non si e' aperto: {why}");
            std::process::exit(1);
        }
    };

    thread::spawn(move || {
        if let Err(why) = broker.start() {
            eprintln!("il broker si e' fermato: {why}");
            std::process::exit(1);
        }
    });

    // Tutto, senza scegliere: e' un broker di prova, e quello che si vuole
    // vedere e' proprio cio' che non ci si aspettava.
    if let Err(why) = commands.subscribe("#") {
        eprintln!("non riesco ad ascoltare: {why}");
        std::process::exit(1);
    }

    println!("broker in ascolto su {listen} - Ctrl+C per fermarlo");
    watch(&mut traffic, options.quiet);
}

/// La configurazione minima di un broker: un router e un server MQTT 3.1.1.
/// I numeri sono quelli di serie di rumqttd; stanno qui invece che in un file
/// perche' un broker di prova che chiede un file di configurazione non e' piu'
/// un programma che si lancia e basta.
fn configuration(listen: std::net::SocketAddr) -> Config {
    let connections = ConnectionSettings {
        connection_timeout_ms: 60_000,
        // Un messaggio di stato sono poche decine di byte: 20 KB e' gia'
        // larghissimo, e tiene fuori chi sbaglia bersaglio.
        max_payload_size: 20_480,
        max_inflight_count: 100,
        auth: None,
        external_auth: None,
        // Cosi' un topic nuovo non ha bisogno di essere dichiarato prima:
        // l'impianto puo' cambiare mentre il broker gira.
        dynamic_filters: true,
    };

    let server = ServerSettings {
        name: "v4-1".to_string(),
        listen,
        // Niente TLS: su una rete di stabilimento chiusa non serve, e il
        // simulatore si collega in chiaro come farebbe con mosquitto di serie.
        tls: None,
        next_connection_delay_ms: 1,
        connections,
    };

    Config {
        id: 0,
        router: RouterConfig {
            max_connections: 10_010,
            max_outgoing_packet_count: 200,
            max_segment_size: 104_857_600,
            max_segment_count: 10,
            ..Default::default()
        },
        v4: Some([("1".to_string(), server)].into_iter().collect()),
        ..Default::default()
    }
}

/// Stampa quello che passa, finche' non lo si ferma.
fn watch(traffic: &mut rumqttd::local::LinkRx, quiet: bool) {
    loop {
        let notification = match traffic.recv() {
            Ok(Some(notification)) => notification,
            // Nessuna notifica in questo giro: non e' un errore.
            Ok(None) => continue,
            Err(why) => {
                eprintln!("l'ascolto si e' interrotto: {why}");
                return;
            }
        };

        if let rumqttd::Notification::Forward(forward) = notification {
            if quiet {
                continue;
            }

            let topic = String::from_utf8_lossy(&forward.publish.topic).to_string();
            let payload = String::from_utf8_lossy(&forward.publish.payload).to_string();

            println!("{topic} {payload}");
        }
    }
}
