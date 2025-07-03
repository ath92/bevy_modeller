use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::command_bridge::spawn_sphere_at_pos;
use crate::mode::{AppMode, AppModeState};
use crate::overlay::OverlayCamera;
use crate::sdf_compute::{evaluate_sdf_async, SdfEvaluationSender};

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
    } else {
        // Mode is not brush - despawn observers if active
        if let Some(observer_entity) = observer_state.drag_observer.take() {
            commands.entity(observer_entity).despawn();
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
    trigger: Trigger<Pointer<Click>>,
    window_query: Query<&Window, With<PrimaryWindow>>,
    sdf_sender: Res<SdfEvaluationSender>,
    camera_query: Query<(&Camera, &GlobalTransform, &OverlayCamera)>,
) {
    // do something on drag
    let viewport_position = trigger.pointer_location.position;

    let Ok((camera, camera_transform, _)) = camera_query.single() else {
        return;
    };

    let Ok(ray) = camera.viewport_to_world(camera_transform, viewport_position) else {
        return;
    };

    if let Ok(window) = window_query.single() {
        let width = window.resolution.width();
        let height = window.resolution.height();

        let mut gpu_points: Vec<Vec2> = Vec::new();
        gpu_points.push(Vec2 {
            x: viewport_position.x / width,
            y: viewport_position.y / height,
        });

        // Clone the sender to move into the async task
        let sender_clone = sdf_sender.clone();

        // Spawn the future and handle results when ready
        bevy::tasks::AsyncComputeTaskPool::get()
            .spawn(async move {
                let Ok(results) = evaluate_sdf_async(gpu_points, &sender_clone).await else {
                    return;
                };
                for (_, result) in results.iter().enumerate() {
                    let new_sphere_radius = 0.1;
                    let pos = ray.get_point(result.distance - new_sphere_radius);

                    spawn_sphere_at_pos(pos, new_sphere_radius);
                }
            })
            .detach();
    }
}
