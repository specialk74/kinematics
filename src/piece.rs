use std::f32::consts::PI;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::PrimitiveTopology;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::carrier::Heading;

pub const PIECE_SIZE: f32 = 30.0;
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
    commands
        .entity(entity)
        .insert((Mesh2d(shapes.square.clone()), MeshMaterial2d(material)));

    let Some((mesh, material, tilt)) = (match arrow {
        Arrow::None => None,
        // La freccia punta sempre dove l'oggetto manda il carrier. Era disegnata
        // a 45 gradi per suggerire la traiettoria obliqua della deviazione, ma
        // quella diagonale dipende da come arriva il flusso — che l'oggetto non
        // sa — quindi meta' delle volte indicava la direzione sbagliata.
        Arrow::Straight => Some((
            shapes.straight_arrow.clone(),
            shapes.arrow_material.clone(),
            0.0,
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
