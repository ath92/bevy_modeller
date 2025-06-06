use crate::{
    overlay::{OverlayCamera, OVERLAY_LAYER},
    selection::{EntityDeselectedEvent, EntitySelectedEvent, Selected},
};
use bevy::{prelude::*, render::view::RenderLayers};

// Plugin for the translation system
pub struct TranslationPlugin;

impl Plugin for TranslationPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DragData>()
            .init_resource::<DragData>()
            .init_resource::<DragHandlesResource>()
            .add_observer(on_add_translatable);
    }
}

// Component to mark objects that can be translated
#[derive(Component)]
pub struct Translatable;

// Resource to track drag state
#[derive(Resource)]
pub enum DragData {
    Dragging {
        start_position: Vec3,
        active_axis: TranslationAxis,
    },
    Idle,
}

impl Default for DragData {
    fn default() -> Self {
        Self::Idle
    }
}

#[derive(Resource)]
pub struct DragHandlesResource {
    entity: Entity,
}

#[derive(Component)]
pub struct DragHandle(TranslationAxis);

impl Default for DragHandlesResource {
    fn default() -> Self {
        Self {
            entity: Entity::PLACEHOLDER,
        }
    }
}

// Enum to track which axis we're dragging along
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranslationAxis {
    X,
    Y,
    Z,
    // Free, // For free movement in the camera plane
}

fn on_add_translatable(trigger: Trigger<OnAdd, Translatable>, mut commands: Commands) {
    let target = trigger.target();

    info!("added translatable");

    let mut select_observer = Observer::new(on_select_translatable);
    let mut deselect_observer = Observer::new(on_deselect_translatable);

    select_observer.watch_entity(target);
    deselect_observer.watch_entity(target);

    commands.spawn(select_observer);
    commands.spawn(deselect_observer);
}

pub fn on_select_translatable(
    trigger: Trigger<EntitySelectedEvent>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>, // Resource to store mesh data
    mut materials: ResMut<Assets<StandardMaterial>>, // Resource to store material data)
    mut drag_handles_resource: ResMut<DragHandlesResource>,
) {
    let target = trigger.target();

    info!("selected something translatable");

    // Create a parent entity to hold our drag handles
    let handle_entity = commands
        .spawn((Transform::default(), Visibility::default()))
        .id();

    // Attach the parent to the target
    commands.entity(target).add_child(handle_entity);

    // Spawn X axis handle
    commands
        .spawn((
            Transform::from_xyz(1.5, 0.0, 0.0),
            Mesh3d(meshes.add(Sphere {
                radius: 0.1,
                ..default()
            })),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgb(0.9, 0.2, 0.2), // Red for X axis
                ..default()
            })),
            ChildOf(handle_entity),
            DragHandle(TranslationAxis::X),
            RenderLayers::layer(OVERLAY_LAYER),
        ))
        .observe(on_drag_start_handle)
        .observe(on_drag_handle)
        .observe(on_drag_end_handle);

    // Spawn Y axis handle
    commands
        .spawn((
            Transform::from_xyz(0., 1.5, 0.0),
            Mesh3d(meshes.add(Sphere {
                radius: 0.1,
                ..default()
            })),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgb(0.2, 0.9, 0.2), // Green for Y axis
                ..default()
            })),
            ChildOf(handle_entity),
            DragHandle(TranslationAxis::Y),
            RenderLayers::layer(OVERLAY_LAYER),
        ))
        .observe(on_drag_start_handle)
        .observe(on_drag_handle)
        .observe(on_drag_end_handle);

    // Spawn Z axis handle
    commands
        .spawn((
            Transform::from_xyz(0., 0.0, 1.5),
            Mesh3d(meshes.add(Sphere {
                radius: 0.1,
                ..default()
            })),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgb(0.2, 0.2, 0.9), // Blue for Z axis
                ..default()
            })),
            ChildOf(handle_entity),
            DragHandle(TranslationAxis::Z),
            RenderLayers::layer(OVERLAY_LAYER),
        ))
        .observe(on_drag_start_handle)
        .observe(on_drag_handle)
        .observe(on_drag_end_handle);

    drag_handles_resource.entity = handle_entity;
}

