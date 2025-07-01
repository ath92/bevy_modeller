use bevy::prelude::*;
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AppMode {
    Translate,
    Brush,
}

impl Default for AppMode {
    fn default() -> Self {
        AppMode::Translate
    }
}

#[derive(Resource)]
pub struct AppModeState {
    pub current_mode: AppMode,
    pub selection_enabled_modes: HashSet<AppMode>,
}

impl Default for AppModeState {
    fn default() -> Self {
        let mut selection_enabled_modes = HashSet::new();
        selection_enabled_modes.insert(AppMode::Translate);
        
        Self {
            current_mode: AppMode::Translate,
            selection_enabled_modes,
        }
    }
}

impl AppModeState {
    pub fn set_mode(&mut self, mode: AppMode) {
        self.current_mode = mode;
    }
    
    pub fn is_mode(&self, mode: AppMode) -> bool {
        self.current_mode == mode
    }
    
    pub fn is_selection_enabled(&self) -> bool {
        self.selection_enabled_modes.contains(&self.current_mode)
    }
    
    pub fn enable_selection_for_mode(&mut self, mode: AppMode) {
        self.selection_enabled_modes.insert(mode);
    }
    
    pub fn disable_selection_for_mode(&mut self, mode: AppMode) {
        self.selection_enabled_modes.remove(&mode);
    }
}

// System to handle mode switching
pub fn switch_mode(mut mode_state: ResMut<AppModeState>, mode: AppMode) {
    mode_state.set_mode(mode);
}

// Convenience functions for mode switching
pub fn switch_to_translate_mode(mut mode_state: ResMut<AppModeState>) {
    mode_state.set_mode(AppMode::Translate);
}

pub fn switch_to_brush_mode(mut mode_state: ResMut<AppModeState>) {
    mode_state.set_mode(AppMode::Brush);
}

pub struct ModePlugin;

impl Plugin for ModePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AppModeState>();
    }
}