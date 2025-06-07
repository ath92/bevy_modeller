use bevy::prelude::*;

mod overlay;
mod post_process;
mod selection;
mod translation;

use overlay::OverlayPlugin;
use post_process::PostProcessPlugin;
use selection::{handle_selection, Selected, SelectionPlugin};
use translation::{DragData, Translatable, TranslationPlugin};

use crate::post_process::PostProcessSettings;

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, PostProcessPlugin))
        .add_plugins(MeshPickingPlugin)
        .add_plugins(SelectionPlugin)
        .add_plugins(OverlayPlugin)
        .add_plugins(TranslationPlugin)
        .add_systems(Startup, setup_system)
        .add_systems(Update, highlight_selected_entities)
        .insert_resource(DragData::default())
        .run();
}

// This system runs once at startup
fn setup_system(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>, // Resource to store mesh data
    mut materials: ResMut<Assets<StandardMaterial>>, // Resource to store material data)
) {
    // Add a 3D camera positioned to view the sphere
    // Add a camera
    commands.spawn((
        Camera {
            order: 0,
            ..default()
        },
        PostProcessSettings {
            intensity: 0.02,
            ..default()
        },
        Camera3d { ..default() },
        Transform::from_xyz(0., 2.0, 5.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    commands.spawn((
        PointLight {
            shadows_enabled: true,
            intensity: 10_000_000.,
            range: 100.0,
            shadow_depth_bias: 0.2,
            ..default()
        },
        Transform::from_xyz(8.0, 16.0, 8.0),
    ));

    // Spawn a red sphere with Translatable component
    commands
        .spawn((
            Translatable,
            Transform::from_xyz(0.0, 0.0, 0.0),
            Mesh3d(meshes.add(Sphere::default())),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgb(0.9, 0.2, 0.2),
                ..default()
            })),
            GlobalTransform::default(),
        ))
        .observe(handle_selection);

    // Spawn a blue cube
    commands
        .spawn((
            Mesh3d(meshes.add(Cuboid::default())),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgb(0.2, 0.2, 0.9),
                ..default()
            })),
            Transform::from_xyz(2.0, 0.0, 0.0),
            GlobalTransform::default(),
            Translatable,
        ))
        .observe(handle_selection);

    // Spawn a green cylinder
    commands
        .spawn((
            Mesh3d(meshes.add(Cylinder::default())),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgb(0.2, 0.9, 0.2),
                ..default()
            })),
            Transform::from_xyz(-2.0, 0.0, 0.0),
            GlobalTransform::default(),
            Translatable,
        ))
        .observe(handle_selection);
}

// System to highlight selected entities with a visual indicator
fn highlight_selected_entities(
    selected_query: Query<(Entity, &GlobalTransform), With<Selected>>,
    mut gizmos: Gizmos,
) {
    for (_, transform) in selected_query.iter() {
        let position = transform.translation();

        // Draw a highlight box around the selected entity
        let size = Vec3::splat(1.2); // Slightly larger than the mesh

        // Draw box outline
        gizmos.cuboid(
            Transform::from_translation(position).with_scale(size),
            Color::srgb(0.9, 0.9, 0.1), // Yellow highlight
        );
    }
}
