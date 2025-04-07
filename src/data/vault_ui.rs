use crate::data::{IndexMapSerial, ItemCache, ItemId, MainQuery, ShortcutBehaviour};
use crate::tasks::sort::{SortDirection, SortType};
use eframe::egui::KeyboardShortcut;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

macro_rules! shortcut {
    ($modifier:ident + $key:ident) => {
        eframe::egui::KeyboardShortcut::new(
            eframe::egui::Modifiers::$modifier,
            eframe::egui::Key::$key,
        )
    };
    ($key:ident) => {
        eframe::egui::KeyboardShortcut::new(eframe::egui::Modifiers::NONE, eframe::egui::Key::$key)
    };
}

const DEFAULT_SHORTCUTS: [KeyboardShortcut; 10] = [
    shortcut!(CTRL + Num1),
    shortcut!(CTRL + Num2),
    shortcut!(CTRL + Num3),
    shortcut!(CTRL + Num4),
    shortcut!(CTRL + Num5),
    shortcut!(CTRL + Num6),
    shortcut!(CTRL + Num7),
    shortcut!(CTRL + Num8),
    shortcut!(CTRL + Num9),
    shortcut!(CTRL + Num0),
];

#[derive(Serialize, Deserialize)]
pub struct VaultViewModel {
    pub search_text: String,

    pub shortcuts: IndexMapSerial<KeyboardShortcut, ShortcutBehaviour>,

    pub sort_type: SortType,
    pub sort_field_id: Option<Uuid>,
    pub sort_direction: SortDirection,

    pub init_row_height: f32,
}

impl Default for VaultViewModel {
    fn default() -> Self {
        Self {
            search_text: Default::default(),
            shortcuts: {
                let mut shortcuts = IndexMapSerial::default();
                for shortcut in DEFAULT_SHORTCUTS {
                    shortcuts.insert(shortcut, Default::default());
                }
                shortcuts
            },
            sort_type: Default::default(),
            sort_field_id: Default::default(),
            sort_direction: Default::default(),
            init_row_height: 128.0,
        }
    }
}

#[derive(Default)]
pub struct VaultCacheModel {
    pub query: MainQuery,
    pub item_list: ItemCache,
    pub selection: Vec<ItemId>,
}
