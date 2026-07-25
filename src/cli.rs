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

    /// Avvia subito la registrazione del filmato. Richiede l'interfaccia:
    /// senza finestra non c'e' niente da riprendere.
    #[arg(long)]
    pub record: bool,

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
