use bevy::{core_pipeline::prepass::DepthPrepass, prelude::*, window::WindowResolution};

use bevy_panorbit_camera::{PanOrbitCamera, PanOrbitCameraPlugin};
use crossbeam_queue::SegQueue;
use std::env;
use std::sync::LazyLock;
use std::time::Duration;
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

enum JSCommand {
    SpawnSphereCommand { position: Vec3, color: Color },
}

// Global thread-safe queue for JS commands
static SPAWN_QUEUE: LazyLock<SegQueue<JSCommand>> = LazyLock::new(|| SegQueue::new());

#[derive(Resource)]
struct AutoCloseTimer {
    timer: Timer,
    enabled: bool,
}

impl AutoCloseTimer {
    fn new() -> Self {
        let args: Vec<String> = env::args().collect();
        let auto_close = args.iter().any(|arg| arg == "--auto-close");

        Self {
            timer: Timer::new(Duration::from_secs(3), TimerMode::Once),
            enabled: auto_close,
        }
    }
}

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
        .add_systems(Update, (process_js_commands, auto_close_system))
        .insert_resource(DragData::default())
        .insert_resource(AutoCloseTimer::new())
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
        .observe(handle_selection)
        .observe(drag_paint);

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
        .observe(handle_selection)
        .observe(drag_paint);

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
        .observe(handle_selection)
        .observe(drag_paint);
}

// System to process sphere spawn commands from the queue
fn process_js_commands(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    camera: Query<(&Camera, &GlobalTransform)>,
) {
    while let Some(cmd) = SPAWN_QUEUE.pop() {
        match cmd {
            JSCommand::SpawnSphereCommand { position, color } => {
                commands
                    .spawn((
                        Translatable,
                        PostProcessEntity,
                        Transform::from_translation(position),
                        Mesh3d(meshes.add(Sphere {
                            radius: 1.0,
                            ..default()
                        })),
                        MeshMaterial3d(materials.add(StandardMaterial {
                            base_color: color,
                            ..default()
                        })),
                        GlobalTransform::default(),
                    ))
                    .observe(handle_selection);
            }
        }
    }
}

#[wasm_bindgen]
pub fn spawn_sphere_at_origin() {
    SPAWN_QUEUE.push(JSCommand::SpawnSphereCommand {
        position: Vec3::new(0., 0., 0.),
        color: Color::Srgba(Srgba::WHITE),
    });
}

fn auto_close_system(
    time: Res<Time>,
    mut timer: ResMut<AutoCloseTimer>,
    mut exit: EventWriter<AppExit>,
) {
    if timer.enabled {
        timer.timer.tick(time.delta());
        if timer.timer.finished() {
            info!("Auto-closing application after 15 seconds");
            exit.write(AppExit::Success);
        }
    }
}

fn drag_paint(trigger: Trigger<Pointer<Drag>>, camera: Query<(&Camera, &GlobalTransform)>) {
    // do something on drag
    let target = trigger.target();
    let viewport_position = trigger.pointer_location.position;
    let Ok((cam, camera_transform)) = camera.single() else {
        return;
    };
    let ray = cam.viewport_to_world(camera_transform, viewport_position);
}
