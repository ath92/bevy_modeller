use bevy::prelude::*;
use bevy::tasks::Task;
use bevy::window::PrimaryWindow;

use crate::command_bridge::spawn_sphere_at_pos;
use crate::mode::{AppMode, AppModeState};
use crate::overlay::OverlayCamera;
use crate::sdf_compute::{evaluate_sdf_async, SdfEvaluationSender};

pub struct BrushModePlugin;

#[derive(Resource)]
pub struct BrushTask {
    pub task: Option<Task<()>>,
}

impl Default for BrushTask {
    fn default() -> Self {
        Self { task: None }
    }
}

impl Plugin for BrushModePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BrushTask>()
            .add_systems(Update, handle_click_brush);
    }
}

// System to handle mode changes for brush mode
fn handle_click_brush(
    mode_state: Res<AppModeState>,
    window: Single<&Window, With<PrimaryWindow>>,
    buttons: Res<ButtonInput<MouseButton>>,
    sdf_sender: Res<SdfEvaluationSender>,
    camera_query: Query<(&Camera, &GlobalTransform, &OverlayCamera)>,
    mut brush_task: ResMut<BrushTask>,
) {
    if !mode_state.is_mode(AppMode::Brush) {
        return;
    }

    if let Some(task) = &brush_task.task {
        if !task.is_finished() {
            return;
        }
    }

    if buttons.pressed(MouseButton::Left) {
        info!("drag paint");
        let Some(viewport_position) = window.cursor_position() else {
            return;
        };
        let Ok((camera, camera_transform, _)) = camera_query.single() else {
            return;
        };

        let Ok(ray) = camera.viewport_to_world(camera_transform, viewport_position) else {
            return;
        };

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
        // Spawn the future and store the task
        let task = bevy::tasks::AsyncComputeTaskPool::get().spawn(async move {
            let Ok(results) = evaluate_sdf_async(gpu_points, &sender_clone).await else {
                return;
            };
            for (_, result) in results.iter().enumerate() {
                let new_sphere_radius = 0.1;
                let pos = ray.get_point(result.distance - new_sphere_radius);

                spawn_sphere_at_pos(pos, new_sphere_radius);
            }
        });

        brush_task.task = Some(task);
    }
}
