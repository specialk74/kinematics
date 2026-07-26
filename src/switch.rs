use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Lo stato di un oggetto, uguale per tutti: due interruttori distinti. Sta in
/// un componente a parte e non dentro a ciascun oggetto perche' e' la stessa
/// cosa per tutti, e perche' chi lo commuta - il mouse oggi, mqtt domani - non
/// deve sapere che oggetto sta commutando.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Switch {
    /// Fuori servizio: non fa la sua funzione e non manda niente. Per il
    /// programma che sta dall'altra parte e' come se l'oggetto non ci fosse.
    pub enabled: bool,
    /// Per gli attuatori e' il comando: fa o non fa la sua azione, quindi un
    /// gate attivo ferma e uno disattivo lascia passare.
    ///
    /// Per i sensori e' invece una forzatura: dichiarano presenza anche quando
    /// davanti non passa nessuno. Serve a chi collauda il programma di comando,
    /// che deve poter provare anche gli scenari che nella realta' non
    /// dovrebbero capitare.
    pub active: bool,
}

impl Default for Switch {
    fn default() -> Self {
        // Un oggetto appena piazzato e' in servizio e fa la sua funzione: e'
        // quello che ci si aspetta vedendolo comparire nella scena.
        Switch {
            enabled: true,
            active: true,
        }
    }
}

impl Switch {
    /// Lo stato di un oggetto appena piazzato: in servizio, e comandato solo se
    /// "attivo" vuol dire fare la propria azione. Un sensore invece nasce non
    /// forzato: nascere forzato vorrebbe dire raccontare una presenza inventata
    /// al programma sotto collaudo prima ancora di cominciare.
    pub fn fresh(forces_signal: bool) -> Self {
        Switch {
            enabled: true,
            active: !forces_signal,
        }
    }

    /// Vero se l'oggetto sta facendo la sua funzione: in servizio e comandato.
    /// E' la condizione che prima si scriveva `active`, ed e' quella che decide
    /// il comportamento di gate, deviatori, svolte, inversioni e sorgenti.
    pub fn working(self) -> bool {
        self.enabled && self.active
    }

    /// Vero se l'oggetto sta forzando il proprio segnale: e' in servizio e
    /// qualcuno gli ha detto di dichiarare presenza a prescindere.
    pub fn forcing(self) -> bool {
        self.enabled && self.active
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// I due interruttori sono indipendenti, ma il servizio comanda: un oggetto
    /// fuori servizio non fa niente, qualunque cosa dica l'altro.
    #[test]
    fn out_of_service_beats_everything_else() {
        let off = Switch {
            enabled: false,
            active: true,
        };

        assert!(!off.working());
        assert!(!off.forcing());
    }

    #[test]
    fn a_new_object_is_in_service_and_doing_its_job() {
        assert!(Switch::fresh(false).working());
    }

    /// Un sensore appena piazzato non forza niente: guarda e riferisce quello
    /// che passa davvero, ed e' il tester a farlo mentire quando gli serve.
    #[test]
    fn a_new_sensor_tells_the_truth() {
        let sensor = Switch::fresh(true);

        assert!(sensor.enabled);
        assert!(!sensor.forcing());
    }
}

/// I gradini di colore con cui un oggetto racconta il proprio stato. Sono
/// ricavati da un colore solo, quello che identifica il tipo di oggetto: cosi'
/// la scala e' la stessa per tutti e nessuno se la inventa per conto suo.
#[derive(Clone)]
pub struct Look {
    /// Fuori servizio.
    idle: Handle<ColorMaterial>,
    /// In servizio ma non comandato.
    dim: Handle<ColorMaterial>,
    /// In servizio e comandato.
    full: Handle<ColorMaterial>,
    /// ...e con un carrier fra le mani.
    busy: Handle<ColorMaterial>,
}

/// Grigio dei disabilitati: uguale per tutti, perche' fuori servizio vuol dire
/// la stessa cosa per tutti.
const IDLE_COLOR: Color = Color::srgb(0.3, 0.3, 0.3);

impl Look {
    pub fn new(materials: &mut Assets<ColorMaterial>, colour: Color) -> Self {
        Look {
            idle: materials.add(IDLE_COLOR),
            dim: materials.add(scaled(colour, 0.42)),
            full: materials.add(colour),
            busy: materials.add(lightened(colour, 0.55)),
        }
    }

    /// Il colore che compete a un oggetto in questo stato.
    pub fn material(&self, switch: Switch, engaged: bool) -> Handle<ColorMaterial> {
        match (switch.enabled, switch.active, engaged) {
            (false, _, _) => self.idle.clone(),
            (true, _, true) => self.busy.clone(),
            (true, true, false) => self.full.clone(),
            (true, false, false) => self.dim.clone(),
        }
    }
}

fn scaled(colour: Color, factor: f32) -> Color {
    let c = colour.to_srgba();

    Color::srgb(c.red * factor, c.green * factor, c.blue * factor)
}

fn lightened(colour: Color, amount: f32) -> Color {
    let c = colour.to_srgba();
    let towards_white = |channel: f32| channel + (1.0 - channel) * amount;

    Color::srgb(
        towards_white(c.red),
        towards_white(c.green),
        towards_white(c.blue),
    )
}
