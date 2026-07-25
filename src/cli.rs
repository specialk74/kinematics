use crate::layout::LayoutFile;

/// Chiede di far girare la simulazione senza finestra.
pub const HIDE_GUI_FLAG: &str = "hide_gui";

/// Come e' stato lanciato il programma.
#[derive(Debug, PartialEq, Eq)]
pub struct Options {
    pub layout: LayoutFile,
    pub hide_gui: bool,
}

impl Options {
    /// La bandiera puo' stare prima o dopo il nome del file, con o senza trattini:
    /// e' l'unica opzione che esiste, non vale la pena imporre un ordine.
    pub fn from_args(args: impl Iterator<Item = String>) -> Self {
        let mut hide_gui = false;
        let mut positional = Vec::new();

        for arg in args {
            if arg.trim_start_matches('-') == HIDE_GUI_FLAG {
                hide_gui = true;
            } else {
                positional.push(arg);
            }
        }

        Options {
            layout: LayoutFile::from_args(positional.into_iter()),
            hide_gui,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Options {
        Options::from_args(args.iter().map(|arg| arg.to_string()))
    }

    #[test]
    fn without_the_flag_the_window_opens() {
        let options = parse(&["impianto.ron"]);

        assert!(!options.hide_gui);
        assert_eq!(options.layout.path, "impianto.ron");
    }

    #[test]
    fn the_flag_does_not_get_mistaken_for_a_file_name() {
        for args in [
            ["impianto.ron", "hide_gui"],
            ["hide_gui", "impianto.ron"],
            ["--hide_gui", "impianto.ron"],
        ] {
            let options = parse(&args);

            assert!(options.hide_gui, "{args:?}");
            assert_eq!(options.layout.path, "impianto.ron", "{args:?}");
        }
    }

    /// Senza layout non c'e' nessuna sorgente, quindi headless non succede
    /// niente: e' legale ma vale la pena accorgersene.
    #[test]
    fn the_flag_alone_leaves_the_default_file() {
        let options = parse(&["hide_gui"]);

        assert!(options.hide_gui);
        assert!(!options.layout.load_at_startup);
    }
}
