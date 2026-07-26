use std::f32::consts::{FRAC_PI_4, PI};

use bevy::asset::RenderAssetUsages;
use bevy::mesh::PrimitiveTopology;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::carrier::{CARRIER_RADIUS, Heading};
use crate::editor::Tool;
use crate::grid::GRID_STEP;

pub const PIECE_SIZE: f32 = 30.0;
/// Un paio di pixel piu' larga di un carrier: quando il carrier le si ferma
/// sopra ne resta scoperta una corona sottile, che dice che l'antenna c'e', di
/// che colore e', e la rende cliccabile.
pub const ANTENNA_RADIUS: f32 = CARRIER_RADIUS + 2.0;

/// Il gate non e' un quadrato come gli altri ma una sbarra su un lato della
/// cella. Cosi' il carrier che ferma si arresta quasi al centro della cella
/// invece che sul confine, e sotto di lui ci puo' stare un'antenna che lo legge.
pub const BAR_LENGTH: f32 = 48.0;
pub const BAR_THICKNESS: f32 = 8.0;
/// Quanto dista dal centro della cella: appoggiata al confine e tutta dentro la
/// propria cella, cosi' due gate accostati non si sovrappongono.
pub const BAR_OFFSET: f32 = (GRID_STEP - BAR_THICKNESS) / 2.0;

/// Anche l'antenna sta spostata verso il lato, e non a caso: e' esattamente
/// dove si ferma il carrier che la sbarra blocca. Il conto e' la faccia interna
/// della sbarra meno il raggio del carrier, cioe' il punto in cui il carrier si
/// arresta; cosi' se un giorno cambiano lo spessore della sbarra o la misura
/// del carrier, l'antenna resta sotto di lui senza doverla ritoccare.
pub const ANTENNA_OFFSET: f32 = BAR_OFFSET - BAR_THICKNESS / 2.0 - CARRIER_RADIUS;

/// Lunghezza complessiva della freccia, stelo compreso.
const ARROW_LENGTH: f32 = 20.0;
const ARROW_HEAD: f32 = 8.0;
const ARROW_HEAD_WIDTH: f32 = 11.0;
const ARROW_STEM_WIDTH: f32 = 4.0;
/// Raggio della freccia arcuata, quella dell'inversione.
const ARC_RADIUS: f32 = 8.0;
const ARC_SEGMENTS: usize = 12;
/// Sopra al quadrato che la contiene.
const ARROW_Z: f32 = 0.1;
const ARROW_COLOR: Color = Color::WHITE;
/// Il quadrato nero del despawn.
const STOP_SIZE: f32 = 14.0;
const STOP_COLOR: Color = Color::BLACK;

/// Che freccia ci va dentro. Non tutti gli oggetti ne hanno una: il gate blocca
/// e basta, in qualunque verso lo si giri.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Arrow {
    /// Nessuna: l'oggetto non ha un verso.
    None,
    /// Dritta: il carrier prosegue di li'.
    Straight,
    /// A quarantacinque gradi: e' la traiettoria del carrier deviato, cioe' la
    /// diagonale fra la sua marcia e lo spostamento di lato. Il deviatore la
    /// legge come tale, quindi la freccia dice il vero da qualunque parte
    /// arrivi il flusso.
    Deflected,
    /// Arcuata: il carrier torna indietro girando.
    Curved,
    /// Un quadrato nero: il carrier finisce li' e basta, da qualunque parte
    /// arrivi. Non e' un verso, quindi non e' una freccia.
    Stop,
}

/// Come e' girato l'oggetto, cioe' dove manda il carrier. Il tasto destro la fa
/// ruotare di un quarto di giro per volta.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Facing(pub Heading);

impl Default for Facing {
    fn default() -> Self {
        // Verso sinistra: e' il verso in cui i carrier viaggiano di partenza.
        Facing(Heading::Left)
    }
}

