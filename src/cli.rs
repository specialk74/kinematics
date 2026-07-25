use clap::Parser;

use crate::layout::LayoutFile;

/// Come e' stato lanciato il programma. Le opzioni si riconoscono dal nome, non
/// dalla posizione: l'ordine in cui si scrivono non conta.
#[derive(Parser, Debug, PartialEq, Eq)]
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
}

impl Options {
    pub fn layout_file(&self) -> LayoutFile {
        LayoutFile::new(self.layout.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::DEFAULT_LAYOUT_PATH;

    fn parse(args: &[&str]) -> Options {
        Options::parse_from(std::iter::once("chapter1").chain(args.iter().copied()))
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
