use crate::data::{Item, Vault};
use eframe::egui;
use std::path::Path;

#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ItemId(egui::Id);

impl ItemId {
    pub fn from_item(vault: &Vault, item: &Item) -> Self {
        let rel_path = item.path();
        let abs_path_res = vault.resolve_abs_path(Path::new(rel_path));
        Self::from_rel_abs_path(rel_path, abs_path_res.ok().as_ref())
    }

    pub fn from_rel_abs_path(rel_path: &str, abs_path: Option<&String>) -> Self {
        Self(egui::Id::new(abs_path.map_or(rel_path, |s| s.as_str())))
    }

    pub fn to_egui_id(self, parent: egui::Id) -> egui::Id {
        parent.with(self.0)
    }
}
