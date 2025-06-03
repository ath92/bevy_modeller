use bevy::prelude::*;

// Plugin for the selection system
pub struct SelectionPlugin;

impl Plugin for SelectionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SelectionState>()
            .add_event::<EntitySelectedEvent>()
            .add_event::<EntityDeselectedEvent>();
    }
}

// Component to mark the currently selected entity
#[derive(Component)]
pub struct Selected;

// Resource to track the currently selected entity
#[derive(Resource, Default)]
pub struct SelectionState {
    pub selected_entity: Option<Entity>,
}

// Events for selection changes
#[derive(Event)]
pub struct EntitySelectedEvent;

#[derive(Event)]
pub struct EntityDeselectedEvent;

// Observer system to handle selection logic using the Bevy picking system
pub fn handle_selection(
    click: Trigger<Pointer<Click>>,
    mut commands: Commands,
    mut selection_state: ResMut<SelectionState>,
) {
    info!("something");
    // Get entity from pointer interactions
    let entity = click.target();

    // Check if the clicked entity is already selected
    if selection_state.selected_entity == Some(entity) {
        return;
    } else {
        // Deselect any currently selected entity
        if let Some(selected_entity) = selection_state.selected_entity {
            commands.entity(selected_entity).remove::<Selected>();
            commands.trigger_targets(EntityDeselectedEvent, selected_entity);
        }

        // Select the new entity
        commands.entity(entity).insert(Selected);
        selection_state.selected_entity = Some(entity);
        commands.trigger_targets(EntitySelectedEvent, entity);
    }
}
