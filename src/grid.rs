use bevy::prelude::*;

use crate::divert::LANE_HEIGHT;
use crate::{HEIGTH, WIDTH, WORK_AREA_LEFT};

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
            .add_systems(Startup, draw_grid);
    }
}

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

fn draw_grid(mut commands: Commands) {
    let right = WIDTH as f32 / 2.0;
    let top = HEIGTH as f32 / 2.0;
    let area_width = right - WORK_AREA_LEFT;
    let area_center_x = (WORK_AREA_LEFT + right) / 2.0;

    for n in boundaries_within(WORK_AREA_LEFT, right) {
        commands.spawn((
            Sprite::from_color(GRID_COLOR, Vec2::new(LINE_THICKNESS, HEIGTH as f32)),
            Transform::from_xyz(boundary(n), 0.0, GRID_Z),
        ));
    }

    for n in boundaries_within(-top, top) {
        commands.spawn((
            Sprite::from_color(GRID_COLOR, Vec2::new(area_width, LINE_THICKNESS)),
            Transform::from_xyz(area_center_x, boundary(n), GRID_Z),
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
