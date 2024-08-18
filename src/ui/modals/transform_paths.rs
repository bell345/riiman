use crate::data::transform::{DestinationExistingBehaviour, DestinationKind, SourceKind};
use crate::data::{ItemId, TransformBulkParams, TransformPathParams};
use crate::errors::AppError;
use crate::state::AppStateRef;
use crate::tasks::sort::sort_items_unstable;
use crate::tasks::transform::{list_destination_paths, transform_path};
use crate::tasks::AsyncTaskResult;
use crate::ui::cloneable_state::CloneablePersistedState;
use crate::ui::modals::query::{DefaultButton, QueryKind};
use crate::ui::modals::{AppModal, QueryOptions};
use crate::ui::{
    behaviour_select, buttons, choice, indent, modals, radio_choice, theme, QueryResult,
};
use eframe::egui;
use egui_modal::{Modal, ModalStyle};
use poll_promise::Promise;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

#[derive(
    Debug, Default, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, derive_more::Display,
)]
enum FormSection {
    #[default]
    Source,
    Destination,
    Format,
    Summary,
}

pub struct TransformPaths {
    modal: Option<Modal>,
    source_item_ids: Vec<ItemId>,
    source_items_updated: bool,
    transformed_paths: Vec<(ItemId, String)>,
    unique_paths: HashSet<String>,
    n_conflicts: usize,
    n_duplicates: usize,
    error_message: Option<String>,
    state: Option<State>,
    app_state: AppStateRef,
    opened: bool,
    is_open: bool,
}

