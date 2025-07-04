use bevy::prelude::*;
use crossbeam_queue::SegQueue;

use std::sync::LazyLock;
use wasm_bindgen::{prelude::wasm_bindgen, JsValue};

use crate::mode::{AppMode, AppModeState};
use crate::sdf_render::{SDFRenderEnabled, SDFRenderEntity};
use crate::selection::handle_selection;
use crate::translation::Translatable;

#[derive(Resource)]
pub struct EntityIndexCounter {
    pub counter: u32,
}

impl Default for EntityIndexCounter {
    fn default() -> Self {
        Self { counter: 0 }
    }
}

pub struct CommandBridgePlugin;

impl Plugin for CommandBridgePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EntityIndexCounter>()
            .add_systems(Update, (process_app_commands, monitor_mode_changes));
    }
}

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_name = dispatch_bevy_event)]
    fn dispatch_bevy_event_js(event_name: &str, detail: JsValue);
}

pub enum AppCommand {
    SpawnSphereCommand {
        position: Vec3,
        scale: f32,
        color: Color,
    },
    SetModeCommand {
        mode: String,
    },
    SetPostProcessEnabledCommand {
        enabled: bool,
    },
}

// Global thread-safe queue for JS commands
static APP_COMMAND_QUEUE: LazyLock<SegQueue<AppCommand>> = LazyLock::new(|| SegQueue::new());

// System to process sphere spawn commands from the queue
pub fn process_app_commands(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    _camera: Query<(&Camera, &GlobalTransform)>,
    mut mode_state: ResMut<AppModeState>,
    mut post_process_enabled: ResMut<SDFRenderEnabled>,
    mut entity_index_counter: ResMut<EntityIndexCounter>,
) {
    while let Some(cmd) = APP_COMMAND_QUEUE.pop() {
        match cmd {
            AppCommand::SpawnSphereCommand {
                position,
                color,
                scale,
            } => {
                let index = entity_index_counter.counter;
                entity_index_counter.counter += 1;
                commands
                    .spawn((
                        Translatable,
                        SDFRenderEntity {
                            index,
                            position,
                            scale,
                        },
                        Transform::from_translation(position),
                        Mesh3d(meshes.add(Sphere {
                            radius: scale,
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
            AppCommand::SetModeCommand { mode } => {
                match mode.as_str() {
                    "Translate" => mode_state.set_mode(AppMode::Translate),
                    "Brush" => mode_state.set_mode(AppMode::Brush),
                    _ => {
                        warn!("Unknown mode requested: {}", mode);
                    }
                }
                info!("Mode changed to: {:?}", mode_state.current_mode);
            }
            AppCommand::SetPostProcessEnabledCommand { enabled } => {
                post_process_enabled.enabled = enabled;
            }
        }
    }
}

#[wasm_bindgen]
pub fn spawn_sphere_at_origin() {
    APP_COMMAND_QUEUE.push(AppCommand::SpawnSphereCommand {
        position: Vec3::new(0., 0., 0.),
        color: Color::Srgba(Srgba::WHITE),
        scale: 1.,
    });
}

pub fn spawn_sphere_at_pos(pos: Vec3, scale: f32) {
    APP_COMMAND_QUEUE.push(AppCommand::SpawnSphereCommand {
        position: pos,
        color: Color::Srgba(Srgba::WHITE),
        scale,
    });
}

// System to monitor mode changes and dispatch JavaScript events
pub fn monitor_mode_changes(mode_state: Res<AppModeState>) {
    #[cfg(target_arch = "wasm32")]
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
    APP_COMMAND_QUEUE.push(AppCommand::SetModeCommand {
        mode: mode.to_string(),
    });
}

#[wasm_bindgen]
pub fn set_post_process_enabled(enabled: bool) {
    APP_COMMAND_QUEUE.push(AppCommand::SetPostProcessEnabledCommand { enabled });
}
