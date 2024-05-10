use crate::state::AppStateRef;

pub(crate) mod edit_tag;
pub(crate) mod message;
pub(crate) mod new_vault;

pub trait AppModal {
    fn update(&mut self, ctx: &eframe::egui::Context, state: AppStateRef) -> &mut dyn AppModal;
    fn is_open(&self) -> bool;
}
