use bevy::{core_pipeline::prepass::DepthPrepass, prelude::*, window::WindowResolution};

use bevy_panorbit_camera::{PanOrbitCamera, PanOrbitCameraPlugin};
use iyes_perf_ui::{prelude::PerfUiDefaultEntries, PerfUiPlugin};
use rand::Rng;
use std::env;
use std::time::Duration;

mod brush_mode;
mod command_bridge;
mod mode;
mod overlay;
mod sdf_compute;
mod sdf_render;
mod selection;
mod translation;

use brush_mode::BrushModePlugin;
pub use command_bridge::spawn_sphere_at_origin;
use command_bridge::CommandBridgePlugin;
use mode::ModePlugin;
pub use mode::{switch_to_brush_mode, switch_to_translate_mode, AppMode, AppModeState};
use overlay::OverlayPlugin;
use sdf_compute::SdfComputePlugin;
use sdf_render::{SDFRenderEnabled, SDFRenderPlugin, SDFRenderSettings};
use selection::SelectionPlugin;
use translation::{DragData, TranslationPlugin};

use crate::command_bridge::spawn_sphere_at_pos;

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
            SDFRenderPlugin,
            PerfUiPlugin,
        ))
        .add_plugins(bevy::diagnostic::FrameTimeDiagnosticsPlugin::default())
        .add_plugins(bevy::diagnostic::EntityCountDiagnosticsPlugin)
        .add_plugins(bevy::diagnostic::SystemInformationDiagnosticsPlugin)
        .add_plugins(bevy::render::diagnostic::RenderDiagnosticsPlugin)
        .add_plugins(PanOrbitCameraPlugin)
        .add_plugins(MeshPickingPlugin)
        .add_plugins(ModePlugin)
        .add_plugins(SelectionPlugin)
        .add_plugins(OverlayPlugin)
        .add_plugins(TranslationPlugin)
        .add_plugins(SdfComputePlugin)
        .add_plugins(BrushModePlugin)
        .add_plugins(CommandBridgePlugin)
        .add_systems(Startup, setup_system)
        .add_systems(Update, (auto_close_system, toggle_sdf_render_system))
        .insert_resource(DragData::default())
        .insert_resource(AutoCloseTimer::new())
        .run();
}

// This system runs once at startup
fn setup_system(mut commands: Commands) {
    // Add a 3D camera positioned to view the sphere
    // Add a camera
    commands.spawn((
        Camera {
            order: 0,
            ..default()
        },
        SDFRenderSettings {
            near_plane: 0.1,
            far_plane: 10.,
            ..default()
        },
        DepthPrepass,
        Msaa::Off,
        PanOrbitCamera {
            button_orbit: MouseButton::Right,
            button_pan: MouseButton::Left,
            modifier_orbit: None,
            modifier_pan: Some(KeyCode::SuperLeft),
            ..default()
        },
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

    // let mut rng = rand::rng();
    // for i in 0..100 {
    //     info!("spanw {:?}", i);
    //     spawn_sphere_at_pos(
    //         Vec3::new(
    //             rng.random_range(-2.0..2.0),
    //             rng.random_range(-2.0..2.0),
    //             rng.random_range(-2.0..2.0),
    //         ),
    //         0.2,
    //     );
    // }
    spawn_sphere_at_pos(
        Vec3 {
            x: 0.,
            y: 0.,
            z: 0.,
        },
        1.,
    );

    commands.spawn(PerfUiDefaultEntries::default());
}

fn auto_close_system(
    time: Res<Time>,
    mut timer: ResMut<AutoCloseTimer>,
    mut exit: EventWriter<AppExit>,
) {
    if timer.enabled {
        timer.timer.tick(time.delta());
        if timer.timer.finished() {
            info!("Auto-closing application after some seconds");
            exit.write(AppExit::Success);
        }
    }
}

fn toggle_sdf_render_system(
    keyboard_input: Res<ButtonInput<KeyCode>>,
    mut sdf_render_enabled: ResMut<SDFRenderEnabled>,
) {
    if keyboard_input.just_pressed(KeyCode::KeyP) {
        sdf_render_enabled.enabled = !sdf_render_enabled.enabled;
        info!("Post-process toggled: {}", sdf_render_enabled.enabled);
    }
}
