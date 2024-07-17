use eframe::egui::Color32;
use egui_modal::Modal;
use poll_promise::Promise;

use crate::data::Vault;
use crate::state::AppStateRef;
use crate::tasks;
use crate::ui::modals::AppModal;

#[derive(Default)]
pub struct NewVault {
    modal: Option<Modal>,
    name: String,
    error_message: Option<String>,
    opened: bool,
}

impl AppModal for NewVault {
    fn id(&self) -> eframe::egui::Id {
        "new_vault_name_modal".into()
    }

    fn update(&mut self, ctx: &eframe::egui::Context, state: AppStateRef) -> &mut dyn AppModal {
        let modal = Modal::new(ctx, self.id().value());

        modal.show(|ui| {
            modal.title(ui, "New vault");
            modal.frame(ui, |ui| {
                ui.label("Enter name of new vault:");
                ui.text_edit_singleline(&mut self.name);

                if let Some(msg) = &self.error_message {
                    ui.colored_label(Color32::RED, msg);
                }
            });
            modal.buttons(ui, |ui| {
                if modal.suggested_button(ui, "Create").clicked() {
                    if self.name.trim().is_empty() {
                        self.error_message = "Please enter a vault name.".to_string().into();
                        modal.open();
                    } else {
                        let Self { name, .. } = std::mem::take(self);
                        state.set_vault_loading();
                        state.add_task("Create vault", |s, p| {
                            Promise::spawn_async(tasks::vault::save_new_vault(
                                s,
                                Vault::new(name),
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
