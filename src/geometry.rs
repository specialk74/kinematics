use bevy::prelude::*;

/// Vero se un cerchio tocca un rettangolo, entrambi non ruotati. Si misura la
/// distanza dal punto del rettangolo piu' vicino al centro del cerchio.
/// La usano il gate per fermare i carrier e il despawn per distruggerli.
pub fn circle_touches_box(
    box_centre: Vec3,
    half_size: Vec2,
    circle_centre: Vec3,
    radius: f32,
) -> bool {
    let distance = (circle_centre.truncate() - box_centre.truncate()).abs() - half_size;

    distance.max(Vec2::ZERO).length() < radius
}

#[cfg(test)]
mod tests {
    use super::*;

    const HALF: Vec2 = Vec2::new(10.0, 20.0);

    #[test]
    fn a_circle_beside_the_box_does_not_touch_it() {
        assert!(!circle_touches_box(
            Vec3::ZERO,
            HALF,
            Vec3::new(40.0, 0.0, 0.0),
            15.0
        ));
    }

    #[test]
    fn a_circle_reaching_the_edge_touches_it() {
        assert!(circle_touches_box(
            Vec3::ZERO,
            HALF,
            Vec3::new(20.0, 0.0, 0.0),
            15.0
        ));
    }

    /// Il rettangolo non e' un quadrato: il contatto dipende dal lato da cui si
    /// arriva, ed e' il caso in cui una distanza fra centri sbaglierebbe.
    #[test]
    fn the_two_sides_are_measured_separately() {
        let above = Vec3::new(0.0, 30.0, 0.0);

        assert!(circle_touches_box(Vec3::ZERO, HALF, above, 15.0));
        assert!(!circle_touches_box(Vec3::ZERO, HALF, above, 5.0));
    }
}