impl Default for TransformPaths {
    fn default() -> Self {
        Self {
            modal: None,
            source_item_ids: Default::default(),
            transformed_paths: Default::default(),
            unique_paths: Default::default(),
            n_conflicts: 0,
            n_duplicates: 0,
            source_items_updated: false,
            state: None,
            error_message: None,
            app_state: Default::default(),
            opened: false,
            is_open: true,
        }
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct State {
    transform_params: TransformPathParams,
    bulk_params: TransformBulkParams,
    form_section: FormSection,
}

impl CloneablePersistedState for State {}

mod request {
    pub const LOAD_VAULT: &str = "load_vault";
    pub const CHOOSE_DIRECTORY: &str = "choose_directory";
    pub const CHOOSE_ARCHIVE: &str = "choose_archive";
    pub const LOAD_PREVIEW: &str = "load_preview";
    pub const QUERY_CONFIRM: &str = "query_confirm";
}

impl TransformPaths {
    fn state(&self) -> &State {
        self.state.as_ref().unwrap()
    }

    fn state_mut(&mut self) -> &mut State {
        self.state.as_mut().unwrap()
    }

    fn source_len(&self, source_type: SourceKind) -> usize {
        match source_type {
            SourceKind::Selection => self.app_state.len_selected_items(),
            SourceKind::Filtered => self.app_state.len_item_list(),
            SourceKind::All => self
                .app_state
                .current_vault_opt()
                .map_or(0, |vault| vault.len_items()),
        }
    }

    fn update_selected_items(
        &mut self,
        bulk: &TransformBulkParams,
        params: &TransformPathParams,
    ) -> Result<(), ()> {
        let vault = self.app_state.current_vault_catch()?;
        let mut items = if bulk.source.kind == SourceKind::All {
            vault.iter_items().map(|i| Arc::clone(&i)).collect()
        } else {
            let ids = match bulk.source.kind {
                SourceKind::Selection => self.app_state.selected_item_ids(),
                SourceKind::Filtered => self.app_state.item_list_ids(),
                SourceKind::All => unreachable!(),
            };
            vault.resolve_item_ids(&ids)
        };

        self.app_state.catch(
            || "sorting preview selection",
            || sort_items_unstable(&mut items, &vault, &self.app_state.sorts()),
        )?;

        self.source_item_ids = items.iter().map(|i| ItemId::from_item(&vault, i)).collect();

        self.transformed_paths = items
            .into_iter()
            .filter_map(|i| {
                Some((
                    ItemId::from_item(&vault, &i),
                    transform_path(&i, params)?.to_string_lossy().into_owned(),
                ))
            })
            .collect();

        self.unique_paths = self
            .transformed_paths
            .iter()
            .map(|(_, p)| p.clone())
            .collect();

        self.n_conflicts = 0;
        if let Ok(dest_paths) = list_destination_paths(&bulk.destination, self.app_state.clone()) {
            let path_set: HashSet<_> = dest_paths.iter().map(|p| p.as_path()).collect();
            match bulk.destination.kind {
                DestinationKind::SameVault => {
                    self.n_conflicts = self
                        .transformed_paths
                        .iter()
                        // exclude items whose paths do not change
                        .filter(|(id, p)| {
                            !vault.get_item_opt_by_id(*id).is_some_and(|i| i.path() == p)
                        })
                        // count paths that collide with other items
                        .filter(|(_, p)| path_set.contains(Path::new(p)))
                        .count();
                }
                DestinationKind::OtherVault
                | DestinationKind::Directory
                | DestinationKind::Archive => {
                    self.n_conflicts = self
                        .transformed_paths
                        .iter()
                        .filter(|(_, p)| path_set.contains(Path::new(p)))
                        .count();
                }
            }
        }

        self.n_duplicates = self.transformed_paths.len() - self.unique_paths.len();
        self.source_items_updated = true;

        Ok(())
    }

    fn type_choice_grid_inner(&mut self, ui: &mut egui::Ui) {
        let form_section = &mut self.state_mut().form_section;

        choice(ui, form_section, FormSection::Source);
        choice(ui, form_section, FormSection::Destination);
        choice(ui, form_section, FormSection::Format);
        choice(ui, form_section, FormSection::Summary);
    }

    fn source_choice(&self, ui: &mut egui::Ui, value_ref: &mut SourceKind, value: SourceKind) {
        ui.radio_value(
            value_ref,
            value,
            format!("{} ({})", value, self.source_len(value)),
        );
    }

    fn source_fragment(&mut self, ui: &mut egui::Ui, p: &mut TransformBulkParams) {
        ui.vertical(|ui| {
            let source_kind = &mut p.source.kind;

            self.source_choice(ui, source_kind, SourceKind::Selection);
            self.source_choice(ui, source_kind, SourceKind::Filtered);
            self.source_choice(ui, source_kind, SourceKind::All);

            ui.add_space(ui.style().spacing.item_spacing.y * 2.0);

            let delete_source = &mut p.source.delete_source;
            ui.radio_value(delete_source, true, "Move (delete originals)");
            ui.radio_value(delete_source, false, "Copy (retain originals)");
        });
    }

    fn handle_request<R>(
        &mut self,
        req_id: impl std::hash::Hash,
        check_fn: impl FnOnce(AsyncTaskResult) -> Result<R, AsyncTaskResult>,
    ) -> Result<R, ()> {
        match self
            .app_state
            .try_take_request_result(self.id().with(req_id))
        {
            None => {}
            Some(Ok(res)) => match check_fn(res) {
                Ok(r) => return Ok(r),
                Err(res) => self.error_message = Some(format!("Unexpected task result: {res:?}")),
            },
            Some(Err(e)) if AppError::UserCancelled.is_err(&e) => {}
            Some(Err(e)) => self.error_message = Some(e.to_string()),
        };
        Err(())
    }

    fn destination_fragment(&mut self, ui: &mut egui::Ui, p: &mut TransformBulkParams) {
        if let Ok(name) = self.handle_request(request::LOAD_VAULT, |res| match res {
            AsyncTaskResult::VaultLoaded { name, .. } => Ok(name),
            _ => Err(res),
        }) {
            p.destination.other_vault_name = name;
        }

        if let Ok(dir) = self.handle_request(request::CHOOSE_DIRECTORY, |res| match res {
            AsyncTaskResult::SelectedDirectory(dir) => Ok(dir),
            _ => Err(res),
        }) {
            p.destination.directory_path = dir;
        }

        if let Ok(file) = self.handle_request(request::CHOOSE_ARCHIVE, |res| match res {
            AsyncTaskResult::SelectedFile(file) => Ok(file),
            _ => Err(res),
        }) {
            p.destination.archive_path = file;
        }

        self.destination_choice_fragment(ui, p);

        ui.add_space(ui.style().spacing.item_spacing.y * 2.0);

        ui.add_enabled_ui(
            matches!(
                p.destination.kind,
                DestinationKind::SameVault | DestinationKind::OtherVault
            ),
            |ui| {
                ui.label("Choose vault destination directory:");

                indent(ui, |ui| {
                    egui::Grid::new(self.id().with("destination_choice_grid"))
                        .num_columns(2)
                        .min_col_width(200.0 - ui.style().spacing.indent)
                        .show(ui, |ui| {
                            ui.radio_value(
                                &mut p.destination.use_subdirectory,
                                false,
                                "Root directory",
                            );
                            ui.end_row();
                            ui.radio_value(
                                &mut p.destination.use_subdirectory,
                                true,
                                "Sub directory:",
                            );
                            ui.add_enabled(
                                p.destination.use_subdirectory,
                                egui::TextEdit::singleline(&mut p.destination.vault_subdirectory),
                            );
                            ui.end_row();
                        });
                });
            },
        );

        ui.add_space(ui.style().spacing.item_spacing.y * 2.0);

        egui::Grid::new(self.id().with("behaviour_select"))
            .num_columns(2)
            .show(ui, |ui| {
                ui.add_enabled(
                    p.destination.kind == DestinationKind::Archive,
                    egui::Label::new("Behaviour when archive exists: "),
                );
                ui.add_enabled_ui(p.destination.kind == DestinationKind::Archive, |ui| {
                    behaviour_select(ui, &mut p.destination.archive_existing_behaviour);
                });
                ui.end_row();

                ui.label("Behaviour when item exists: ");
                behaviour_select(ui, &mut p.destination.item_existing_behaviour);
                ui.end_row();
            });

        ui.checkbox(
            &mut p.destination.preserve_directory_structure,
            "Preserve directory structure",
        );
    }

    #[allow(clippy::too_many_lines)]
    fn destination_choice_fragment(&mut self, ui: &mut egui::Ui, p: &mut TransformBulkParams) {
        let curr_name = self
            .app_state
            .current_vault_name()
            .expect("vault to be loaded");
        let vault_names = self.app_state.valid_vault_names();

        egui::Grid::new(self.id().with("destination_choice_grid"))
            .num_columns(2)
            .min_col_width(200.0)
            .show(ui, |ui| {
                let dest_kind = &mut p.destination.kind;
                radio_choice(ui, dest_kind, DestinationKind::SameVault);
                ui.end_row();

                ui.radio_value(dest_kind, DestinationKind::OtherVault, "Different vault:");
                ui.horizontal(|ui| {
                    ui.add_enabled_ui(*dest_kind == DestinationKind::OtherVault, |ui| {
                        egui::ComboBox::new(self.id().with("other_vault_choose_box"), "")
                            .selected_text(&p.destination.other_vault_name)
                            .show_ui(ui, |ui| {
                                let v = &mut p.destination.other_vault_name;
                                ui.selectable_value(v, String::new(), "--");
                                for vault_name in vault_names {
                                    if vault_name != curr_name {
                                        ui.selectable_value(v, vault_name.clone(), vault_name);
                                    }
                                }
                            });

                        if ui.button("Load a vault...").clicked() {
                            self.app_state.add_task_request(
                                self.id().with(request::LOAD_VAULT),
                                "Load vault",
                                |s, p| {
                                    Promise::spawn_async(
                                        crate::tasks::vault::choose_and_load_vault(s, p, false),
                                    )
                                },
                            );
                        }
                    });
                });
                ui.end_row();

                ui.radio_value(dest_kind, DestinationKind::Directory, "Directory:");
                ui.horizontal(|ui| {
                    ui.add_enabled_ui(*dest_kind == DestinationKind::Directory, |ui| {
                        ui.text_edit_singleline(&mut p.destination.directory_path);
                        if ui.button("Select...").clicked() {
                            self.app_state.add_task_request(
                                self.id().with(request::CHOOSE_DIRECTORY),
                                "Choose directory",
                                |_, _| Promise::spawn_async(crate::tasks::choose::choose_folder()),
                            );
                        }
                    });
                });
                ui.end_row();

                ui.radio_value(dest_kind, DestinationKind::Archive, "Archive:");

                ui.horizontal(|ui| {
                    ui.add_enabled_ui(*dest_kind == DestinationKind::Archive, |ui| {
                        ui.text_edit_singleline(&mut p.destination.archive_path);
                        if ui.button("Select...").clicked() {
                            self.app_state.add_task_request(
                                self.id().with(request::CHOOSE_ARCHIVE),
                                "Choose archive",
                                |_, _| Promise::spawn_async(crate::tasks::choose::choose_archive()),
                            );
                        }
                    });
                })
            });
    }

    fn format_fragment(&mut self, ui: &mut egui::Ui, p: &mut TransformPathParams) {
        /*
        let Some(vault) = self.app_state.current_vault_opt() else {
            return;
        };
        ui.vertical(|ui| {
            ui.label("Format specification:");

            widgets::SearchBox::new(self.id().with("format_box"), &mut p.format, vault)
                .interactive()
                .icon("")
                .placeholder("Enter format string...")
                .desired_width(ui.available_width())
                .show(ui);
        });
         */

        ui.label("Format is currently hardcoded as:");
        ui.label(egui::RichText::new("twitter_{user_id}_{post_id}_{num}.{ext}").monospace());

        ui.checkbox(&mut p.dry_run, "Is dry run?");
    }

    fn summary_fragment(
        &mut self,
        ui: &mut egui::Ui,
        bulk: &TransformBulkParams,
        p: &TransformPathParams,
    ) {
        egui::Grid::new(self.id().with("summary_grid"))
            .num_columns(2)
            .show(ui, |ui| {
                ui.label("Source: ");
                ui.label(format!(
                    "{} ({})",
                    bulk.source.kind,
                    self.source_len(bulk.source.kind)
                ));
                ui.end_row();

                ui.label("Destination:");
                ui.label(match bulk.destination.kind {
                    k @ DestinationKind::SameVault => k.to_string(),
                    k @ DestinationKind::OtherVault => {
                        format!("{k}: {}", bulk.destination.other_vault_name)
                    }
                    k @ DestinationKind::Directory => {
                        format!("{k}: {}", bulk.destination.directory_path)
                    }
                    k @ DestinationKind::Archive => {
                        format!("{k}: {}", bulk.destination.archive_path)
                    }
                });
                ui.end_row();

                ui.label("# of source items: ");
                ui.label(format!("{}", self.source_item_ids.len()));
                ui.end_row();

                ui.label("# of source items with valid transformed paths: ");
                ui.label(format!("{}", self.transformed_paths.len()));
                ui.end_row();

                ui.label("# of transformed paths: ");
                ui.label(format!("{}", self.unique_paths.len()));
                ui.end_row();

                if self.n_conflicts > 0 {
                    ui.colored_label(theme::ERROR_TEXT, "# of conflicts with existing items: ");
                    ui.colored_label(theme::ERROR_TEXT, format!("{}", self.n_conflicts));
                    ui.end_row();
                }

                if self.n_duplicates > 0 {
                    ui.colored_label(
                        theme::ERROR_TEXT,
                        "# of conflicts with other transformed paths: ",
                    );
                    ui.colored_label(theme::ERROR_TEXT, format!("{}", self.n_duplicates));
                    ui.end_row();
                }

                ui.label("Operation: ");
                ui.label(if bulk.source.delete_source {
                    "Move (delete originals)"
                } else {
                    "Copy (retain originals)"
                });
                ui.end_row();

                ui.label("Dry run: ");
                ui.label(if p.dry_run { "Yes" } else { "No" });
                ui.end_row();

                ui.label("Behaviour when item already exists: ");
                ui.label(format!("{}", bulk.destination.item_existing_behaviour));
                ui.end_row();
            });
    }

    fn top_preview_panel(&mut self, ui: &mut egui::Ui) {
        egui_extras::TableBuilder::new(ui)
            .column(egui_extras::Column::initial(400.0))
            .column(egui_extras::Column::remainder())
            .striped(true)
            .header(24.0, |mut row| {
                row.col(|ui| {
                    ui.label("Old");
                });
                row.col(|ui| {
                    ui.label("New");
                });
            })
            .body(|mut body| {
                let Some(vault) = self.app_state.current_vault_opt() else {
                    return;
                };
                body.rows(24.0, self.transformed_paths.len(), |mut row| {
                    let (item_id, transformed_path) = &self.transformed_paths[row.index()];
                    let Some(item) = vault.get_item_by_id(*item_id).ok() else {
                        return;
                    };
                    row.col(|ui| {
                        ui.label(item.path());
                    });
                    row.col(|ui| {
                        ui.label(transformed_path);
                    });
                });
            });
    }

    fn modal_contents(&mut self, ui: &mut egui::Ui) {
        egui::TopBottomPanel::top(self.id().with("preview_panel"))
            .exact_height(256.0)
            .show_inside(ui, |ui| {
                self.top_preview_panel(ui);
            });

        egui::SidePanel::left(self.id().with("type_panel"))
            .min_width(100.0)
            .max_width(350.0)
            .show_inside(ui, |ui| {
                egui::ScrollArea::both()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.with_layout(
                            egui::Layout::top_down(egui::Align::LEFT).with_cross_justify(true),
                            |ui| {
                                self.type_choice_grid_inner(ui);
                            },
                        );
                    });
            });

        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::central_panel(ui.style()))
                    .show_inside(ui, |ui| {
                        ui.vertical(|ui| {
                            let form_section = self.state().form_section;
                            let mut global_params =
                                std::mem::take(&mut self.state_mut().transform_params);
                            let mut bulk_params = std::mem::take(&mut self.state_mut().bulk_params);

                            match form_section {
                                FormSection::Source => self.source_fragment(ui, &mut bulk_params),
                                FormSection::Destination => {
                                    self.destination_fragment(ui, &mut bulk_params);
                                }
                                FormSection::Format => self.format_fragment(ui, &mut global_params),
                                FormSection::Summary => {
                                    self.summary_fragment(ui, &bulk_params, &global_params)
                                }
                            }

                            self.state_mut().transform_params = global_params;
                            self.state_mut().bulk_params = bulk_params;

                            if let Some(msg) = &self.error_message {
                                ui.colored_label(egui::Color32::RED, msg);
                            }
                        });
                    });
            });
    }

    fn validate(&self) -> Result<(), &'static str> {
        let s = &self.state().bulk_params.source;
        let d = &self.state().bulk_params.destination;
        if d.kind == DestinationKind::OtherVault && d.other_vault_name.is_empty() {
            return Err("Name of destination vault is required.");
        }

        if d.kind == DestinationKind::OtherVault
            && self.app_state.get_vault(&d.other_vault_name).is_err()
        {
            return Err("Chosen destination vault was not found.");
        }

        if d.kind == DestinationKind::Directory && d.directory_path.is_empty() {
            return Err("Name of destination directory is required.");
        }

        if d.kind == DestinationKind::Archive && d.archive_path.is_empty() {
            return Err("Name of destination archive is required.");
        }

        if d.kind == DestinationKind::Archive {
            return Err("Archive destinations are not yet supported.");
        }

        if d.use_subdirectory && d.vault_subdirectory.is_empty() {
            return Err("Name of vault destination subdirectory is required.");
        }

        if !s.delete_source && d.item_existing_behaviour == DestinationExistingBehaviour::Remove {
            return Err(
                "Behaviour when item already exists cannot be 'Remove' when copying items.",
            );
        }

        Ok(())
    }

    fn perform_transformation(&self) {
        let Ok(vault) = self.app_state.current_vault_catch() else {
            return;
        };
        let source_ids = self.source_item_ids.clone();
        let bulk = self.state().bulk_params.clone();
        let params = self.state().transform_params.clone();
        self.app_state.add_global_task("Transform paths", |s, p| {
            Promise::spawn_async(crate::tasks::transform::apply_path_transformations(
                s, vault, source_ids, bulk, params, p,
            ))
        });
    }
}

