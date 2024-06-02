use std::collections::HashMap;

use eframe::egui;
use eframe::egui::Color32;
use poll_promise::Promise;

use crate::errors::AppError;
use crate::state::AppStateRef;
use crate::tasks::AsyncTaskResult;
use crate::ui::cloneable_state::CloneableTempState;
use crate::ui::modals::AppModal;
use crate::ui::theme;

#[derive(Default)]
pub struct ManageVaults {
    widget_state: State,
    opened: bool,
    updated: bool,
}

#[derive(Clone)]
struct State {
    error_message: Option<String>,
    vault_errors: HashMap<String, Option<String>>,
    vault_to_path: HashMap<String, String>,
    current_name: Option<String>,
    is_adding: bool,
    is_editing: bool,
    opened: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            error_message: None,
            vault_errors: HashMap::new(),
            vault_to_path: HashMap::new(),
            current_name: None,
            is_adding: false,
            is_editing: false,
            opened: true,
        }
    }
}

impl CloneableTempState for State {}

impl ManageVaults {
    fn table_row(&mut self, name: String, row: &mut egui_extras::Strip, state: AppStateRef) {
        let req_name = format!("manage_vaults_load_{name}");
        let error_message = self
            .widget_state
            .vault_errors
            .entry(name.clone())
            .or_insert(None);

        let path = self.widget_state.vault_to_path.get(&name);
        let is_current = self.widget_state.current_name.as_ref() == Some(&name);

        row.cell(|ui| {
            ui.label(&name);
        });

        row.cell(|ui| match (path, is_current) {
            (None, _) => {
                if let Some(error_message) = error_message {
                    let first_line = error_message.lines().next().unwrap_or("?");
                    ui.label(egui::RichText::new(first_line).color(theme::ERROR_TEXT))
                        .on_hover_text(error_message.clone());
                } else {
                    ui.label(egui::RichText::new("Not resolved").color(theme::ERROR_TEXT));
                }

                if ui.button("Open...").clicked() {
                    *error_message = None;
                    state.blocking_read().add_task_request(req_name, |s, p| {
                        Promise::spawn_async(crate::tasks::vault::choose_and_load_vault(s, p, true))
                    });
                }
            }
            (Some(path), true) => {
                ui.label(path);
            }
            (Some(path), false) => {
                ui.label(path);

                if ui.button("Change...").clicked() {
                    *error_message = None;
                    state.blocking_read().add_task_request(req_name, |s, p| {
                        Promise::spawn_async(crate::tasks::vault::choose_and_load_vault(s, p, true))
                    });
                }
            }
        });

        row.cell(|ui| {
            ui.with_layout(
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| match (path, is_current) {
                    (None, _) => {}
                    (Some(_), true) => {
                        ui.label(egui::RichText::new("Current vault").color(theme::PROGRESS_TEXT));
                    }
                    (Some(_), false) => {
                        if ui.button("Select").clicked() {
                            if let Err(e) =
                                state.blocking_read().set_current_vault_name(name.clone())
                            {
                                self.widget_state.error_message = Some(e.to_string());
                            }
                        }
                    }
                },
            );
        });
    }
    //noinspection DuplicatedCode
    fn edit_ui(&mut self, ui: &mut egui::Ui, state: AppStateRef) {
        let vault_names = {
            let r = state.blocking_read();

            self.widget_state.current_name = r.current_vault_name();

            let vault_names = r.known_vault_names();
            for name in vault_names.iter() {
                let error_message = self
                    .widget_state
                    .vault_errors
                    .entry(name.clone())
                    .or_insert(None);
                match r.try_take_request_result(name) {
                    None => {}
                    Some(Ok(AsyncTaskResult::VaultLoaded { name: res_name, .. }))
                        if &res_name == name =>
                    {
                        // expected, but state is already updated so we don't need to do anything
                    }
                    Some(Ok(AsyncTaskResult::VaultLoaded { name: res_name, .. })) => {
                        *error_message = Some(format!(
                            "Vault has wrong name (expected {name}, got {res_name})"
                        ))
                    }
                    Some(Ok(res)) => {
                        *error_message = Some(format!("Unexpected task result: {res:?}"))
                    }
                    Some(Err(e)) if AppError::UserCancelled.is_err(&e) => {}
                    Some(Err(e)) => *error_message = Some(e.to_string()),
                }
            }

            self.widget_state.vault_to_path = r.vault_name_to_file_paths();
            vault_names
        };

        egui::ScrollArea::vertical().show_viewport(ui, |ui, _vp| {
            ui.group(|ui| {
                ui.vertical_centered_justified(|ui| {
                    /*ui.push_id(self.id().with("table"), |ui| {
                        egui_extras::TableBuilder::new(ui)
                            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                            .column(egui_extras::Column::auto())
                            .column(egui_extras::Column::remainder().clip(true).at_least(300.0))
                            .column(egui_extras::Column::auto())
                            .auto_shrink([false, true])
                            .vscroll(false)
                            .body(|mut body| {
                                for name in vault_names {
                                    self.table_row(name, &mut body, state.clone());
                                }
                            });
                    });*/
                    egui_extras::StripBuilder::new(ui)
                        .sizes(egui_extras::Size::exact(48.0), vault_names.len())
                        /*.size(egui_extras::Size::initial(50.0).at_most(200.0))
                        .size(egui_extras::Size::remainder())
                        .size(egui_extras::Size::exact(100.0))*/
                        .vertical(|mut strip| {
                            for name in vault_names {
                                strip.strip(|builder| {
                                    builder
                                        .size(egui_extras::Size::exact(100.0))
                                        .size(egui_extras::Size::remainder())
                                        .size(egui_extras::Size::exact(100.0))
                                        .horizontal(|mut strip| {
                                            self.table_row(name, &mut strip, state.clone());
                                        });
                                });
                            }
                        })
                });
            });
        });
    }
}

impl AppModal for ManageVaults {
    fn id(&self) -> egui::Id {
        "manage_vaults_window".into()
    }

    fn update(&mut self, ctx: &egui::Context, app_state: AppStateRef) -> &mut dyn AppModal {
        self.widget_state = State::load(ctx, self.id()).unwrap_or_default();
        let prev_updated = self.updated;
        let mut opened = self.widget_state.opened;
        let error_message = self.widget_state.error_message.clone();

        let mut do_close = false;

        egui::Window::new("Manage vaults")
            .id(self.id())
            .open(&mut opened)
            .min_width(500.0)
            .show(ctx, |ui| {
                egui::TopBottomPanel::bottom("manage_vaults_window_bottom").show_inside(ui, |ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Close").clicked() {
                            do_close = true;
                        }
                    });
                });

                egui::CentralPanel::default().show_inside(ui, |ui| {
                    self.edit_ui(ui, app_state.clone());

                    if let Some(msg) = &error_message {
                        ui.colored_label(Color32::RED, msg);
                    }
                });
            });

        if prev_updated && self.updated {
            self.updated = false;
        }

        if do_close {
            opened = false;
        }

        self.widget_state.opened = opened;
        self.opened = self.widget_state.opened;
        std::mem::take(&mut self.widget_state).store(ctx, self.id());
        self
    }

    fn dispose(&mut self, ctx: &egui::Context, _state: AppStateRef) {
        State::dispose(ctx, self.id());
    }

    fn is_open(&self) -> bool {
        self.opened
    }
}
