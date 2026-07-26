use bevy::ecs::component::Mutable;
use bevy::prelude::*;

use crate::antenna::Antenna;
use crate::carrier::{Carrier, Heading};
use crate::divert::Divert;
use crate::grid;
use crate::piece::Facing;
use crate::reverser::Reverser;
use crate::sensor::Sensor;
use crate::switch::Switch;
use crate::turner::Turner;

/// Un oggetto che si accorge dei carrier che gli passano per le mani. Per ora se
/// ne accorge e basta, e lo mostra accendendosi; quando ci sara' mqtt sara'
/// questo lo stato da mandare sul bus, ed e' il motivo per cui il conto si fa
/// qui nella simulazione e non nella parte grafica.
pub trait Engaged: Component<Mutability = Mutable> {
    fn engaged(&self) -> bool;
    fn set_engaged(&mut self, engaged: bool);
    /// Vero se quel carrier, in questo istante, lo riguarda. E' l'unica cosa
    /// che cambia da un oggetto all'altro: il sensore ha il suo fascio,
    /// l'antenna il suo cerchio, i deviatori la loro cella.
    /// L'interruttore arriva fin qui perche' per alcuni oggetti "sto agendo"
    /// dipende anche da come sono comandati: un divert non comandato lascia
    /// proseguire dritto, e non sta agendo su nessuno.
    fn reaches(
        &self,
        switch: Switch,
        at: Vec3,
        facing: Heading,
        carrier: &Carrier,
        carrier_at: Vec3,
    ) -> bool;

    /// Vero se per questo oggetto "attivo" vuol dire forzare il proprio
    /// segnale invece di comandare un'azione. E' il caso dei sensori e
    /// dell'antenna: un tester li accende a mano per far arrivare al programma
    /// di comando una presenza che nella realta' non c'e'.
    fn forced_by_switch(&self) -> bool {
        false
    }
}

/// Vero se il carrier e' nella cella dell'oggetto. La usano svolta e
/// inversione, la cui azione dura un frame solo - l'istante in cui cambiano la
/// marcia al carrier: accendersi solo li' sarebbe un lampo invisibile, quindi
/// per loro "ce l'ho fra le mani" resta la risposta buona.
///
/// I deviatori invece hanno una condizione vera e duratura - `catches` - e
/// usano quella: prima si accendevano anche per un carrier che attraversava la
/// loro cella senza che loro lo toccassero, e sembravano al lavoro mentre non
/// facevano niente.
pub fn in_the_same_cell(at: Vec3, carrier_at: Vec3) -> bool {
    grid::cell(at.truncate()) == grid::cell(carrier_at.truncate())
}

/// Un sistema solo per tutti gli oggetti che sanno rispondere a `reaches`.
pub fn mark_engaged<T: Engaged>(
    carriers: Query<(&Carrier, &Transform)>,
    objects: Query<(&mut T, &Switch, &Facing, &Transform), Without<Carrier>>,
) {
    for (mut object, switch, facing, at) in objects {
        let really = carriers.iter().any(|(carrier, carrier_at)| {
            object.reaches(
                *switch,
                at.translation,
                facing.0,
                carrier,
                carrier_at.translation,
            )
        });
        // Fuori servizio non si accorge di niente. In servizio si accorge di
        // quello che passa davvero, e i sensori anche di quello che un tester
        // gli fa dichiarare a mano.
        let engaged = switch.enabled && (really || (object.forced_by_switch() && switch.forcing()));

        // Si scrive solo quando cambia davvero: il colore si aggiorna guardando
        // `Changed<T>`, e riscriverlo ogni frame lo sveglierebbe per niente.
        if object.engaged() != engaged {
            object.set_engaged(engaged);
        }
    }
}

/// La parte che conta anche senza interfaccia: chi ha fra le mani chi.
pub struct EngagementPlugin;

impl Plugin for EngagementPlugin {
    fn build(&self, app: &mut App) {
        // Senza `run_if`: anche in pausa e durante una riproduzione un carrier
        // fermo davanti a un oggetto e' comunque davanti a quell'oggetto.
        app.add_systems(
            Update,
            (
                mark_engaged::<Sensor>,
                mark_engaged::<Antenna>,
                mark_engaged::<Divert>,
                mark_engaged::<Turner>,
                mark_engaged::<Reverser>,
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::carrier::{CarrierType, Motion};
    use crate::divert::{DivertKind, LANE_HEIGHT};
    use crate::grid::GRID_STEP;

    /// Fa girare il sistema vero: un divert e un carrier, e si guarda se il
    /// divert se n'e' accorto.
    fn engaged(active: bool, carrier_at: Vec3) -> bool {
        engaged_with(active, carrier_at, CarrierType::WithTube)
    }

    fn engaged_with(active: bool, carrier_at: Vec3, kind: CarrierType) -> bool {
        let mut app = App::new();
        app.add_plugins(EngagementPlugin);

        let divert = app
            .world_mut()
            .spawn((
                Transform::default(),
                Facing(Heading::Up),
                Switch {
                    enabled: active,
                    active: true,
                },
                Divert {
                    kind: DivertKind::Divert,
                    engaged: false,
                },
            ))
            .id();
        app.world_mut().spawn((
            Transform::from_translation(carrier_at),
            Carrier {
                kind,
                carrier_id: 1,
                sample_id: None,
                motion: Motion::Straight(Heading::Left),
            },
        ));

        app.update();

        app.world().get::<Divert>(divert).expect("divert").engaged
    }

    #[test]
    fn an_object_notices_the_carrier_it_is_moving() {
        assert!(engaged(true, Vec3::ZERO));
        assert!(
            !engaged(true, Vec3::new(GRID_STEP, 0.0, 0.0)),
            "quello nella cella accanto non lo riguarda"
        );
        // Un deviatore si accende solo mentre sta agganciando: un carrier che
        // gli attraversa la cella fuori dalla sua finestra di presa non lo
        // riguarda, e prima lo faceva sembrare al lavoro mentre non lo era.
        assert!(
            !engaged(true, Vec3::new(GRID_STEP / 2.0 - 1.0, 0.0, 0.0)),
            "di lato alla finestra di presa non lo tocca"
        );
        // E nemmeno un carrier che quel divert non smista.
        assert!(
            !engaged_with(true, Vec3::ZERO, CarrierType::Empty),
            "un vuoto passa dritto, quindi il divert non sta agendo su di lui"
        );
        // Ne' uno che sta gia' viaggiando nella corsia di arrivo: e' dentro il
        // corridoio della manovra, ma non c'e' piu' niente da spostare.
        assert!(
            !engaged(true, Vec3::new(0.0, LANE_HEIGHT, 0.0)),
            "chi e' gia' arrivato non lo riguarda piu'"
        );
    }

    /// Spento vuol dire fuori servizio, qui come sui sensori.
    #[test]
    fn a_switched_off_object_notices_nothing() {
        assert!(!engaged(false, Vec3::ZERO));
    }

    /// La cella e' quella della griglia, non un intorno del centro: due oggetti
    /// vicini non si contendono lo stesso carrier.
    #[test]
    fn the_cell_is_the_grid_cell() {
        let object = Vec3::ZERO;

        assert!(in_the_same_cell(object, Vec3::new(31.0, -31.0, 0.0)));
        assert!(!in_the_same_cell(object, Vec3::new(33.0, 0.0, 0.0)));
    }
}