fn on_deselect_translatable(
    trigger: Trigger<EntityDeselectedEvent>,
    handle: Res<DragHandlesResource>,
    mut commands: Commands,
) {
    let target = trigger.target();
    let handle_entity = handle.entity;

    info!("deselect translatable");
    info!("target: {:?}", target);
    info!("handle_entity: {:?}", handle_entity);

    // Properly despawn the handle entity
    commands.entity(handle_entity).despawn();
}

fn on_drag_start_handle(
    trigger: Trigger<Pointer<DragStart>>,
    drag_handles: Query<&DragHandle>,
    mut drag_data: ResMut<DragData>,
) {
    let Some(hit_position) = trigger.event().hit.position else {
        return;
    };

    let Ok(handle) = drag_handles.get(trigger.target()) else {
        return;
    };

    info!("dragstart");

    *drag_data = DragData::Dragging {
        start_position: hit_position,
        active_axis: handle.0,
    };
}

fn on_drag_handle(
    trigger: Trigger<Pointer<Drag>>,
    drag_data: ResMut<DragData>,
    mut selected_translatable: Query<(&mut Transform, &Translatable, &Selected)>,
    cameras: Query<(&Camera, &GlobalTransform, &OverlayCamera)>,
) {
    let (start_pos, active_axis) = match *drag_data {
        DragData::Dragging {
            start_position,
            active_axis,
        } => (start_position, active_axis),
        DragData::Idle => return,
    };

    let Ok((_, camera_transform, _)) = cameras.single() else {
        return;
    };

    let Ok((mut entity_transform, _, _)) = selected_translatable.single_mut() else {
        return;
    };

    let ndc_delta = trigger.event().delta;

    // Scale factor - adjust as needed for movement sensitivity
    let drag_sensitivity = 0.001;

    // Get camera basis vectors
    let right = camera_transform.right();
    let up = camera_transform.up();

    // Calculate distance from camera to object for scaling
    let camera_to_object = (start_pos - camera_transform.translation()).length();
    let movement_scale = camera_to_object * drag_sensitivity;
    info!("dragging");

    match active_axis {
        TranslationAxis::X => {
            // Project screen movement onto world X axis
            let x_proj_right = right.dot(Vec3::X);
            let x_proj_up = up.dot(Vec3::X);

            // Calculate movement along X axis
            let x_movement =
                (ndc_delta.x * x_proj_right + ndc_delta.y * x_proj_up) * movement_scale;
            entity_transform.translation += Vec3::X * x_movement;
        }
        TranslationAxis::Y => {
            // Project screen movement onto world Y axis
            let y_proj_right = right.dot(Vec3::Y);
            let y_proj_up = up.dot(Vec3::Y);

            // Calculate movement along Y axis
            let y_movement =
                (ndc_delta.x * y_proj_right + ndc_delta.y * y_proj_up) * movement_scale;
            entity_transform.translation -= Vec3::Y * y_movement;
        }
        TranslationAxis::Z => {
            // Project screen movement onto world Z axis
            let z_proj_right = right.dot(Vec3::Z);
            let z_proj_up = up.dot(Vec3::Z);

            // Calculate movement along Z axis
            let z_movement =
                (ndc_delta.x * z_proj_right + ndc_delta.y * z_proj_up) * movement_scale;
            entity_transform.translation -= Vec3::Z * z_movement;
        } // TranslationAxis::Free => {
          //     // Move in the camera plane (perpendicular to view direction)
          //     let world_delta =
          //         right * ndc_delta.x * movement_scale + up * ndc_delta.y * movement_scale;
          //     entity_transform.translation += world_delta;
          // }
    }
}

fn on_drag_end_handle(_: Trigger<Pointer<DragEnd>>, mut drag_data: ResMut<DragData>) {
    *drag_data = DragData::Idle;
}
