use bevy::prelude::*;

use crate::carrier::Heading;
use crate::layout::Placed;
use crate::piece::{self, Facing, PieceShapes, Tool};

/// Un tratto di guida: una linea sola, lunga quanto la cella. Non tocca il
/// movimento e non ha niente da comandare - serve a far vedere dove i carrier
/// possono andare, e basta.
///
/// Ce n'e' un tipo solo perche' due linee affiancate le compone l'utente
/// piazzandone una sopra e una sotto al flusso: cosi' un corridoio, un
/// incrocio o un innesto nascono dagli stessi pezzi, senza doverne inventare
/// uno per ogni caso.
#[derive(Component)]
pub struct Guide;

pub struct GuideVisualsPlugin;

impl Plugin for GuideVisualsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_guide_assets)
            // In fila: un tratto appena nato prende la figura piena e subito
            // dopo, nello stesso frame, la corregge se ha un deviatore accanto.
            .add_systems(
                Update,
                (attach_guide_visuals, open_the_lane_where_needed).chain(),
            );
    }
}

#[derive(Resource)]
pub struct GuideAssets {
    material: Handle<ColorMaterial>,
}

fn setup_guide_assets(mut commands: Commands, mut materials: ResMut<Assets<ColorMaterial>>) {
    commands.insert_resource(GuideAssets {
        // Un grigio spento, senza gradini di stato: la guida non ha stati, e
        // deve far vedere il tracciato senza rubare l'occhio agli oggetti, che
        // invece con il colore dicono qualcosa.
        material: materials.add(Color::srgb(0.42, 0.44, 0.50)),
    });
}

pub fn spawn_guide(commands: &mut Commands, position: Vec3) -> Entity {
    commands
        .spawn((Transform::from_translation(position), Guide))
        .id()
}

fn attach_guide_visuals(
    mut commands: Commands,
    shapes: Res<PieceShapes>,
    assets: Res<GuideAssets>,
    guides: Query<Entity, (With<Guide>, Without<Mesh2d>)>,
) {
    for entity in guides.iter() {
        let (shape, _) = piece::dressing(&shapes, Tool::Guide);

        commands
            .entity(entity)
            .insert((Mesh2d(shape), MeshMaterial2d(assets.material.clone())));
    }
}

/// Dove un deviatore fa uscire il carrier, il fianco della corsia deve aprirsi -
/// e il tratto di guida che passa di li' deve smettere di disegnarlo.
///
/// E' il guaio che si vedeva a schermo: il carrier attraversava una riga, perche'
/// la guida e' solo disegno e non ferma nessuno, ma quella riga diceva il
/// contrario di quello che l'impianto faceva.
///
/// Il bordo sta **fra due celle**, quindi lo apre sia un deviatore di qua sia uno
/// di la': un ATR nel ramo apre il fianco della corsia principale che gli sta
/// sopra, dove il tratto di guida ce l'ha messo l'utente. Per questo non si
/// cancella niente e non si vieta niente: la guida resta dov'e' e disegna un
/// bordo solo, che e' esattamente cio' che quel pezzo di corridoio e'.
/// Quali dei due bordi disegna il tratto di guida in `cell`, viste le celle
/// attorno: `[avanti, dietro]` rispetto al verso in cui e' girato.
pub fn edges_for(cell: IVec2, facing: Facing, around: &[(IVec2, Tool, Facing)]) -> [bool; 2] {
    let open = |toward: Heading| {
        let beyond = cell + toward.as_vec().as_ivec2();

        around
            .iter()
            .any(|(at, tool, facing)| match tool.opens_toward(*facing) {
                // Il deviatore e' in questa stessa cella e apre di qua...
                Some(opening) if *at == cell => opening == toward,
                // ...oppure e' dall'altra parte del bordo e apre verso di noi.
                Some(opening) if *at == beyond => opening == toward.opposite(),
                _ => false,
            })
    };

    [!open(facing.0), !open(facing.0.opposite())]
}

