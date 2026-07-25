use std::error::Error;
use std::fs;
use std::path::Path;

use bevy::prelude::*;
use ron::ser::PrettyConfig;
use serde::{Deserialize, Serialize};

use crate::editor::Tool;

/// File usato quando non se ne passa uno sulla riga di comando. Il percorso e'
/// relativo alla cartella da cui si lancia il programma, non a dove sta l'eseguibile.
pub const DEFAULT_LAYOUT_PATH: &str = "layout.ron";

/// Il file su cui lavorano Salva e Carica per tutta la sessione.
#[derive(Resource, Debug, PartialEq, Eq)]
pub struct LayoutFile {
    pub path: String,
    /// Solo un percorso scelto esplicitamente viene caricato all'avvio: senza
    /// argomenti la scena parte vuota, come prima.
    pub load_at_startup: bool,
}

impl LayoutFile {
    /// Legge il primo argomento della riga di comando. Il file indicato viene
    /// caricato all'avvio e diventa anche il bersaglio dei due bottoni: passare
    /// un nome equivale a scegliere su cosa si sta lavorando.
    pub fn from_args(mut args: impl Iterator<Item = String>) -> Self {
        match args.next() {
            Some(path) => LayoutFile {
                path,
                load_at_startup: true,
            },
            None => LayoutFile {
                path: DEFAULT_LAYOUT_PATH.to_string(),
                load_at_startup: false,
            },
        }
    }

    /// Solo il nome del file, per l'etichetta nella barra: un percorso intero non
    /// ci starebbe. Quello completo finisce nel log all'avvio e a ogni salvataggio.
    pub fn display_name(&self) -> &str {
        Path::new(&self.path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(&self.path)
    }
}

/// Un oggetto statico: cosa e' e in che cella sta. Non salviamo i pixel ma gli
/// indici di cella, cosi' il file resta valido anche se cambia il passo della
/// griglia. Lo stato acceso/spento non fa parte del layout: il file descrive
/// l'impianto, non la sua configurazione del momento.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayoutObject {
    pub tool: Tool,
    pub cell: (i32, i32),
}

#[derive(Serialize, Deserialize, Debug, Default, PartialEq, Eq)]
pub struct Layout {
    pub objects: Vec<LayoutObject>,
}

pub fn to_ron(layout: &Layout) -> Result<String, ron::Error> {
    ron::ser::to_string_pretty(layout, PrettyConfig::default())
}

pub fn from_ron(text: &str) -> Result<Layout, ron::de::SpannedError> {
    ron::from_str(text)
}

pub fn save(layout: &Layout, path: &str) -> Result<(), Box<dyn Error>> {
    fs::write(path, to_ron(layout)?)?;
    Ok(())
}

pub fn load(path: &str) -> Result<Layout, Box<dyn Error>> {
    Ok(from_ron(&fs::read_to_string(path)?)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Layout {
        Layout {
            objects: vec![
                LayoutObject {
                    tool: Tool::CarrierSource,
                    cell: (6, 0),
                },
                LayoutObject {
                    tool: Tool::Divert,
                    cell: (2, 0),
                },
                LayoutObject {
                    tool: Tool::Atr,
                    cell: (-1, 1),
                },
                LayoutObject {
                    tool: Tool::Gate,
                    cell: (-3, 0),
                },
            ],
        }
    }

    #[test]
    fn a_layout_survives_the_round_trip() {
        let saved = to_ron(&sample()).expect("serializzazione");

        assert_eq!(from_ron(&saved).expect("rilettura"), sample());
    }

    /// Il file deve restare modificabile a mano: se il formato diventasse opaco
    /// perderebbe gran parte del suo senso.
    #[test]
    fn the_file_is_readable() {
        let saved = to_ron(&sample()).expect("serializzazione");

        assert!(saved.contains("CarrierSource"), "{saved}");
        assert!(saved.contains("(6, 0)"), "{saved}");
    }

    #[test]
    fn a_broken_file_is_reported_and_not_swallowed() {
        assert!(from_ron("questo non e' un layout").is_err());
    }

    #[test]
    fn the_command_line_chooses_the_file() {
        let chosen = LayoutFile::from_args(["impianto.ron".to_string()].into_iter());

        assert_eq!(chosen.path, "impianto.ron");
        assert!(chosen.load_at_startup, "il file passato si apre da solo");
    }

    #[test]
    fn the_label_shows_the_file_name_not_the_whole_path() {
        let nested = LayoutFile::from_args(["/tmp/impianti/linea2.ron".to_string()].into_iter());

        assert_eq!(nested.display_name(), "linea2.ron");
        assert_eq!(
            LayoutFile::from_args(std::iter::empty()).display_name(),
            DEFAULT_LAYOUT_PATH
        );
    }

    #[test]
    fn without_arguments_the_scene_starts_empty() {
        let fallback = LayoutFile::from_args(std::iter::empty());

        assert_eq!(fallback.path, DEFAULT_LAYOUT_PATH);
        assert!(
            !fallback.load_at_startup,
            "senza argomenti non si carica niente all'avvio"
        );
    }
}