impl AppModal for TransformPaths {
    fn id(&self) -> egui::Id {
        "transform_paths_modal".into()
    }

    fn update(&mut self, ctx: &egui::Context, app_state: AppStateRef) {
        let modal = Modal::new(ctx, self.id().value()).with_style(&ModalStyle {
            default_width: Some(600.0),
            default_height: Some(280.0),
            ..Default::default()
        });
        self.state
            .replace(State::load(ctx, self.id()).unwrap_or_default());
        self.app_state = app_state;
        let mut is_open = self.is_open;
        let mut do_close = false;

        let selected_items_new_last_frame = self.source_items_updated;
        let old_transform_params = self.state().transform_params.clone();
        let old_bulk_params = self.state().bulk_params.clone();

        if !self.opened || selected_items_new_last_frame {
            self.update_selected_items(&old_bulk_params, &old_transform_params)
                .expect("vault to exist");
        }

        egui::Window::new("Transform")
            .id(self.id())
            .open(&mut is_open)
            .min_size([700.0, 250.0])
            .show(ctx, |ui| {
                buttons(self.id(), ui, |ui| {
                    if ui.button("Transform").clicked() {
                        if let Err(e) = self.validate() {
                            self.error_message = e.to_string().into();
                        } else if self.n_duplicates > 0 || self.n_conflicts > 0 {
                            let mut msg = String::new();
                            if self.n_duplicates > 0 {
                                msg.push_str(&format!("There will be {} items with the same transformed path.\n", self.n_duplicates));
                            }
                            if self.n_conflicts > 0 {
                                msg.push_str(&format!("There will be {} items with transformed paths that are the same as existing items.\n", self.n_conflicts));
                            }
                            msg.push_str(&format!("\nCurrent duplicate behaviour: {}\n\nContinue?", self.state().bulk_params.destination.item_existing_behaviour));

                            self.app_state.add_dialog(modals::Query::new(
                                self.id().with(request::QUERY_CONFIRM),
                                "Confirm",
                                msg,
                                QueryOptions {
                                    kind: QueryKind::YesNo,
                                    default_button: DefaultButton::Button2,
                                    icon: egui_modal::Icon::Warning,
                                }
                            ));
                        } else {
                            self.perform_transformation();
                            do_close = true;
                        }
                    }
                    if ui.button("Close").clicked() {
                        do_close = true;
                    }
                    if ui.button("Reset").clicked() {
                        *self.state_mut() = Default::default();
                    }
                });

                self.modal_contents(ui);
            });

        if selected_items_new_last_frame {
            self.source_items_updated = false;
        }

        if self.state().transform_params != old_transform_params {
            self.source_items_updated = true;
        }

        if self.state().bulk_params != old_bulk_params {
            self.source_items_updated = true;
        }

        if let Ok(query_res) = self.handle_request(request::QUERY_CONFIRM, |res| match res {
            AsyncTaskResult::QueryResult(query_res) => Ok(query_res),
            _ => Err(res),
        }) {
            if query_res == QueryResult::Yes {
                if let Err(e) = self.validate() {
                    self.error_message = e.to_string().into();
                    do_close = false;
                } else {
                    self.perform_transformation();
                    do_close = true;
                }
            }
        }

        if do_close {
            is_open = false;
        }

        self.is_open = is_open;
        self.opened = is_open;

        self.state.take().unwrap().store(ctx, self.id());
        self.modal = Some(modal);
    }

    fn is_open(&self) -> bool {
        self.is_open
    }
}
