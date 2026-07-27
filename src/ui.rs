//! I pezzi con cui si mette un comando a schermo: i colori che dicono se e'
//! premibile, le misure di un bottone e i posti in cui si appoggia.
//!
//! Stanno qui e non nell'editor perche' li usa chiunque abbia un comando da
//! mostrare (la pausa della simulazione, i tasti della registrazione, il
//! pannello dei nomi) e nessuno di quelli ha a che fare con il costruire
//! l'impianto. Tenerli in un posto solo e' anche cio' che fa sembrare i comandi
//! parte della stessa interfaccia invece di quattro barre che si somigliano.

use bevy::prelude::*;

/// Larghezza della barra degli strumenti. La sanno anche gli altri pannelli:
/// e' il bordo sinistro oltre il quale possono appoggiarsi.
pub const PALETTE_WIDTH: f32 = 120.0;

pub const BUTTON_IDLE: Color = Color::srgb(0.20, 0.20, 0.24);
/// Comando che in questo momento non risponde: si vede che c'e' e che non e'
/// disponibile, invece di far credere a un tasto che non fa niente.
pub const BUTTON_UNAVAILABLE: Color = Color::srgb(0.16, 0.16, 0.18);
/// Comando pronto a essere premuto. Il grigio non bastava a distinguerlo da uno
/// inerte, e sono proprio i comandi che cambiano disponibilita' a doversi
/// distinguere a colpo d'occhio.
pub const BUTTON_READY: Color = Color::srgb(0.16, 0.52, 0.24);

/// Posto in fila in alto a destra. Le posizioni stanno qui e non sparse nei
/// moduli: cosi' aggiungere un bottone non ne sovrappone un altro.
pub fn top_button(slot: u32) -> (Button, Node) {
    const WIDTH: f32 = 96.0;
    const GAP: f32 = 8.0;

    (
        Button,
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(12.0),
            right: Val::Px(12.0 + slot as f32 * (WIDTH + GAP)),
            width: Val::Px(WIDTH),
            height: Val::Px(36.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
    )
}

/// Un bottone che riempie la colonna in cui sta: quelli della barra e del
/// riquadro del file.
pub fn button_node() -> (Button, Node) {
    (
        Button,
        Node {
            width: Val::Percent(100.0),
            height: Val::Px(34.0),
            flex_shrink: 0.0,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
    )
}

pub fn button_label(text: &str) -> (Text, TextFont, TextColor) {
    (
        Text::new(text),
        TextFont {
            font_size: 14.0,
            ..default()
        },
        TextColor(Color::WHITE),
    )
}

/// Vero se il puntatore e' su un elemento dell'interfaccia. I bottoni
/// galleggiano sopra la scena (play/pausa e reset in alto a destra), quindi non
/// basta escludere la barra: se il mouse e' su un bottone, quel clic e' suo.
pub fn pointer_over_ui(ui_interactions: &Query<&Interaction>) -> bool {
    ui_interactions
        .iter()
        .any(|interaction| *interaction != Interaction::None)
}
