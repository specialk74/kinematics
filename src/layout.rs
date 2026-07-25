use std::error::Error;
use std::fs;
use std::path::Path;

use bevy::prelude::*;
use ron::ser::PrettyConfig;
use serde::{Deserialize, Serialize};

use crate::divert::DivertKind;
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
    /// Un file indicato esplicitamente viene caricato all'avvio e diventa anche
    /// il bersaglio dei due bottoni: sceglierlo equivale a dire su cosa si lavora.
    pub fn new(path: Option<String>) -> Self {
        match path {
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

/// Oggetto appoggiato sulla griglia. Tiene la cella e lo strumento che l'ha
/// creato: bastano a sapere cosa c'e' in una cella e a riscrivere il file.
#[derive(Component)]
pub struct Placed {
    pub tool: Tool,
    pub cell: IVec2,
}

/// Costruisce la scena statica. Sta qui e non nell'editor perche' serve anche
/// senza interfaccia: e' il modo in cui un impianto salvato torna in memoria.
pub struct LayoutPlugin;

impl Plugin for LayoutPlugin {
    fn build(&self, app: &mut App) {
        // In PostStartup perche' fra sistemi dello stesso schedule l'ordine non
        // e' garantito, e la scena deve nascere dopo tutti i setup.
        app.add_systems(PostStartup, load_layout_at_startup);
    }
}

/// Unico punto in cui nasce un oggetto della scena: lo usano il clic
/// dell'editor, il bottone Carica e l'avvio da riga di comando.
pub fn place_in_cell(commands: &mut Commands, tool: Tool, cell: IVec2) {
    let position = crate::grid::cell_center(cell).extend(1.0);
    let object = match tool {
        Tool::CarrierSource => crate::source::spawn_source(commands, position),
        Tool::Gate => crate::gate::spawn_gate(commands, position),
        Tool::Divert => crate::divert::spawn_divert(commands, position, DivertKind::Divert),
        Tool::Atr => crate::divert::spawn_divert(commands, position, DivertKind::Atr),
    };

    commands.entity(object).insert(Placed { tool, cell });
}

pub fn spawn_layout(commands: &mut Commands, layout: &Layout) {
    for object in &layout.objects {
        place_in_cell(
            commands,
            object.tool,
            IVec2::new(object.cell.0, object.cell.1),
        );
    }

    info!("caricati {} oggetti", layout.objects.len());
}

/// Apre il layout passato sulla riga di comando. Un file che non si legge viene
/// segnalato e basta: si parte a scena vuota, cosi' si puo' comunque costruirlo
/// e salvarlo su quel nome.
fn load_layout_at_startup(mut commands: Commands, layout_file: Res<LayoutFile>) {
    info!("file di layout: {}", layout_file.path);

    if !layout_file.load_at_startup {
        return;
    }

    match load(&layout_file.path) {
        Ok(layout) => spawn_layout(&mut commands, &layout),
        Err(error) => error!("non riesco ad aprire {}: {error}", layout_file.path),
    }
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
    fn a_chosen_file_opens_by_itself() {
        let chosen = LayoutFile::new(Some("impianto.ron".to_string()));

        assert_eq!(chosen.path, "impianto.ron");
        assert!(chosen.load_at_startup);
    }

    #[test]
    fn the_label_shows_the_file_name_not_the_whole_path() {
        let nested = LayoutFile::new(Some("/tmp/impianti/linea2.ron".to_string()));

        assert_eq!(nested.display_name(), "linea2.ron");
        assert_eq!(LayoutFile::new(None).display_name(), DEFAULT_LAYOUT_PATH);
    }

    #[test]
    fn the_default_file_is_not_opened_on_its_own() {
        let fallback = LayoutFile::new(None);

        assert_eq!(fallback.path, DEFAULT_LAYOUT_PATH);
        assert!(
            !fallback.load_at_startup,
            "senza scelta esplicita non si carica niente all'avvio"
        );
    }
}
