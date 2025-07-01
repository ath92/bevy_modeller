use bevy::prelude::*;
use crossbeam_queue::SegQueue;
use std::sync::LazyLock;
use wasm_bindgen::{prelude::wasm_bindgen, JsValue};

use crate::mode::{AppMode, AppModeState};
use crate::post_process::PostProcessEntity;
use crate::sdf_compute::{evaluate_sdf_async, SdfEvaluationReceiver, SdfEvaluationSender};
use crate::translation::Translatable;

pub struct JSBridgePlugin;

impl Plugin for JSBridgePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (process_js_commands, monitor_mode_changes));
    }
}

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_name = dispatch_bevy_event)]
    fn dispatch_bevy_event_js(event_name: &str, detail: JsValue);
}

pub enum JSCommand {
    SpawnSphereCommand { position: Vec3, color: Color },
    EvaluateSdfCommand { points: Vec<Vec3> },
    SetModeCommand { mode: String },
}

// Global thread-safe queue for JS commands
static SPAWN_QUEUE: LazyLock<SegQueue<JSCommand>> = LazyLock::new(|| SegQueue::new());

// System to process sphere spawn commands from the queue
pub fn process_js_commands(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    _camera: Query<(&Camera, &GlobalTransform)>,
    sdf_sender: Res<SdfEvaluationSender>,
    sdf_receiver: Res<SdfEvaluationReceiver>,
    mut mode_state: ResMut<AppModeState>,
) {
    while let Some(cmd) = SPAWN_QUEUE.pop() {
        match cmd {
            JSCommand::SpawnSphereCommand { position, color } => {
                commands.spawn((
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
                ));
            }
            JSCommand::EvaluateSdfCommand { points } => {
                let mut gpu_points: Vec<Vec2> = Vec::new();
                for p in points {
                    gpu_points.push(Vec2 { x: p.x, y: p.y });
                }
                info!("Starting SDF evaluation for {} points", gpu_points.len());
                let future = evaluate_sdf_async(gpu_points, &sdf_sender, &sdf_receiver);
                // The future will complete asynchronously and results will be processed
                // by the process_sdf_responses system
                // Spawn the future and handle results when ready
                bevy::tasks::AsyncComputeTaskPool::get()
                    .spawn(async move {
                        let results = future.await;
                        info!("SDF Evaluation Results:");
                        for (i, result) in results.iter().enumerate() {
                            info!("  Point {}: distance = {:.3}", i, result.distance);
                        }
                    })
                    .detach();
            }
            JSCommand::SetModeCommand { mode } => {
                match mode.as_str() {
                    "Translate" => mode_state.set_mode(AppMode::Translate),
                    "Brush" => mode_state.set_mode(AppMode::Brush),
                    _ => {
                        warn!("Unknown mode requested: {}", mode);
                    }
                }
                info!("Mode changed to: {:?}", mode_state.current_mode);
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

#[wasm_bindgen]
pub fn test_sdf_evaluation() {
    // Test SDF evaluation at a few interesting points
    let test_points = vec![
        Vec3::new(0.5, 0.5, 0.0),  // Center (should be inside red sphere)
        Vec3::new(1.0, 0.0, 0.0),  // Edge of red sphere
        Vec3::new(2.0, 0.0, 0.0),  // Center of blue sphere
        Vec3::new(-2.0, 0.0, 0.0), // Center of green sphere
        Vec3::new(0.0, 2.0, 0.0),  // Above spheres
        Vec3::new(5.0, 0.0, 0.0),  // Far away
    ];

    SPAWN_QUEUE.push(JSCommand::EvaluateSdfCommand {
        points: test_points,
    });
}

// System to monitor mode changes and dispatch JavaScript events
pub fn monitor_mode_changes(mode_state: Res<AppModeState>) {
    if mode_state.is_changed() {
        let mode_name = match mode_state.current_mode {
            AppMode::Translate => "Translate",
            AppMode::Brush => "Brush",
        };
        dispatch_bevy_event_js("modeChanged", JsValue::from_str(mode_name));
    }
}

#[wasm_bindgen]
pub fn set_mode(mode: &str) {
    SPAWN_QUEUE.push(JSCommand::SetModeCommand {
        mode: mode.to_string(),
    });
}
