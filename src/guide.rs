use bevy::prelude::*;

use crate::piece::{self, PieceShapes, Tool};

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
            .add_systems(Update, attach_guide_visuals);
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
