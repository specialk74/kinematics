use bevy::prelude::*;

use crate::divert::LANE_HEIGHT;

/// Passo della griglia. Coincide col dislivello fra le corsie, cosi' la corsia
/// deviata cade esattamente una cella piu' su di quella principale.
pub const GRID_STEP: f32 = LANE_HEIGHT;

/// Fondo esplicito: quello di default di Bevy e' `srgb_u8(43, 44, 47)`, troppo
/// vicino a qualsiasi grigio tenue perche' il reticolo si distingua.
pub const BACKGROUND_COLOR: Color = Color::srgb(0.07, 0.07, 0.09);
const GRID_COLOR: Color = Color::srgb(0.22, 0.22, 0.27);
const LINE_THICKNESS: f32 = 1.0;
/// Dietro a tutto il resto: carrier a z = 0, oggetti piazzati a z = 1.
const GRID_Z: f32 = -10.0;

pub struct GridPlugin;

impl Plugin for GridPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ClearColor(BACKGROUND_COLOR))
            .init_resource::<DrawnExtent>()
            .add_systems(Update, redraw_grid);
    }
}

/// Righe e colonne attualmente disegnate. Serve a ridisegnare solo quando la
/// vista scopre celle nuove, invece che a ogni frame.
#[derive(Resource, Default, PartialEq, Eq)]
struct DrawnExtent(Option<(IVec2, IVec2)>);

#[derive(Component)]
struct GridLine;

/// Cella che contiene il punto.
pub fn cell(position: Vec2) -> IVec2 {
    (position / GRID_STEP).round().as_ivec2()
}

/// Centro della cella: e' li' che vengono appoggiati gli oggetti. I centri
/// cadono sui multipli del passo, quindi la corsia y = 0 e' una riga di celle.
pub fn cell_center(cell: IVec2) -> Vec2 {
    cell.as_vec2() * GRID_STEP
}

/// Coordinata del bordo fra la cella `n - 1` e la cella `n`.
fn boundary(n: i32) -> f32 {
    n as f32 * GRID_STEP - GRID_STEP / 2.0
}

/// Estremi (inclusi) delle linee che cadono dentro l'intervallo.
fn boundaries_within(from: f32, to: f32) -> std::ops::RangeInclusive<i32> {
    let first = ((from + GRID_STEP / 2.0) / GRID_STEP).ceil() as i32;
    let last = ((to + GRID_STEP / 2.0) / GRID_STEP).floor() as i32;
    first..=last
}

/// Riquadro di mondo inquadrato dalla camera, in coordinate mondo.
fn visible_rect(window: &Window, camera: &Transform, scale: f32) -> Rect {
    let half = Vec2::new(window.width(), window.height()) / 2.0 * scale;
    let centre = camera.translation.truncate();

    Rect::from_corners(centre - half, centre + half)
}

/// Ridisegna la griglia in modo che copra sempre quello che si vede: a
/// schermo intero, dopo uno zoom o dopo uno spostamento della vista. Il reticolo
/// non ha quindi un bordo, se non quello della finestra.
fn redraw_grid(
    mut commands: Commands,
    windows: Query<&Window>,
    camera: Query<(&Transform, &Projection), With<Camera2d>>,
    lines: Query<Entity, With<GridLine>>,
    mut drawn: ResMut<DrawnExtent>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let Ok((transform, projection)) = camera.single() else {
        return;
    };
    let Projection::Orthographic(orthographic) = projection else {
        return;
    };

    let visible = visible_rect(window, transform, orthographic.scale);
    let columns = boundaries_within(visible.min.x, visible.max.x);
    let rows = boundaries_within(visible.min.y, visible.max.y);

    let extent = DrawnExtent(Some((
        IVec2::new(*columns.start(), *rows.start()),
        IVec2::new(*columns.end(), *rows.end()),
    )));
    if *drawn == extent {
        return;
    }
    *drawn = extent;

    for line in lines.iter() {
        commands.entity(line).despawn();
    }

    // Le linee arrivano fino ai bordi della griglia disegnata, non a quelli
    // della finestra: cosi' restano valide anche mentre la vista scivola, finche'
    // non entrano in gioco celle nuove.
    let (left, right) = (boundary(*columns.start()), boundary(*columns.end()));
    let (bottom, top) = (boundary(*rows.start()), boundary(*rows.end()));
    // Lo spessore e' in pixel di schermo: senza questa correzione, zoomando, le
    // linee diventerebbero bande larghe.
    let thickness = LINE_THICKNESS * orthographic.scale;

    for column in columns {
        commands.spawn((
            Sprite::from_color(GRID_COLOR, Vec2::new(thickness, top - bottom)),
            Transform::from_xyz(boundary(column), (bottom + top) / 2.0, GRID_Z),
            GridLine,
        ));
    }

    for row in rows {
        commands.spawn((
            Sprite::from_color(GRID_COLOR, Vec2::new(right - left, thickness)),
            Transform::from_xyz((left + right) / 2.0, boundary(row), GRID_Z),
            GridLine,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clicks_snap_to_the_nearest_cell_centre() {
        assert_eq!(cell_center(cell(Vec2::new(5.0, -3.0))), Vec2::ZERO);
        assert_eq!(
            cell_center(cell(Vec2::new(GRID_STEP - 4.0, GRID_STEP + 4.0))),
            Vec2::splat(GRID_STEP)
        );
    }

    /// Due click nella stessa cella devono dare lo stesso posto: e' quello che
    /// permette di riconoscere una cella gia' occupata.
    #[test]
    fn every_point_of_a_cell_maps_to_the_same_place() {
        let just_inside = GRID_STEP / 2.0 - 0.01;

        assert_eq!(cell(Vec2::new(just_inside, -just_inside)), IVec2::ZERO);
        assert_eq!(cell(Vec2::new(-just_inside, just_inside)), IVec2::ZERO);
        assert_eq!(cell(Vec2::new(GRID_STEP / 2.0 + 0.01, 0.0)), IVec2::X);
    }

    #[test]
    fn lines_sit_between_cell_centres() {
        assert_eq!(boundary(0), -GRID_STEP / 2.0);
        assert_eq!(boundary(1), GRID_STEP / 2.0);
    }
}
