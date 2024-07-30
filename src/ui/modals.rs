use crate::state::AppStateRef;

mod delete_def;
mod download;
mod edit_tag;
mod link_vault;
mod manage_vaults;
mod message;
mod new_vault;
mod preview;
mod tag_shortcuts;
mod transform;

pub use delete_def::DeleteDefinition;
pub use download::Download;
pub use edit_tag::EditTag;
pub use link_vault::LinkVault;
pub use manage_vaults::ManageVaults;
pub use message::Message;
pub use new_vault::NewVault;
pub use preview::Preview;
pub use tag_shortcuts::TagShortcuts;
pub use transform::TransformImages;

pub trait AppModal: Send + Sync + 'static {
    fn id(&self) -> eframe::egui::Id;
    fn update(&mut self, ctx: &eframe::egui::Context, state: AppStateRef);
    fn dispose(&mut self, _ctx: &eframe::egui::Context, _state: AppStateRef) {}
    fn is_open(&self) -> bool;

    fn update_or_dispose(&mut self, ctx: &eframe::egui::Context, state: AppStateRef) -> bool {
        self.update(ctx, state.clone());
        let is_open = self.is_open();
        if !is_open {
            self.dispose(ctx, state);
        }
        is_open
    }
}
