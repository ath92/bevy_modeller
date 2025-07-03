use crate::{
    overlay::{OverlayCamera, OVERLAY_LAYER},
    selection::{EntityDeselectedEvent, EntitySelectedEvent, Selected},
    AppMode, AppModeState,
};
use bevy::{prelude::*, render::view::RenderLayers};
use bevy_panorbit_camera::PanOrbitCamera;

// Plugin for the translation system
pub struct TranslationPlugin;

impl Plugin for TranslationPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DragData>()
            .init_resource::<DragData>()
            .init_resource::<DragHandlesResource>()
            .add_systems(Update, on_change_app_mode)
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
        entity_start_position: Vec3,
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
}

fn on_add_translatable(trigger: Trigger<OnAdd, Translatable>, mut commands: Commands) {
    let target = trigger.target();

    let mut select_observer = Observer::new(on_select_translatable);
    let mut deselect_observer = Observer::new(on_deselect_translatable);

    select_observer.watch_entity(target);
    deselect_observer.watch_entity(target);

    commands.spawn(select_observer);
    commands.spawn(deselect_observer);
}

const HANDLE_DIST: f32 = 1.5;

pub fn on_change_app_mode(
    app_mode: Res<AppModeState>,
    drag_handles_resource: ResMut<DragHandlesResource>,
    mut commands: Commands,
) {
    if app_mode.is_mode(AppMode::Translate) || !app_mode.is_changed() {
        return;
    }
    let handle_entity = drag_handles_resource.entity;

    info!("deselect translatable");
    info!("handle_entity: {:?}", handle_entity);

    // Properly despawn the handle entity
    commands.entity(handle_entity).despawn();
}

pub fn on_select_translatable(
    trigger: Trigger<EntitySelectedEvent>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>, // Resource to store mesh data
    mut materials: ResMut<Assets<StandardMaterial>>, // Resource to store material data)
    mut drag_handles_resource: ResMut<DragHandlesResource>,
    app_mode: Res<AppModeState>,
) {
    if !app_mode.is_mode(AppMode::Translate) {
        return;
    }
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
            Transform::from_xyz(HANDLE_DIST, 0.0, 0.0),
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
            Transform::from_xyz(0., HANDLE_DIST, 0.0),
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
            Transform::from_xyz(0., 0.0, HANDLE_DIST),
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
    mut pan_orbit_query: Query<&mut PanOrbitCamera>,
    transform_query: Query<(&Transform, &Selected)>,
) {
    let Some(hit_position) = trigger.event().hit.position else {
        return;
    };

    let Ok(handle) = drag_handles.get(trigger.target()) else {
        return;
    };

    if let Ok(mut pan_orbit) = pan_orbit_query.single_mut() {
        pan_orbit.enabled = false;
    };

    info!("dragstart");

    let Ok((entity_start_transform, _)) = transform_query.single() else {
        return;
    };

    let active_axis = handle.0;

    *drag_data = DragData::Dragging {
        start_position: hit_position,
        active_axis,
        entity_start_position: entity_start_transform.translation,
    };
}

fn on_drag_handle(
    trigger: Trigger<Pointer<Drag>>,
    drag_data: ResMut<DragData>,
    mut selected_translatable: Query<(&mut Transform, &Translatable, &Selected)>,
    cameras: Query<(&Camera, &GlobalTransform, &OverlayCamera)>,
) {
    let (start_pos, entity_start_position, active_axis) = match *drag_data {
        DragData::Dragging {
            start_position,
            entity_start_position,
            active_axis,
        } => (start_position, entity_start_position, active_axis),
        DragData::Idle => return,
    };

    let Ok((camera, camera_transform, _)) = cameras.single() else {
        return;
    };

    let Ok((mut entity_transform, _, _)) = selected_translatable.single_mut() else {
        return;
    };

    info!("dragging");

    match active_axis {
        TranslationAxis::X => {
            let Ok(ray) = camera
                .viewport_to_world(camera_transform, trigger.event().pointer_location.position)
            else {
                return;
            };
            let diff = start_pos.y - ray.origin.y;
            let t = diff / ray.direction.y;
            if t < 0. {
                return;
            }
            let intersection = ray.get_point(t);

            let x_movement = (intersection - start_pos).dot(Vec3::X);

            entity_transform.translation = entity_start_position + Vec3::X * x_movement;
        }
        TranslationAxis::Y => {
            let Ok(ray) = camera
                .viewport_to_world(camera_transform, trigger.event().pointer_location.position)
            else {
                return;
            };

            let Some(t) = ray.intersect_plane(
                start_pos,
                InfinitePlane3d::new((ray.origin - start_pos).with_y(0.)),
            ) else {
                return;
            };

            let intersection = ray.get_point(t);

            let y_movement = (intersection - start_pos).dot(Vec3::Y);

            entity_transform.translation = entity_start_position + Vec3::Y * y_movement;
        }
        TranslationAxis::Z => {
            let Ok(ray) = camera
                .viewport_to_world(camera_transform, trigger.event().pointer_location.position)
            else {
                return;
            };
            let diff = start_pos.y - ray.origin.y;
            let t = diff / ray.direction.y;
            if t < 0. {
                return;
            }
            let intersection = ray.get_point(t);

            let z_movement = (intersection - start_pos).dot(Vec3::Z);

            entity_transform.translation = entity_start_position + Vec3::Z * z_movement;
        }
    }
}

fn on_drag_end_handle(
    _: Trigger<Pointer<DragEnd>>,
    mut drag_data: ResMut<DragData>,
    mut pan_orbit_query: Query<&mut PanOrbitCamera>,
) {
    *drag_data = DragData::Idle;

    if let Ok(mut pan_orbit) = pan_orbit_query.single_mut() {
        pan_orbit.enabled = true;
    };
}
