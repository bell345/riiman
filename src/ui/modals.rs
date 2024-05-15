use crate::state::AppStateRef;

pub(crate) mod edit_tag;
pub(crate) mod message;
pub(crate) mod new_vault;

pub trait AppModal {
    fn update(&mut self, ctx: &eframe::egui::Context, state: AppStateRef) -> &mut dyn AppModal;
    fn dispose(&mut self, ctx: &eframe::egui::Context, state: AppStateRef);
    fn is_open(&self) -> bool;

    fn update_or_dispose(&mut self, ctx: &eframe::egui::Context, state: AppStateRef) -> bool {
        let is_open = self.update(ctx, state.clone()).is_open();
        if !is_open {
            self.dispose(ctx, state);
        }
        is_open
    }
}