/// Mesh condivise da tutti gli oggetti: sono tutti quadrati uguali, cambia solo
/// il colore. La freccia dentro dice dove finisce il carrier.
#[derive(Resource)]
pub struct PieceShapes {
    square: Handle<Mesh>,
    circle: Handle<Mesh>,
    bar: Handle<Mesh>,
    straight_arrow: Handle<Mesh>,
    curved_arrow: Handle<Mesh>,
    stop: Handle<Mesh>,
    arrow_material: Handle<ColorMaterial>,
    stop_material: Handle<ColorMaterial>,
}

/// Mesh piatta da un elenco di triangoli. Le frecce non sono figure primitive:
/// vanno costruite a mano vertice per vertice.
fn triangles(points: Vec<[f32; 3]>) -> Mesh {
    let normals = vec![[0.0, 0.0, 1.0]; points.len()];
    let uvs = vec![[0.0, 0.0]; points.len()];

    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, points)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
}

/// Freccia con lo stelo, disegnata verso l'alto: due triangoli per il gambo e
/// uno per la punta.
fn straight_arrow() -> Mesh {
    let tip = ARROW_LENGTH / 2.0;
    let neck = tip - ARROW_HEAD;
    let tail = -ARROW_LENGTH / 2.0;
    let stem = ARROW_STEM_WIDTH / 2.0;
    let head = ARROW_HEAD_WIDTH / 2.0;

    triangles(vec![
        [-stem, tail, 0.0],
        [stem, tail, 0.0],
        [stem, neck, 0.0],
        [-stem, tail, 0.0],
        [stem, neck, 0.0],
        [-stem, neck, 0.0],
        [-head, neck, 0.0],
        [head, neck, 0.0],
        [0.0, tip, 0.0],
    ])
}

/// Freccia arcuata: mezzo giro attorno al centro del quadrato, con la punta in
/// fondo. Dice a colpo d'occhio che il carrier torna indietro girando.
fn curved_arrow() -> Mesh {
    let half = ARROW_STEM_WIDTH / 2.0;
    let mut points = Vec::new();

    // L'arco e' una striscia: due triangoli per ogni segmento.
    for segment in 0..ARC_SEGMENTS {
        let from = PI * segment as f32 / ARC_SEGMENTS as f32;
        let to = PI * (segment + 1) as f32 / ARC_SEGMENTS as f32;
        let (inner_from, outer_from) = (arc_point(from, -half), arc_point(from, half));
        let (inner_to, outer_to) = (arc_point(to, -half), arc_point(to, half));

        points.extend([inner_from, outer_from, outer_to]);
        points.extend([inner_from, outer_to, inner_to]);
    }

    // La punta chiude l'arco, tangente all'ultimo segmento.
    let end = arc_point(PI, 0.0);
    let width = ARROW_HEAD_WIDTH / 2.0;
    points.extend([
        [end[0] - width, end[1], 0.0],
        [end[0] + width, end[1], 0.0],
        [end[0], end[1] - ARROW_HEAD, 0.0],
    ]);

    triangles(points)
}

fn arc_point(angle: f32, offset: f32) -> [f32; 3] {
    let radius = ARC_RADIUS + offset;

    [radius * angle.cos(), radius * angle.sin(), 0.0]
}

pub struct PiecePlugin;

impl Plugin for PiecePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_piece_shapes)
            .add_systems(Update, orient_pieces);
    }
}

fn setup_piece_shapes(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    commands.insert_resource(PieceShapes {
        square: meshes.add(Rectangle::new(PIECE_SIZE, PIECE_SIZE)),
        circle: meshes
            .add(Mesh::from(Circle::new(ANTENNA_RADIUS)).translated_by(Vec3::Y * ANTENNA_OFFSET)),
        // La sbarra e' spostata nella mesh stessa e non nel Transform: la
        // posizione dell'oggetto resta il centro della cella, che e' quello che
        // sanno la griglia, il salvataggio e il trascinamento.
        bar: meshes.add(
            Mesh::from(Rectangle::new(BAR_LENGTH, BAR_THICKNESS))
                .translated_by(Vec3::Y * BAR_OFFSET),
        ),
        straight_arrow: meshes.add(straight_arrow()),
        curved_arrow: meshes.add(curved_arrow()),
        stop: meshes.add(Rectangle::new(STOP_SIZE, STOP_SIZE)),
        arrow_material: materials.add(ARROW_COLOR),
        stop_material: materials.add(STOP_COLOR),
    });
}

