use crate::data::Vault;
use crate::errors::AppError;
use crate::state::AppStateRef;
use crate::tasks::AsyncTaskResult;
use crate::ui::modals::AppModal;
use eframe::egui;
use eframe::egui::Color32;
use egui_modal::Modal;
use poll_promise::Promise;
use std::sync::Arc;

pub struct LinkVault {
    vault: Arc<Vault>,
    modal: Option<Modal>,
    selected_vault_name: String,
    error_message: Option<String>,
    opened: bool,
}

impl LinkVault {
    pub fn new(vault: Arc<Vault>) -> Self {
        Self {
            vault,
            modal: None,
            selected_vault_name: String::new(),
            error_message: None,
            opened: false,
        }
    }

    fn verify(&self) -> Result<(), String> {
        if self.selected_vault_name.is_empty() {
            return Err("Please select a vault to link.".to_string());
        }
        Ok(())
    }
}

impl AppModal for LinkVault {
    fn id(&self) -> egui::Id {
        "link_vault_modal".into()
    }

    fn update(&mut self, ctx: &egui::Context, state: AppStateRef) {
        let request_id = self.id().with("load_modal");
        let modal = Modal::new(ctx, self.id().value());

        let vault_names = state.valid_vault_names();
        match state.try_take_request_result(request_id) {
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

        modal.show(|ui| {
            modal.title(ui, "Link vault");
            modal.frame(ui, |ui| {
                ui.vertical(|ui| {
                    ui.label("Choose loaded vault:");
                    egui::ComboBox::new(self.id().with("choose_box"), "")
                        .selected_text(&self.selected_vault_name)
                        .show_ui(ui, |ui| {
                            let v = &mut self.selected_vault_name;
                            ui.selectable_value(v, String::new(), "--");
                            for vault_name in vault_names {
                                if &vault_name != &self.vault.name {
                                    ui.selectable_value(v, vault_name.clone(), vault_name);
                                }
                            }
                        });
                    ui.label("-- or --");
                    if ui.button("Load a vault...").clicked() {
                        state.add_task_request(request_id, "Load vault", |s, p| {
                            Promise::spawn_async(crate::tasks::vault::choose_and_load_vault(
                                s, p, false,
                            ))
                        });
                    }

                    if let Some(msg) = &self.error_message {
                        ui.colored_label(Color32::RED, msg);
                    }
                });
            });
            modal.buttons(ui, |ui| {
                if modal.suggested_button(ui, "Link").clicked() {
                    match self.verify() {
                        Ok(()) => {
                            let vault = self.vault.clone();
                            let Ok(other_vault) = state.get_vault_catch(&self.selected_vault_name)
                            else {
                                return;
                            };
                            state.add_global_task(
                                format!("Link with {}", &self.selected_vault_name),
                                |s, p| {
                                    Promise::spawn_async(crate::tasks::link::link_vaults_by_path(
                                        vault,
                                        other_vault,
                                        s,
                                        p,
                                    ))
                                },
                            );
                        }
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
    }

    fn is_open(&self) -> bool {
        self.modal.as_ref().is_some_and(|m| m.is_open())
    }
}
