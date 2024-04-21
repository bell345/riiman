use eframe::egui::{Color32, TextBuffer};
use egui_modal::Modal;

#[derive(Default)]
pub struct NewVaultDialog {
    modal: Option<Modal>,
    new_vault_name: String,
    error_message: Option<String>,
    is_ready: bool,
}

impl NewVaultDialog {
    pub fn update(&mut self, ctx: &eframe::egui::Context) -> &mut Self {
        let modal = Modal::new(ctx, "new_vault_name_modal");

        modal.show(|ui| {
            modal.title(ui, "New vault name");
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
                        self.is_ready = true;
                    }
                }
                modal.button(ui, "Cancel");
            });
        });

        self.modal = Some(modal);
        self
    }

    pub fn ready(&mut self) -> Option<String> {
        if self.is_ready {
            let new_vault_name = self.new_vault_name.take().trim().into();
            *self = Default::default();
            return Some(new_vault_name);
        }
        None
    }

    pub fn open(&mut self) {
        self.modal.take().unwrap().open();
    }
}
