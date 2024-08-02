use crate::data::transform::{
    ChromaSubsampling, CompressionFileType, DestinationExistingBehaviour, DestinationKind,
    EsrganModel, FitAlgorithm, InfillTechnique, ScaleAlgorithm, SourceKind,
};
use crate::data::{ItemId, TransformBulkParams, TransformPathParams};
use crate::errors::AppError;
use crate::state::AppStateRef;
use crate::tasks::sort::sort_items_unstable;
use crate::tasks::transform::transform_path;
use crate::tasks::AsyncTaskResult;
use crate::ui::cloneable_state::CloneablePersistedState;
use crate::ui::modals::AppModal;
use crate::ui::{buttons, choice, indent, theme, widgets};
use eframe::egui;
use egui_modal::{Modal, ModalStyle};
use poll_promise::Promise;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use tracing::info;

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

    fn update_selected_items(&mut self, source_kind: SourceKind) -> Result<(), ()> {
        let vault = self.app_state.current_vault_catch()?;
        let mut items = if source_kind == SourceKind::All {
            vault.iter_items().map(|i| Arc::clone(&i)).collect()
        } else {
            let ids = match source_kind {
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

        let params = &self.state().transform_params;
        self.transformed_paths = items
            .into_iter()
            .filter_map(|i| Some((ItemId::from_item(&vault, &i), transform_path(&i, params)?)))
            .collect();

        self.unique_paths = self
            .transformed_paths
            .iter()
            .map(|(_, p)| p.clone())
            .collect();

        self.n_conflicts = self
            .transformed_paths
            .iter()
            .filter_map(|(_, p)| {
                vault.get_item(Path::new(&p)).ok()?;
                Some(p.clone())
            })
            .count();

        self.n_duplicates = self.transformed_paths.len() - self.unique_paths.len();
        self.source_items_updated = true;

        Ok(())
    }

    fn type_choice_grid_inner(&mut self, ui: &mut egui::Ui) {
        let form_section = &mut self.state_mut().form_section;

        choice(ui, form_section, FormSection::Source);
        choice(ui, form_section, FormSection::Destination);
        choice(ui, form_section, FormSection::Format);
        choice(ui, form_section, FormSection::Summary)
    }

    fn source_fragment(&mut self, ui: &mut egui::Ui, p: &mut TransformBulkParams) {
        ui.vertical(|ui| {
            let source_kind = &mut p.source.kind;
            let old_source_kind = *source_kind;

            ui.radio_value(
                source_kind,
                SourceKind::Selection,
                format!(
                    "Currently selected images ({})",
                    self.source_len(SourceKind::Selection)
                ),
            );
            ui.radio_value(
                source_kind,
                SourceKind::Filtered,
                format!(
                    "All images which match filters ({})",
                    self.source_len(SourceKind::Filtered)
                ),
            );
            ui.radio_value(
                source_kind,
                SourceKind::All,
                format!("All images in vault ({})", self.source_len(SourceKind::All)),
            );

            if *source_kind != old_source_kind {
                self.update_selected_items(*source_kind)
                    .expect("vault to exist");
            }

            ui.add_space(ui.style().spacing.item_spacing.y * 2.0);

            ui.checkbox(
                &mut p.source.delete_source,
                "Delete source images after transformation",
            );
        });
    }

    fn handle_request<R>(
        &mut self,
        req_id: egui::Id,
        check_fn: impl FnOnce(AsyncTaskResult) -> Result<R, AsyncTaskResult>,
    ) -> Result<R, ()> {
        match self.app_state.try_take_request_result(req_id) {
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
        let req_prefix = self.id().with(ui.next_auto_id());

        if let Ok(name) = self.handle_request(req_prefix.with(request::LOAD_VAULT), |res| match res
        {
            AsyncTaskResult::VaultLoaded { name, .. } => Ok(name),
            _ => Err(res),
        }) {
            p.destination.other_vault_name = name;
        }

        if let Ok(dir) =
            self.handle_request(
                req_prefix.with(request::CHOOSE_DIRECTORY),
                |res| match res {
                    AsyncTaskResult::SelectedDirectory(dir) => Ok(dir),
                    _ => Err(res),
                },
            )
        {
            p.destination.directory_path = dir;
        }

        if let Ok(file) =
            self.handle_request(req_prefix.with(request::CHOOSE_ARCHIVE), |res| match res {
                AsyncTaskResult::SelectedFile(file) => Ok(file),
                _ => Err(res),
            })
        {
            p.destination.archive_path = file;
        }

        self.destination_choice_fragment(ui, req_prefix, p);

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
                    let behaviour = &mut p.destination.archive_existing_behaviour;
                    egui::ComboBox::new(self.id().with("archive_behaviour_select"), "")
                        .selected_text(format!("{behaviour}"))
                        .show_ui(ui, |ui| {
                            choice(ui, behaviour, DestinationExistingBehaviour::Skip);
                            choice(ui, behaviour, DestinationExistingBehaviour::Remove);
                            choice(ui, behaviour, DestinationExistingBehaviour::Overwrite);
                            choice(
                                ui,
                                behaviour,
                                DestinationExistingBehaviour::AppendDiscriminator,
                            );
                        });
                });
                ui.end_row();

                let behaviour = &mut p.destination.item_existing_behaviour;
                ui.label("Behaviour when item exists: ");
                egui::ComboBox::new(self.id().with("item_behaviour_select"), "")
                    .selected_text(format!("{behaviour}"))
                    .show_ui(ui, |ui| {
                        choice(ui, behaviour, DestinationExistingBehaviour::Skip);
                        choice(ui, behaviour, DestinationExistingBehaviour::Remove);
                        choice(ui, behaviour, DestinationExistingBehaviour::Overwrite);
                        choice(
                            ui,
                            behaviour,
                            DestinationExistingBehaviour::AppendDiscriminator,
                        );
                    });
                ui.end_row();
            });

        ui.checkbox(
            &mut p.destination.preserve_directory_structure,
            "Preserve directory structure",
        );
    }

    #[allow(clippy::too_many_lines)]
    fn destination_choice_fragment(
        &mut self,
        ui: &mut egui::Ui,
        id_prefix: egui::Id,
        p: &mut TransformBulkParams,
    ) {
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
                ui.radio_value(dest_kind, DestinationKind::SameVault, "Same vault");
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
                                id_prefix.with(request::LOAD_VAULT),
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
                                id_prefix.with(request::CHOOSE_DIRECTORY),
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
                                id_prefix.with(request::CHOOSE_ARCHIVE),
                                "Choose archive",
                                |_, _| Promise::spawn_async(crate::tasks::choose::choose_archive()),
                            );
                        }
                    });
                })
            });
    }

    fn format_fragment(&mut self, ui: &mut egui::Ui, p: &mut TransformPathParams) {
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
    }

    fn summary_fragment(
        &mut self,
        ui: &mut egui::Ui,
        bulk: &TransformBulkParams,
        p: &TransformPathParams,
    ) {
        let Ok(vault) = self.app_state.current_vault() else {
            return;
        };

        egui::Grid::new(self.id().with("summary_grid"))
            .num_columns(2)
            .show(ui, |ui| {
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
        Ok(())
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

        if !self.opened {
            self.update_selected_items(self.state().bulk_params.source.kind)
                .expect("vault to exist");
        }

        egui::Window::new("Transform")
            .id(self.id())
            .open(&mut is_open)
            .min_size([700.0, 250.0])
            .show(ctx, |ui| {
                buttons(self.id(), ui, |ui| {
                    if ui.button("Transform").clicked() {
                        info!("Clicked transform");
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
