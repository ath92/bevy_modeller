use bevy::{core_pipeline::prepass::DepthPrepass, prelude::*, window::WindowResolution};

use bevy_panorbit_camera::{PanOrbitCamera, PanOrbitCameraPlugin};
use crossbeam_queue::SegQueue;
use std::sync::LazyLock;
use wasm_bindgen::prelude::wasm_bindgen;

mod overlay;
mod post_process;
mod selection;
mod translation;

use overlay::OverlayPlugin;
use post_process::{PostProcessEntity, PostProcessPlugin};
use selection::{handle_selection, SelectionPlugin};
use translation::{DragData, Translatable, TranslationPlugin};

use crate::post_process::PostProcessSettings;

// Command for spawning spheres from JavaScript
#[derive(Debug, Clone)]
struct SpawnSphereCommand {
    position: Vec3,
    color: Color,
}

// Global thread-safe queue for sphere spawn commands
static SPAWN_QUEUE: LazyLock<SegQueue<SpawnSphereCommand>> = LazyLock::new(|| SegQueue::new());

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins.set(WindowPlugin {
                primary_window: Some(Window {
                    resolution: WindowResolution::new(1.0, 1.0).with_scale_factor_override(1.0),
                    fit_canvas_to_parent: true,
                    prevent_default_event_handling: false,
                    ..default()
                }),
                ..default()
            }),
            PostProcessPlugin,
        ))
        .add_plugins(PanOrbitCameraPlugin)
        .add_plugins(MeshPickingPlugin)
        .add_plugins(SelectionPlugin)
        .add_plugins(OverlayPlugin)
        .add_plugins(TranslationPlugin)
        .add_systems(Startup, setup_system)
        .add_systems(Update, process_spawn_commands)
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
            near_plane: 0.1,
            far_plane: 10.,
            ..default()
        },
        DepthPrepass,
        Msaa::Off,
        PanOrbitCamera::default(),
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
            PostProcessEntity,
            Transform::from_xyz(0.0, 0.0, 0.0),
            Mesh3d(meshes.add(Sphere {
                radius: 1.,
                ..default()
            })),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgb(0.9, 0.2, 0.2),
                ..default()
            })),
            GlobalTransform::default(),
        ))
        .observe(handle_selection);

    // Spawn a blue sphere
    commands
        .spawn((
            Mesh3d(meshes.add(Sphere {
                radius: 1.,
                ..default()
            })),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgb(0.2, 0.2, 0.9),
                ..default()
            })),
            Transform::from_xyz(2.0, 0.0, 0.0),
            GlobalTransform::default(),
            Translatable,
            PostProcessEntity,
        ))
        .observe(handle_selection);

    // Spawn a green sphere
    commands
        .spawn((
            Mesh3d(meshes.add(Sphere {
                radius: 1.,
                ..default()
            })),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgb(0.2, 0.9, 0.2),
                ..default()
            })),
            Transform::from_xyz(-2.0, 0.0, 0.0),
            GlobalTransform::default(),
            Translatable,
            PostProcessEntity,
        ))
        .observe(handle_selection);
}

// System to process sphere spawn commands from the queue
fn process_spawn_commands(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    while let Some(cmd) = SPAWN_QUEUE.pop() {
        commands
            .spawn((
                Translatable,
                PostProcessEntity,
                Transform::from_translation(cmd.position),
                Mesh3d(meshes.add(Sphere {
                    radius: 1.0,
                    ..default()
                })),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: cmd.color,
                    ..default()
                })),
                GlobalTransform::default(),
            ))
            .observe(handle_selection);
    }
}

#[wasm_bindgen]
pub fn spawn_sphere(x: f32, y: f32, z: f32, r: f32, g: f32, b: f32) -> String {
    let command = SpawnSphereCommand {
        position: Vec3::new(x, y, z),
        color: Color::srgb(r, g, b),
    };

    SPAWN_QUEUE.push(command);
    format!(
        "Queued sphere spawn at ({}, {}, {}) with color ({}, {}, {})",
        x, y, z, r, g, b
    )
}

#[wasm_bindgen]
pub fn spawn_sphere_at_origin() -> String {
    spawn_sphere(0.0, 0.0, 0.0, 0.5, 0.5, 0.9)
}
