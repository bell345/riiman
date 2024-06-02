use eframe::egui;
use eframe::egui::Color32;
use egui_modal::Modal;
use poll_promise::Promise;

use crate::errors::AppError;
use crate::state::AppStateRef;
use crate::tasks::AsyncTaskResult;
use crate::ui::modals::AppModal;

#[derive(Default)]
pub struct LinkVault {
    modal: Option<Modal>,
    selected_vault_name: String,
    error_message: Option<String>,
    opened: bool,
}

impl LinkVault {
    fn verify(&self) -> Result<(), String> {
        if self.selected_vault_name.is_empty() {
            return Err("Please select a vault to link.".to_string());
        }
        Ok(())
    }
}

impl AppModal for LinkVault {
    fn id(&self) -> eframe::egui::Id {
        "link_vault_modal".into()
    }

    fn update(&mut self, ctx: &eframe::egui::Context, state: AppStateRef) -> &mut dyn AppModal {
        let request_name = "link_vault_modal_load_request".to_string();
        let modal = Modal::new(ctx, self.id().value());

        let r = state.blocking_read();
        let vault_names = r.valid_vault_names();
        match r.try_take_request_result(&request_name) {
            None => {}
            Some(Ok(AsyncTaskResult::VaultLoaded {
                name: loaded_vault_name,
                ..
            })) => {
                self.selected_vault_name = loaded_vault_name;
            }
            Some(Ok(res)) => self.error_message = Some(format!("Unexpected task result: {res:?}")),
            Some(Err(e)) if AppError::UserCancelled.is_err(&e) => {}
            Some(Err(e)) => self.error_message = Some(e.to_string()),
        }
        drop(r);

        modal.show(|ui| {
            modal.title(ui, "Link vault");
            modal.frame(ui, |ui| {
                ui.label("Choose loaded vault:");
                egui::ComboBox::new(self.id().with("choose_box"), "")
                    .selected_text(&self.selected_vault_name)
                    .show_ui(ui, |ui| {
                        let v = &mut self.selected_vault_name;
                        ui.selectable_value(v, String::from(""), "--");
                        for vault_name in vault_names {
                            ui.selectable_value(v, vault_name.clone(), vault_name);
                        }
                    });
                ui.label("-- or --");
                if ui.button("Load a vault...").clicked() {
                    state
                        .blocking_read()
                        .add_task_request(request_name, |s, p| {
                            Promise::spawn_async(crate::tasks::vault::choose_and_load_vault(
                                s, p, true,
                            ))
                        });
                }

                if let Some(msg) = &self.error_message {
                    ui.colored_label(Color32::RED, msg);
                }
            });
            modal.buttons(ui, |ui| {
                if modal.suggested_button(ui, "Link").clicked() {
                    match self.verify() {
                        Ok(_) => {}
                        Err(e) => {
                            self.error_message = Some(e);
                            modal.open();
                        }
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
