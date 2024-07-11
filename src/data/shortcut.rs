use eframe::egui;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Default, serde::Serialize, serde::Deserialize)]
pub enum ShortcutAction {
    #[default]
    None,
    ToggleTag(Uuid)
}