fn open_the_lane_where_needed(
    shapes: Res<PieceShapes>,
    pieces: Query<(&Placed, &Facing)>,
    // Basta che qualcosa si sia mosso, sia nato o sia stato girato: fuori da
    // quei momenti i bordi restano quelli, e ricalcolarli a ogni frame vorrebbe
    // dire riscrivere le stesse figure sessanta volte al secondo.
    touched: Query<(), Or<(Changed<Placed>, Changed<Facing>)>>,
    mut gone: RemovedComponents<Placed>,
    mut guides: Query<(&Placed, &Facing, &mut Mesh2d), With<Guide>>,
) {
    if touched.is_empty() && gone.read().next().is_none() {
        return;
    }

    let around: Vec<(IVec2, Tool, Facing)> = pieces
        .iter()
        .map(|(placed, facing)| (placed.cell, placed.tool, *facing))
        .collect();

    for (placed, facing, mut mesh) in guides.iter_mut() {
        let wanted = shapes.guide_corridor(edges_for(placed.cell, *facing, &around));

        // Si riscrive solo se cambia davvero: assegnare lo stesso handle
        // sveglierebbe il rendering per niente.
        if mesh.0 != wanted {
            mesh.0 = wanted;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOTH: [bool; 2] = [true, true];

    /// Il caso visto a schermo: un ATR nel ramo, e sopra di lui la corsia
    /// principale disegnata con i suoi tratti di guida. Il fianco fra i due si
    /// deve aprire, ma il tratto da correggere e' quello **della cella sopra**,
    /// non quello della cella dell'ATR: e' li' che l'utente l'ha messo. La
    /// parete esterna della corsia principale resta invece dov'e'.
    #[test]
    fn an_atr_below_opens_the_lane_drawn_above_it() {
        let lane = IVec2::ZERO;
        let along = Facing(Heading::Up);
        // Girato a destra, l'ATR riporta il carrier alla propria sinistra,
        // cioe' in su: verso la corsia.
        let atr = [(IVec2::new(0, -1), Tool::Atr, Facing(Heading::Right))];

        assert_eq!(
            edges_for(lane, along, &atr),
            [true, false],
            "cade il bordo verso l'ATR, resta quello esterno"
        );
        assert_eq!(
            edges_for(lane, along, &[]),
            BOTH,
            "senza deviatori attorno il corridoio e' chiuso da tutt'e due i lati"
        );
    }

    /// Vale anche per il deviatore che sta nella cella della guida: e' il caso
    /// del divert, che vive in mezzo alla corsia principale.
    #[test]
    fn a_divert_in_the_same_cell_opens_the_side_it_pushes_toward() {
        let cell = IVec2::ZERO;
        let along = Facing(Heading::Up);
        let divert = [(cell, Tool::Divert, Facing(Heading::Up))];

        assert_eq!(edges_for(cell, along, &divert), [false, true]);
    }

    /// Chi non devia nessuno non apre niente: un gate nella stessa cella o in
    /// quella accanto lascia il corridoio chiuso com'era.
    #[test]
    fn a_gate_does_not_open_anything() {
        let cell = IVec2::ZERO;
        let along = Facing(Heading::Up);
        let gates = [
            (cell, Tool::Gate, Facing(Heading::Up)),
            (IVec2::new(0, 1), Tool::Gate, Facing(Heading::Down)),
        ];

        assert_eq!(edges_for(cell, along, &gates), BOTH);
    }

    /// Un deviatore che apre dall'altra parte non riguarda questo tratto: il
    /// bordo che si apre e' uno solo, e sta dove passa il carrier.
    #[test]
    fn a_divert_pushing_elsewhere_leaves_this_corridor_alone() {
        let cell = IVec2::ZERO;
        let along = Facing(Heading::Up);
        // Sotto, ma spinge verso il basso: dal corridoio di sopra se ne va.
        let divert = [(IVec2::new(0, -1), Tool::Divert, Facing(Heading::Down))];

        assert_eq!(edges_for(cell, along, &divert), BOTH);
    }
}