/// Mesh del quadrato, per l'anteprima dell'editor.
pub fn square(shapes: &PieceShapes) -> Handle<Mesh> {
    shapes.square.clone()
}

/// Mesh del cerchio dell'antenna, usata anche dall'anteprima dell'editor. Sta
/// qui perche' l'anteprima deve avere la stessa forma dell'oggetto che promette,
/// e averne una copia sola lo garantisce.
pub fn circle(shapes: &PieceShapes) -> Handle<Mesh> {
    shapes.circle.clone()
}

/// Mesh della sbarra del gate e dei sensori.
pub fn bar(shapes: &PieceShapes) -> Handle<Mesh> {
    shapes.bar.clone()
}

/// La figura che un oggetto occupa davvero dentro la sua cella. Serve a sapere
/// che cosa si sta puntando quando due oggetti condividono la cella: sulla
/// figura c'e' quello di linea, altrove l'antenna che gli sta sotto.
pub fn covers(tool: Tool, facing: Facing, centre: Vec2, point: Vec2) -> bool {
    let offset = point - centre;

    match tool {
        // La sbarra sta su un lato: il punto va riportato nel verso del gate
        // prima di misurarlo, altrimenti si misurerebbe sempre lo stesso lato.
        Tool::Gate | Tool::TubeSensor | Tool::CarrierSensor => {
            let local = facing.0.rotation().inverse() * offset.extend(0.0);

            local.x.abs() <= BAR_LENGTH / 2.0 && (local.y - BAR_OFFSET).abs() <= BAR_THICKNESS / 2.0
        }
        Tool::Antenna => {
            let local = facing.0.rotation().inverse() * offset.extend(0.0);

            local.truncate().distance(Vec2::Y * ANTENNA_OFFSET) <= ANTENNA_RADIUS
        }
        _ => offset.abs().cmple(Vec2::splat(PIECE_SIZE / 2.0)).all(),
    }
}

/// Da' corpo a un oggetto: il quadrato del suo colore, con dentro la freccia che
/// gli compete. Chi non ha un verso resta un quadrato pieno, e infatti il gate
/// blocca comunque lo si giri.
pub fn dress(
    commands: &mut Commands,
    entity: Entity,
    shapes: &PieceShapes,
    material: Handle<ColorMaterial>,
    arrow: Arrow,
) {
    dress_shape(
        commands,
        entity,
        shapes,
        shapes.square.clone(),
        material,
        arrow,
    );
}

/// Come `dress` ma con una figura scelta: la usa il gate, che e' una sbarra e
/// non un quadrato.
pub fn dress_shape(
    commands: &mut Commands,
    entity: Entity,
    shapes: &PieceShapes,
    shape: Handle<Mesh>,
    material: Handle<ColorMaterial>,
    arrow: Arrow,
) {
    commands
        .entity(entity)
        .insert((Mesh2d(shape), MeshMaterial2d(material)));

    let Some((mesh, material, tilt)) = (match arrow {
        Arrow::None => None,
        Arrow::Straight => Some((
            shapes.straight_arrow.clone(),
            shapes.arrow_material.clone(),
            0.0,
        )),
        Arrow::Deflected => Some((
            shapes.straight_arrow.clone(),
            shapes.arrow_material.clone(),
            FRAC_PI_4,
        )),
        Arrow::Curved => Some((
            shapes.curved_arrow.clone(),
            shapes.arrow_material.clone(),
            0.0,
        )),
        Arrow::Stop => Some((shapes.stop.clone(), shapes.stop_material.clone(), 0.0)),
    }) else {
        return;
    };

    commands.entity(entity).with_child((
        Mesh2d(mesh),
        MeshMaterial2d(material),
        Transform::from_xyz(0.0, 0.0, ARROW_Z).with_rotation(Quat::from_rotation_z(tilt)),
    ));
}

