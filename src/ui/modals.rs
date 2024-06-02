use crate::state::AppStateRef;

mod delete_def;
mod download;
mod edit_tag;
mod link_vault;
mod manage_vaults;
mod message;
mod new_vault;

pub use delete_def::DeleteDefinition;
pub use download::Download;
pub use edit_tag::EditTagDialog;
pub use link_vault::LinkVault;
pub use manage_vaults::ManageVaults;
pub use message::MessageDialog;
pub use new_vault::NewVaultDialog;

pub trait AppModal: Send + Sync + 'static {
    fn id(&self) -> eframe::egui::Id;
    fn update(&mut self, ctx: &eframe::egui::Context, state: AppStateRef) -> &mut dyn AppModal;
    fn dispose(&mut self, _ctx: &eframe::egui::Context, _state: AppStateRef) {}
    fn is_open(&self) -> bool;

    fn update_or_dispose(&mut self, ctx: &eframe::egui::Context, state: AppStateRef) -> bool {
        let is_open = self.update(ctx, state.clone()).is_open();
        if !is_open {
            self.dispose(ctx, state);
        }
        is_open
    }
}
