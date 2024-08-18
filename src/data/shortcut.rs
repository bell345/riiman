use eframe::egui;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ShortcutAction {
    #[default]
    None,
    ToggleTag(Uuid),
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ShortcutBehaviour {
    pub action: ShortcutAction,
    pub move_next: bool,
}