/// Gira l'oggetto secondo il suo orientamento. Ruotare il quadrato equivale a
/// ruotare la freccia, visto che il quadrato e' simmetrico: cosi' basta un
/// unico sistema e la freccia non ha bisogno di saperne niente.
fn orient_pieces(pieces: Query<(&Facing, &mut Transform), Changed<Facing>>) {
    for (facing, mut transform) in pieces {
        transform.rotation = facing.0.rotation();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// La sbarra sta su un lato della cella e lascia libero il centro: e' li'
    /// che va l'antenna, ed e' li' che si ferma il carrier.
    #[test]
    fn the_bar_leans_on_the_side_and_leaves_the_middle_free() {
        let centre = Vec2::ZERO;
        let facing = Facing(Heading::Up);

        assert!(
            covers(Tool::Gate, facing, centre, Vec2::new(0.0, BAR_OFFSET)),
            "sulla sbarra"
        );
        assert!(
            !covers(Tool::Gate, facing, centre, centre),
            "il centro della cella resta dell'antenna"
        );
        // Girato di un quarto, la sbarra passa sull'altro lato.
        assert!(covers(
            Tool::Gate,
            Facing(Heading::Left),
            centre,
            Vec2::new(-BAR_OFFSET, 0.0)
        ));
        assert!(!covers(
            Tool::Gate,
            Facing(Heading::Left),
            centre,
            Vec2::new(0.0, BAR_OFFSET)
        ));
    }

    /// Gli altri oggetti restano quadrati, e coprono l'antenna che gli sta
    /// sotto: e' il prezzo di averla grande quanto un carrier.
    #[test]
    fn the_other_pieces_are_still_squares() {
        let inside = Vec2::splat(PIECE_SIZE / 2.0 - 1.0);
        let outside = Vec2::splat(PIECE_SIZE / 2.0 + 1.0);

        assert!(covers(Tool::Atr, Facing::default(), Vec2::ZERO, inside));
        assert!(!covers(Tool::Atr, Facing::default(), Vec2::ZERO, outside));
    }

    /// L'antenna non e' al centro della cella ma spostata verso il lato, e si
    /// gira insieme all'oggetto: e' li' che si ferma il carrier da leggere.
    #[test]
    fn the_antenna_sits_where_the_stopped_carrier_stands() {
        let centre = Vec2::ZERO;
        let up = Facing(Heading::Up);

        assert!(covers(
            Tool::Antenna,
            up,
            centre,
            Vec2::new(0.0, ANTENNA_OFFSET)
        ));
        assert!(
            !covers(Tool::Antenna, up, centre, Vec2::new(0.0, -ANTENNA_OFFSET)),
            "dalla parte opposta non c'e'"
        );
        // Girata, si sposta con l'oggetto.
        assert!(covers(
            Tool::Antenna,
            Facing(Heading::Left),
            centre,
            Vec2::new(-ANTENNA_OFFSET, 0.0)
        ));
    }

    /// La freccia e' disegnata verso l'alto: la rotazione la porta nel verso
    /// dell'orientamento.
    #[test]
    fn the_rotation_points_the_arrow_where_the_carrier_goes() {
        for heading in [Heading::Left, Heading::Right, Heading::Up, Heading::Down] {
            let pointed = heading.rotation() * Vec3::Y;

            assert!(
                (pointed.truncate() - heading.as_vec()).length() < 0.001,
                "{heading:?} punta in {pointed:?}"
            );
        }
    }
}
