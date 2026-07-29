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

/// Vero se un cerchio tocca un segmento spesso: si misura la distanza fra il
/// centro del cerchio e il punto piu' vicino del segmento.
///
/// E' la figura dei muri delle guide, e non un rettangolo, perche' una guida non
/// e' detto che sia allineata agli assi: c'e' la linea dritta di adesso, ma
/// anche la diagonale e l'arco, che e' una spezzata di questi. Un rettangolo non
/// ruotato saprebbe fare solo il primo caso.
pub fn circle_touches_segment(
    from: Vec2,
    to: Vec2,
    half_thickness: f32,
    circle_centre: Vec3,
    radius: f32,
) -> bool {
    let centre = circle_centre.truncate();
    let along = to - from;
    let length_squared = along.length_squared();

    // Un segmento lungo zero e' un punto: il rapporto non esisterebbe e la
    // proiezione verrebbe NaN, che poi non sarebbe ne' dentro ne' fuori.
    let travelled = if length_squared == 0.0 {
        0.0
    } else {
        // Fuori dagli estremi il punto piu' vicino e' l'estremo stesso: il
        // segmento ha due capi, la retta no.
        ((centre - from).dot(along) / length_squared).clamp(0.0, 1.0)
    };

    (from + along * travelled).distance(centre) < radius + half_thickness
}

#[cfg(test)]
mod tests {
    use super::*;

    const HALF: Vec2 = Vec2::new(10.0, 20.0);
    /// Un segmento orizzontale lungo una cella, centrato nell'origine.
    const FROM: Vec2 = Vec2::new(-32.0, 0.0);
    const TO: Vec2 = Vec2::new(32.0, 0.0);
    /// Mezzo spessore di una linea di guida.
    const THIN: f32 = 2.0;

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

    /// Il caso per cui il segmento esiste: un carrier che corre lungo la linea
    /// non la tocca, uno che ci arriva addosso di traverso si'.
    #[test]
    fn a_circle_beside_the_segment_does_not_touch_it() {
        let alongside = Vec3::new(0.0, 20.0, 0.0);
        let against = Vec3::new(0.0, 10.0, 0.0);

        assert!(!circle_touches_segment(FROM, TO, THIN, alongside, 15.0));
        assert!(circle_touches_segment(FROM, TO, THIN, against, 15.0));
    }

    /// Oltre il capo del segmento non c'e' piu' niente da toccare: e' cio' che
    /// distingue un segmento da una retta, ed e' il buco da cui passa la manovra
    /// di un deviatore.
    #[test]
    fn past_the_end_the_segment_stops() {
        let beyond = Vec3::new(TO.x + 20.0, 0.0, 0.0);
        let at_the_end = Vec3::new(TO.x + 10.0, 0.0, 0.0);

        assert!(!circle_touches_segment(FROM, TO, THIN, beyond, 15.0));
        assert!(circle_touches_segment(FROM, TO, THIN, at_the_end, 15.0));
    }

    /// Storto o dritto non cambia niente, e questo e' il motivo per cui i muri
    /// delle guide sono segmenti: una diagonale, o il tratto di un arco, si
    /// misura con lo stesso conto.
    #[test]
    fn a_slanted_segment_is_measured_the_same_way() {
        let diagonal = (Vec2::new(-32.0, -32.0), Vec2::new(32.0, 32.0));
        // Sulla perpendicolare che passa per l'origine, a 12 px dalla linea:
        // dentro la portata di un carrier, fuori da quella di uno piu' piccolo.
        let across = Vec3::new(-8.49, 8.49, 0.0);

        assert!(circle_touches_segment(
            diagonal.0, diagonal.1, THIN, across, 15.0
        ));
        assert!(
            !circle_touches_segment(diagonal.0, diagonal.1, THIN, across, 5.0),
            "un carrier piu' piccolo ci passa accanto"
        );
    }

    /// Un segmento degenere non deve mandare in NaN il conto: puo' nascere da
    /// una spezzata con due punti coincidenti.
    #[test]
    fn a_segment_of_no_length_is_just_a_point() {
        let point = Vec2::ZERO;

        assert!(circle_touches_segment(
            point,
            point,
            THIN,
            Vec3::new(10.0, 0.0, 0.0),
            15.0
        ));
        assert!(!circle_touches_segment(
            point,
            point,
            THIN,
            Vec3::new(20.0, 0.0, 0.0),
            15.0
        ));
    }
}
