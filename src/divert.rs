use bevy::prelude::*;

#[derive(PartialEq)]
pub enum DivertType {
    Std,
    Nsd,
}

#[derive(Component)]
pub struct Divert(pub DivertType);

pub struct DivertPlugin;

impl Plugin for DivertPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_divert);
    }
}

fn setup_divert(mut commands: Commands) {
    commands.spawn((
        Text2d::new("@"),
        TextFont {
            font_size: 12.0,
            font: default(),
            ..default()
        },
        TextColor(Color::WHITE),
        Transform::from_translation(Vec3::ZERO),
        Divert(DivertType::Nsd),
    ));
}
