use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::mode::{AppMode, AppModeState};
use crate::sdf_compute::{evaluate_sdf_async, SdfEvaluationReceiver, SdfEvaluationSender};

pub struct BrushModePlugin;

impl Plugin for BrushModePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BrushModeObserverState>()
            .add_systems(Startup, setup_brush_mode_observers)
            .add_systems(Update, handle_mode_change_for_brush);
    }
}

// Resource to track brush mode observer entities
#[derive(Resource, Default)]
pub struct BrushModeObserverState {
    pub drag_observer: Option<Entity>,
}

// System to set up brush mode observers based on initial mode
fn setup_brush_mode_observers(
    mut commands: Commands,
    mode_state: Res<AppModeState>,
    mut observer_state: ResMut<BrushModeObserverState>,
) {
    if mode_state.is_mode(AppMode::Brush) {
        let observer = commands.spawn(Observer::new(drag_paint)).id();
        observer_state.drag_observer = Some(observer);
        info!("Spawned brush mode observers");
    } else {
        // Mode is not brush - despawn observers if active
        if let Some(observer_entity) = observer_state.drag_observer.take() {
            commands.entity(observer_entity).despawn();
            info!("Despawned brush mode observers");
        }
    }
}

// System to handle mode changes for brush mode
fn handle_mode_change_for_brush(
    commands: Commands,
    mode_state: Res<AppModeState>,
    observer_state: ResMut<BrushModeObserverState>,
) {
    if mode_state.is_changed() {
        setup_brush_mode_observers(commands, mode_state, observer_state);
    }
}

fn drag_paint(
    trigger: Trigger<Pointer<Drag>>,
    window_query: Query<&Window, With<PrimaryWindow>>,
    sdf_sender: Res<SdfEvaluationSender>,
    sdf_receiver: Res<SdfEvaluationReceiver>,
) {
    // do something on drag
    let viewport_position = trigger.pointer_location.position;

    if let Ok(window) = window_query.single() {
        let width = window.resolution.width();
        let height = window.resolution.height();

        let mut gpu_points: Vec<Vec2> = Vec::new();
        gpu_points.push(Vec2 {
            x: viewport_position.x / width,
            y: viewport_position.y / height,
        });
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
}
