use bevy::ecs::component::Mutable;
use bevy::prelude::*;

use crate::antenna::Antenna;
use crate::carrier::{Carrier, Heading};
use crate::divert::Divert;
use crate::grid;
use crate::piece::Facing;
use crate::reverser::Reverser;
use crate::sensor::Sensor;
use crate::turner::Turner;

/// Un oggetto che si accorge dei carrier che gli passano per le mani. Per ora se
/// ne accorge e basta, e lo mostra accendendosi; quando ci sara' mqtt sara'
/// questo lo stato da mandare sul bus, ed e' il motivo per cui il conto si fa
/// qui nella simulazione e non nella parte grafica.
pub trait Engaged: Component<Mutability = Mutable> {
    /// Un oggetto spento e' fuori servizio: non si accorge di niente.
    fn active(&self) -> bool;
    fn engaged(&self) -> bool;
    fn set_engaged(&mut self, engaged: bool);
    /// Vero se quel carrier, in questo istante, lo riguarda. E' l'unica cosa
    /// che cambia da un oggetto all'altro: il sensore ha il suo fascio,
    /// l'antenna il suo cerchio, i deviatori la loro cella.
    fn reaches(&self, at: Vec3, facing: Heading, carrier: &Carrier, carrier_at: Vec3) -> bool;
}

/// Vero se il carrier e' nella cella dell'oggetto. E' la risposta buona per
/// deviatori, svolte e inversioni: dice "ce l'ho fra le mani", che e' quanto
/// serve a farli accendere.
///
/// E' pero' una approssimazione, e va saputo: un carrier che attraversa la
/// cella di un divert spento, o che ci passa in un verso che quel divert non
/// tocca, accende comunque l'oggetto. La condizione vera - "sto agendo su di
/// lui" - e' diversa per ciascuno e andra' scritta quando serviranno i
/// messaggi veri, non il colore.
pub fn in_the_same_cell(at: Vec3, carrier_at: Vec3) -> bool {
    grid::cell(at.truncate()) == grid::cell(carrier_at.truncate())
}

/// Un sistema solo per tutti gli oggetti che sanno rispondere a `reaches`.
pub fn mark_engaged<T: Engaged>(
    carriers: Query<(&Carrier, &Transform)>,
    objects: Query<(&mut T, &Facing, &Transform), Without<Carrier>>,
) {
    for (mut object, facing, at) in objects {
        let engaged = object.active()
            && carriers.iter().any(|(carrier, carrier_at)| {
                object.reaches(at.translation, facing.0, carrier, carrier_at.translation)
            });

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
    use crate::divert::DivertKind;
    use crate::grid::GRID_STEP;

    /// Fa girare il sistema vero: un divert e un carrier, e si guarda se il
    /// divert se n'e' accorto.
    fn engaged(active: bool, carrier_at: Vec3) -> bool {
        let mut app = App::new();
        app.add_plugins(EngagementPlugin);

        let divert = app
            .world_mut()
            .spawn((
                Transform::default(),
                Facing(Heading::Up),
                Divert {
                    kind: DivertKind::Divert,
                    active,
                    engaged: false,
                },
            ))
            .id();
        app.world_mut().spawn((
            Transform::from_translation(carrier_at),
            Carrier {
                kind: CarrierType::Empty,
                carrier_id: 1,
                sample_id: None,
                motion: Motion::Straight(Heading::Left),
            },
        ));

        app.update();

        app.world().get::<Divert>(divert).expect("divert").engaged
    }

    #[test]
    fn an_object_notices_the_carrier_in_its_own_cell() {
        assert!(engaged(true, Vec3::ZERO));
        assert!(engaged(true, Vec3::new(GRID_STEP / 2.0 - 1.0, 0.0, 0.0)));
        assert!(
            !engaged(true, Vec3::new(GRID_STEP, 0.0, 0.0)),
            "quello nella cella accanto non lo riguarda"
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
