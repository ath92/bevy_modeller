use bevy::{prelude::*, render::view::RenderLayers};

pub struct OverlayPlugin;

#[derive(Component)]
pub struct OverlayCamera;

impl Plugin for OverlayPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_system)
            .add_systems(Update, sync_handles_camera_to_main);
    }
}

pub const OVERLAY_LAYER: usize = 1;

fn setup_system(mut commands: Commands) {
    let camera_entity = commands
        .spawn((
            Camera {
                clear_color: ClearColorConfig::None,
                order: OVERLAY_LAYER as isize,
                ..default()
            },
            RenderLayers::layer(OVERLAY_LAYER),
            Camera3d { ..default() },
            OverlayCamera,
        ))
        .id();

    commands.spawn((
        PointLight { ..default() },
        ChildOf(camera_entity),
        RenderLayers::layer(OVERLAY_LAYER),
    ));
}

fn sync_handles_camera_to_main(
    // Query the main camera (assuming it doesn't have HandlesCamera component)
    main_camera_query: Query<
        (&GlobalTransform, &Projection),
        (With<Camera>, Without<OverlayCamera>),
    >,
    // Query the handles camera
    mut handles_camera_query: Query<(&mut Transform, &mut Projection), With<OverlayCamera>>,
) {
    if let Ok((main_gtransform, main_projection)) = main_camera_query.get_single() {
        if let Ok((mut handles_transform, mut handles_projection)) =
            handles_camera_query.get_single_mut()
        {
            // Get scale, rotation, and translation from the main camera's global transform
            let (s, r, t) = main_gtransform.to_scale_rotation_translation();

            // Apply to the handles camera's local transform
            // Handles camera should ideally be a child of the scene root or have its transform
            // set globally if it's not parented to the main camera directly.
            handles_transform.translation = t;
            handles_transform.rotation = r;
            // If the main camera has non-default scale, apply it too.
            // handles_transform.scale = s;

            *handles_projection = main_projection.clone();
        }
    }
}
