use eframe::egui::Color32;
use egui_modal::Modal;
use poll_promise::Promise;

use crate::data::Vault;
use crate::state::AppStateRef;
use crate::tasks;
use crate::ui::modals::AppModal;

#[derive(Default)]
pub struct NewVaultDialog {
    modal: Option<Modal>,
    new_vault_name: String,
    error_message: Option<String>,
    opened: bool,
}

impl AppModal for NewVaultDialog {
    fn id(&self) -> eframe::egui::Id {
        "new_vault_name_modal".into()
    }

    fn update(&mut self, ctx: &eframe::egui::Context, state: AppStateRef) -> &mut dyn AppModal {
        let modal = Modal::new(ctx, self.id().value());

        modal.show(|ui| {
            modal.title(ui, "New vault");
            modal.frame(ui, |ui| {
                ui.label("Enter name of new vault:");
                ui.text_edit_singleline(&mut self.new_vault_name);

                if let Some(msg) = &self.error_message {
                    ui.colored_label(Color32::RED, msg);
                }
            });
            modal.buttons(ui, |ui| {
                if modal.suggested_button(ui, "Create").clicked() {
                    if self.new_vault_name.trim().is_empty() {
                        self.error_message = "Please enter a vault name.".to_string().into();
                        modal.open();
                    } else {
                        let Self { new_vault_name, .. } = std::mem::take(self);
                        let r = state.blocking_read();
                        r.set_vault_loading();
                        r.add_task("Create vault".into(), |s, p| {
                            Promise::spawn_async(tasks::vault::save_new_vault(
                                s,
                                Vault::new(new_vault_name),
                                p,
                            ))
                        });
                    }
                }
                modal.button(ui, "Cancel");
            });
        });

        if !self.opened {
            modal.open();
            self.opened = true;
        }

        self.modal = Some(modal);
        self
    }

    fn is_open(&self) -> bool {
        self.modal.as_ref().is_some_and(|m| m.is_open())
    }
}
