use std::error::Error;
use std::fs;
use std::path::Path;

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use ron::ser::PrettyConfig;
use serde::{Deserialize, Serialize};

use crate::antenna::Antenna;
use crate::divert::{Divert, DivertKind};
use crate::editor::Tool;
use crate::gate::Gate;
use crate::name::{Identity, PieceId, PieceName};
use crate::piece::Facing;
use crate::reverser::Reverser;
use crate::sensor::Sensor;
use crate::source::CarrierSource;
use crate::turner::Turner;

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
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct LayoutObject {
    /// Chi e' questo oggetto per il programma. Assente nei file salvati prima
    /// degli id: quelli lo ricevono all'apertura.
    #[serde(default)]
    pub id: u32,
    pub tool: Tool,
    pub cell: (i32, i32),
    /// Dove manda il carrier. Assente nei file salvati prima che gli oggetti
    /// avessero un orientamento: quelli si riaprono con il verso di partenza.
    #[serde(default)]
    pub facing: Facing,
    /// Con che nome l'oggetto si presenta fuori di qui, mqtt compreso. Assente
    /// nei file salvati prima dei nomi: quelli lo ricevono all'apertura.
    #[serde(default)]
    pub name: String,
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

/// Gli oggetti che si accendono e si spengono. Raggrupparli evita di trascinare
/// cinque query separate in ogni sistema che li tocca.
#[derive(SystemParam)]
pub struct Switches<'w, 's> {
    sources: Query<'w, 's, &'static mut CarrierSource>,
    gates: Query<'w, 's, &'static mut Gate>,
    diverts: Query<'w, 's, &'static mut Divert>,
    turners: Query<'w, 's, &'static mut Turner>,
    reversers: Query<'w, 's, &'static mut Reverser>,
    antennas: Query<'w, 's, &'static mut Antenna>,
    sensors: Query<'w, 's, &'static mut Sensor>,
}

impl Switches<'_, '_> {
    /// Stato dell'oggetto, se e' di quelli che si accendono.
    pub fn get(&self, entity: Entity) -> Option<bool> {
        if let Ok(source) = self.sources.get(entity) {
            Some(source.active)
        } else if let Ok(gate) = self.gates.get(entity) {
            Some(gate.active)
        } else if let Ok(divert) = self.diverts.get(entity) {
            Some(divert.active)
        } else if let Ok(turner) = self.turners.get(entity) {
            Some(turner.active)
        } else if let Ok(reverser) = self.reversers.get(entity) {
            Some(reverser.active)
        } else if let Ok(antenna) = self.antennas.get(entity) {
            Some(antenna.active)
        } else {
            self.sensors.get(entity).ok().map(|s| s.active)
        }
    }

    pub fn set(&mut self, entity: Entity, active: bool) {
        if let Ok(mut source) = self.sources.get_mut(entity) {
            source.active = active;
        } else if let Ok(mut gate) = self.gates.get_mut(entity) {
            gate.active = active;
        } else if let Ok(mut divert) = self.diverts.get_mut(entity) {
            divert.active = active;
        } else if let Ok(mut turner) = self.turners.get_mut(entity) {
            turner.active = active;
        } else if let Ok(mut reverser) = self.reversers.get_mut(entity) {
            reverser.active = active;
        } else if let Ok(mut antenna) = self.antennas.get_mut(entity) {
            antenna.active = active;
        } else if let Ok(mut sensor) = self.sensors.get_mut(entity) {
            sensor.active = active;
        }
    }

    pub fn toggle(&mut self, entity: Entity) {
        if let Some(active) = self.get(entity) {
            self.set(entity, !active);
        }
    }
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
pub fn place_in_cell(
    commands: &mut Commands,
    tool: Tool,
    cell: IVec2,
    facing: Facing,
    who: Identity,
) {
    // La quota la decide il piano dell'oggetto: quelli di linea stanno sopra ai
    // carrier, l'antenna sotto.
    let position = crate::grid::cell_center(cell).extend(tool.layer().z());
    let object = match tool {
        Tool::CarrierSource => crate::source::spawn_source(commands, position),
        Tool::Gate => crate::gate::spawn_gate(commands, position),
        Tool::Divert => crate::divert::spawn_divert(commands, position, DivertKind::Divert),
        Tool::Atr => crate::divert::spawn_divert(commands, position, DivertKind::Atr),
        Tool::Despawner => crate::despawner::spawn_despawner(commands, position),
        Tool::Turner => crate::turner::spawn_turner(commands, position),
        Tool::Reverser => crate::reverser::spawn_reverser(commands, position),
        Tool::Antenna => crate::antenna::spawn_antenna(commands, position),
        Tool::TubeSensor => {
            crate::sensor::spawn_sensor(commands, position, crate::sensor::SensorKind::Tube)
        }
        Tool::CarrierSensor => {
            crate::sensor::spawn_sensor(commands, position, crate::sensor::SensorKind::Carrier)
        }
    };

    commands.entity(object).insert((
        Placed { tool, cell },
        facing,
        PieceId(who.id),
        PieceName(who.name),
    ));
}

/// Raccoglie in un `Layout` gli oggetti attualmente in scena. Lo usano il
/// bottone Salva e la registrazione, che porta con se' l'impianto.
pub fn collect(placed: &Query<(&Placed, &Facing, &PieceId, &PieceName)>) -> Layout {
    Layout {
        objects: placed
            .iter()
            .map(|(placed, facing, id, name)| LayoutObject {
                id: id.0,
                tool: placed.tool,
                cell: (placed.cell.x, placed.cell.y),
                facing: *facing,
                name: name.0.clone(),
            })
            .collect(),
    }
}

/// Chi e' ogni oggetto del layout, nell'ordine in cui stanno nel file. Chi non
/// ha id o nome li riceve qui: sono i file salvati prima che esistessero, e un
/// oggetto senza identita' non potrebbe ne' parlare su mqtt ne' comparire in una
/// registrazione. Quelli assegnati d'ufficio non si pestano con quelli che il
/// file porta gia'.
pub fn fill_identities(layout: &Layout) -> Vec<Identity> {
    let mut names: Vec<String> = layout
        .objects
        .iter()
        .map(|object| object.name.clone())
        .filter(|name| !name.is_empty())
        .collect();
    let mut ids: Vec<u32> = layout
        .objects
        .iter()
        .map(|object| object.id)
        .filter(|id| *id != 0)
        .collect();

    layout
        .objects
        .iter()
        .map(|object| {
            let name = if object.name.is_empty() {
                let fresh = crate::name::next_free(object.tool, &names);
                names.push(fresh.clone());
                fresh
            } else {
                object.name.clone()
            };

            // Lo zero non e' un id: e' il posto vuoto lasciato da un file
            // scritto prima che gli id esistessero.
            let id = if object.id == 0 {
                let fresh = crate::name::next_free_id(&ids);
                ids.push(fresh);
                fresh
            } else {
                object.id
            };

            Identity { id, name }
        })
        .collect()
}

pub fn spawn_layout(commands: &mut Commands, layout: &Layout) {
    for (object, who) in layout.objects.iter().zip(fill_identities(layout)) {
        place_in_cell(
            commands,
            object.tool,
            IVec2::new(object.cell.0, object.cell.1),
            object.facing,
            who,
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
                    id: 1,
                    name: "sorgente-1".to_string(),
                    tool: Tool::CarrierSource,
                    cell: (6, 0),
                    facing: Facing::default(),
                },
                LayoutObject {
                    id: 2,
                    name: "divert-1".to_string(),
                    tool: Tool::Divert,
                    cell: (2, 0),
                    facing: Facing(crate::carrier::Heading::Up),
                },
                LayoutObject {
                    id: 3,
                    name: "atr-1".to_string(),
                    tool: Tool::Atr,
                    cell: (-1, 1),
                    facing: Facing::default(),
                },
                LayoutObject {
                    id: 4,
                    name: "gate-1".to_string(),
                    tool: Tool::Gate,
                    cell: (-3, 0),
                    facing: Facing::default(),
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

    /// Il nome fa parte dell'oggetto salvato: senza, all'apertura si
    /// perderebbe proprio quello con cui l'impianto parla al mondo di fuori.
    #[test]
    fn the_names_travel_with_the_layout() {
        let saved = to_ron(&sample()).expect("serializzazione");

        assert!(saved.contains("gate-1"), "{saved}");
        assert_eq!(
            from_ron(&saved).expect("rilettura").objects[3].name,
            "gate-1"
        );
    }

    /// I file salvati prima dei nomi si aprono lo stesso: chi non ce l'ha lo
    /// riceve al caricamento, e nessuno resta senza.
    #[test]
    fn an_old_file_without_names_still_loads() {
        let old = "(objects: [(tool: Gate, cell: (1, 2)), (tool: Gate, cell: (3, 2))])";

        let layout = from_ron(old).expect("rilettura");
        assert!(layout.objects[0].name.is_empty());

        let given = fill_identities(&layout);
        let names: Vec<&str> = given.iter().map(|who| who.name.as_str()).collect();
        let ids: Vec<u32> = given.iter().map(|who| who.id).collect();

        assert_eq!(names, vec!["gate-1", "gate-2"], "battezzati all'apertura");
        assert_eq!(ids, vec![1, 2], "e numerati");
    }

    /// Un file a meta' strada - qualche nome scritto a mano, altri no - non deve
    /// generare doppioni: i nomi dati d'ufficio saltano quelli gia' usati.
    #[test]
    fn the_names_given_at_load_time_avoid_the_ones_already_there() {
        let mixed = "(objects: [\
            (tool: Gate, cell: (1, 2), name: \"gate-1\"), \
            (tool: Gate, cell: (3, 2)), \
            (tool: Gate, cell: (5, 2), name: \"gate-2\")])";

        let layout = from_ron(mixed).expect("rilettura");

        let names: Vec<String> = fill_identities(&layout)
            .into_iter()
            .map(|who| who.name)
            .collect();

        assert_eq!(names, vec!["gate-1", "gate-3", "gate-2"]);
    }

    /// I file salvati prima dell'orientamento devono continuare ad aprirsi.
    #[test]
    fn an_old_file_without_facing_still_loads() {
        let old = "(objects: [(tool: Gate, cell: (1, 2))])";

        let layout = from_ron(old).expect("rilettura");

        assert_eq!(layout.objects[0].facing, Facing::default());
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
