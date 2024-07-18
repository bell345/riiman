use crate::data::transform::{
    ChromaSubsampling, CompressionFileType, DestinationType, EsrganModel, FitAlgorithm,
    InfillTechnique, ScaleAlgorithm, SourceType,
};
use crate::data::{ItemId, TransformParams};
use crate::errors::AppError;
use crate::state::AppStateRef;
use crate::tasks::sort::sort_items_unstable;
use crate::tasks::AsyncTaskResult;
use crate::ui::cloneable_state::CloneablePersistedState;
use crate::ui::modals::AppModal;
use crate::ui::thumb_grid::ThumbnailGrid;
use eframe::egui;
use eframe::egui::Color32;
use egui_modal::{Modal, ModalStyle};
use poll_promise::Promise;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::info;

pub struct TransformImages {
    modal: Option<Modal>,
    preview_grid: ThumbnailGrid,
    selected_item_ids: Vec<ItemId>,
    selected_items_updated: bool,
    params: TransformParams,
    error_message: Option<String>,
    app_state: AppStateRef,
    opened: bool,
}

impl Default for TransformImages {
    fn default() -> Self {
        Self {
            modal: None,
            preview_grid: ThumbnailGrid::new("transform_modal_preview_grid"),
            selected_item_ids: Default::default(),
            selected_items_updated: false,
            params: Default::default(),
            error_message: None,
            app_state: Default::default(),
            opened: false,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct State {
    params: TransformParams,
}

impl CloneablePersistedState for State {}

mod request {
    pub const LOAD_VAULT: &str = "load_vault";
    pub const CHOOSE_DIRECTORY: &str = "choose_directory";
    pub const CHOOSE_ARCHIVE: &str = "choose_archive";
}

fn indent<R>(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.horizontal(|ui| {
        ui.add_space(ui.style().spacing.indent);
        ui.vertical(add_contents).inner
    })
    .inner
}

fn choice<T: PartialEq + std::fmt::Display>(ui: &mut egui::Ui, value_ref: &mut T, alternative: T) {
    let label = alternative.to_string();
    ui.selectable_value(value_ref, alternative, label);
}

impl TransformImages {
    fn source_len(&self, source_type: SourceType) -> usize {
        match source_type {
            SourceType::Selection => self.app_state.len_selected_items(),
            SourceType::Filtered => self.app_state.len_item_list(),
            SourceType::All => self
                .app_state
                .current_vault_opt()
                .map_or(0, |vault| vault.len_items()),
        }
    }

    fn update_selected_items(&mut self, source_type: SourceType) -> Result<(), ()> {
        let vault = self.app_state.current_vault_catch()?;
        let mut items = if source_type == SourceType::All {
            vault.iter_items().map(|i| Arc::clone(&i)).collect()
        } else {
            let ids = match source_type {
                SourceType::Selection => self.app_state.selected_item_ids(),
                SourceType::Filtered => self.app_state.item_list_ids(),
                SourceType::All => unreachable!(),
            };
            vault.resolve_item_ids(&ids)
        };

        self.app_state.catch(
            || "sorting preview selection",
            || sort_items_unstable(&mut items, &vault, &self.app_state.sorts()),
        )?;

        self.selected_item_ids = items
            .into_iter()
            .map(|i| ItemId::from_item(&vault, &i))
            .collect();
        self.selected_items_updated = true;

        Ok(())
    }

    fn source_fragment(&mut self, ui: &mut egui::Ui, p: &mut TransformParams) {
        ui.heading("Source");

        ui.collapsing("Source options", |ui| {
            ui.vertical(|ui| {
                let source_type = &mut p.source_type;
                let old_source_type = *source_type;

                ui.radio_value(
                    source_type,
                    SourceType::Selection,
                    format!(
                        "Currently selected images ({})",
                        self.source_len(SourceType::Selection)
                    ),
                );
                ui.radio_value(
                    source_type,
                    SourceType::Filtered,
                    format!(
                        "All images which match filters ({})",
                        self.source_len(SourceType::Filtered)
                    ),
                );
                ui.radio_value(
                    source_type,
                    SourceType::All,
                    format!("All images in vault ({})", self.source_len(SourceType::All)),
                );

                if *source_type != old_source_type {
                    self.update_selected_items(*source_type)
                        .expect("vault to exist");
                }

                ui.add_space(ui.style().spacing.item_spacing.y * 2.0);

                ui.checkbox(
                    &mut p.source_options.delete_source,
                    "Delete source images after transformation",
                );
            });
        });

        ui.add_space(ui.style().spacing.item_spacing.y * 3.0);
    }

    fn handle_request<R>(
        &mut self,
        request_name: &str,
        check_fn: impl FnOnce(AsyncTaskResult) -> Result<R, AsyncTaskResult>,
    ) -> Result<R, ()> {
        match self.app_state.try_take_request_result(request_name) {
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

    fn destination_fragment(&mut self, ui: &mut egui::Ui, p: &mut TransformParams) {
        let req_prefix = self.id().with(ui.next_auto_id());
        let id = |suf: &str| req_prefix.with(suf).value().to_string();

        if let Ok(name) = self.handle_request(&id(request::LOAD_VAULT), |res| match res {
            AsyncTaskResult::VaultLoaded { name, .. } => Ok(name),
            _ => Err(res),
        }) {
            p.destination_options.other_vault_name = name;
        }

        if let Ok(dir) = self.handle_request(&id(request::CHOOSE_DIRECTORY), |res| match res {
            AsyncTaskResult::SelectedDirectory(dir) => Ok(dir),
            _ => Err(res),
        }) {
            p.destination_options.directory_path = dir;
        }

        if let Ok(file) = self.handle_request(&id(request::CHOOSE_ARCHIVE), |res| match res {
            AsyncTaskResult::SelectedFile(file) => Ok(file),
            _ => Err(res),
        }) {
            p.destination_options.archive_path = file;
        }

        ui.heading("Destination");

        ui.collapsing("Destination options", |ui| {
            self.destination_choice_fragment(ui, req_prefix, p);

            ui.add_space(ui.style().spacing.item_spacing.y * 2.0);

            ui.add_enabled_ui(
                matches!(
                    p.destination_type,
                    DestinationType::SameVault | DestinationType::OtherVault
                ),
                |ui| {
                    ui.label("Choose vault destination directory:");

                    indent(ui, |ui| {
                        egui::Grid::new(self.id().with("destination_choice_grid"))
                            .num_columns(2)
                            .min_col_width(200.0 - ui.style().spacing.indent)
                            .show(ui, |ui| {
                                ui.radio_value(
                                    &mut p.destination_options.use_subdirectory,
                                    false,
                                    "Root directory",
                                );
                                ui.end_row();
                                ui.radio_value(
                                    &mut p.destination_options.use_subdirectory,
                                    true,
                                    "Sub directory:",
                                );
                                ui.add_enabled(
                                    p.destination_options.use_subdirectory,
                                    egui::TextEdit::singleline(
                                        &mut p.destination_options.vault_subdirectory,
                                    ),
                                );
                                ui.end_row();
                            });
                    });
                },
            );

            ui.add_space(ui.style().spacing.item_spacing.y * 2.0);

            ui.add_enabled(
                p.destination_type == DestinationType::Archive,
                egui::Checkbox::new(
                    &mut p.destination_options.replace_archive_if_existing,
                    "Replace archive if existing",
                ),
            );
            ui.checkbox(
                &mut p.destination_options.replace_items_if_existing,
                "Replace items if existing",
            );
            ui.checkbox(
                &mut p.destination_options.preserve_directory_structure,
                "Preserve directory structure",
            );
        });

        ui.add_space(ui.style().spacing.item_spacing.y * 3.0);
    }

    #[allow(clippy::too_many_lines)]
    fn destination_choice_fragment(
        &mut self,
        ui: &mut egui::Ui,
        id_prefix: egui::Id,
        p: &mut TransformParams,
    ) {
        let id = |suf: &str| id_prefix.with(suf).value().to_string();

        let curr_name = self
            .app_state
            .current_vault_name()
            .expect("vault to be loaded");
        let vault_names = self.app_state.valid_vault_names();

        egui::Grid::new(self.id().with("destination_choice_grid"))
            .num_columns(2)
            .min_col_width(200.0)
            .show(ui, |ui| {
                ui.radio_value(
                    &mut p.destination_type,
                    DestinationType::SameVault,
                    "Replace existing images",
                );
                ui.end_row();

                ui.radio_value(
                    &mut p.destination_type,
                    DestinationType::OtherVault,
                    "Different vault:",
                );
                ui.horizontal(|ui| {
                    ui.add_enabled_ui(p.destination_type == DestinationType::OtherVault, |ui| {
                        egui::ComboBox::new(self.id().with("other_vault_choose_box"), "")
                            .selected_text(&p.destination_options.other_vault_name)
                            .show_ui(ui, |ui| {
                                let v = &mut p.destination_options.other_vault_name;
                                ui.selectable_value(v, String::new(), "--");
                                for vault_name in vault_names {
                                    if vault_name != curr_name {
                                        ui.selectable_value(v, vault_name.clone(), vault_name);
                                    }
                                }
                            });

                        if ui.button("Load a vault...").clicked() {
                            self.app_state
                                .add_task_request(id(request::LOAD_VAULT), |s, p| {
                                    Promise::spawn_async(
                                        crate::tasks::vault::choose_and_load_vault(s, p, false),
                                    )
                                });
                        }
                    });
                });
                ui.end_row();

                ui.radio_value(
                    &mut p.destination_type,
                    DestinationType::Directory,
                    "Directory:",
                );
                ui.horizontal(|ui| {
                    ui.add_enabled_ui(p.destination_type == DestinationType::Directory, |ui| {
                        ui.text_edit_singleline(&mut p.destination_options.directory_path);
                        if ui.button("Select...").clicked() {
                            self.app_state
                                .add_task_request(id(request::CHOOSE_DIRECTORY), |_, _| {
                                    Promise::spawn_async(crate::tasks::choose::choose_folder())
                                });
                        }
                    });
                });
                ui.end_row();

                ui.radio_value(
                    &mut p.destination_type,
                    DestinationType::Archive,
                    "Archive:",
                );

                ui.horizontal(|ui| {
                    ui.add_enabled_ui(p.destination_type == DestinationType::Archive, |ui| {
                        ui.text_edit_singleline(&mut p.destination_options.archive_path);
                        if ui.button("Select...").clicked() {
                            self.app_state
                                .add_task_request(id(request::CHOOSE_ARCHIVE), |_, _| {
                                    Promise::spawn_async(crate::tasks::choose::choose_archive())
                                });
                        }
                    });
                })
            });
    }

    fn scaling_fragment(&mut self, ui: &mut egui::Ui, p: &mut TransformParams) {
        ui.checkbox(&mut p.scale_options.enabled, "Enable scaling");

        ui.collapsing("Scaling options", |ui| {
            ui.add_enabled_ui(p.scale_options.enabled, |ui| {
                egui::Grid::new(self.id().with("scaling_options_width_height_grid"))
                    .num_columns(2)
                    .min_col_width(200.0)
                    .show(ui, |ui| {
                        ui.checkbox(&mut p.scale_options.use_target_width, "Target width: ");
                        ui.add_enabled(
                            p.scale_options.use_target_width,
                            egui::DragValue::new(&mut p.scale_options.target_width).suffix(" px"),
                        );
                        ui.end_row();

                        ui.checkbox(&mut p.scale_options.use_target_height, "Target height: ");
                        ui.add_enabled(
                            p.scale_options.use_target_height,
                            egui::DragValue::new(&mut p.scale_options.target_height).suffix(" px"),
                        );
                        ui.end_row();
                    });

                ui.add_space(ui.style().spacing.item_spacing.y * 2.0);

                let prefix_id = self.id();
                let scaling_id = self.id().with("scaling_options_grid");
                egui::Grid::new(scaling_id)
                    .num_columns(2)
                    .min_col_width(200.0)
                    .show(ui, |ui| {
                        let fit = &mut p.scale_options.fit_algorithm;
                        ui.label("Fit algorithm: ");
                        ui.horizontal(|ui| {
                            choice(ui, fit, FitAlgorithm::Fill);
                            choice(ui, fit, FitAlgorithm::Fit);
                            choice(ui, fit, FitAlgorithm::Stretch);
                        });
                        ui.end_row();

                        Self::scaling_algorithm_fragment(ui, prefix_id, p);

                        ui.checkbox(&mut p.scale_options.integer_scaling, "Use integer scaling");
                        ui.end_row();

                        ui.checkbox(
                            &mut p.scale_options.scale_down,
                            "Scale down oversized images",
                        );
                        ui.end_row();

                        ui.checkbox(
                            &mut p.scale_options.use_maximum_scaling,
                            "Use maximum scaling:",
                        );
                        ui.add_enabled(
                            p.scale_options.use_maximum_scaling,
                            egui::Slider::new(&mut p.scale_options.maximum_scaling, 1.0..=16.0)
                                .fixed_decimals(1),
                        );
                        ui.end_row();
                    });
            });
        });

        ui.add_space(ui.style().spacing.item_spacing.y * 2.0);
    }

    fn scaling_algorithm_fragment(ui: &mut egui::Ui, prefix_id: egui::Id, p: &mut TransformParams) {
        let combo_id = prefix_id.with("scaling_algorithm_choice");
        let esrgan_combo_id = prefix_id.with("esrgan_model_choice");
        let scale_algo = &mut p.scale_options.scale_algorithm;
        let esrgan_model = &mut p.scale_options.esrgan_model;

        ui.label("Scaling algorithm: ");
        egui::ComboBox::new(combo_id, "")
            .selected_text(scale_algo.to_string())
            .show_ui(ui, |ui| {
                choice(ui, scale_algo, ScaleAlgorithm::NearestNeighbour);
                choice(ui, scale_algo, ScaleAlgorithm::Bilinear);
                choice(ui, scale_algo, ScaleAlgorithm::Bicubic);
                choice(ui, scale_algo, ScaleAlgorithm::Hqx);
                choice(ui, scale_algo, ScaleAlgorithm::Xbr);
                choice(ui, scale_algo, ScaleAlgorithm::Esrgan);
            });
        ui.end_row();

        ui.add_enabled(
            *scale_algo == ScaleAlgorithm::Esrgan,
            egui::Label::new("Choose ESRGAN model: "),
        );
        ui.add_enabled_ui(*scale_algo == ScaleAlgorithm::Esrgan, |ui| {
            egui::ComboBox::new(esrgan_combo_id, "")
                .selected_text(esrgan_model.to_string())
                .show_ui(ui, |ui| {
                    choice(ui, esrgan_model, EsrganModel::RealEsrgan);
                    choice(ui, esrgan_model, EsrganModel::RealEsrganAnime);
                });
        });
        ui.end_row();
    }

    #[allow(clippy::too_many_lines)]
    fn infill_fragment(&mut self, ui: &mut egui::Ui, p: &mut TransformParams) {
        ui.checkbox(&mut p.infill_options.enabled, "Enable infill");

        ui.collapsing("Infill options", |ui| {
            ui.add_enabled_ui(p.infill_options.enabled, |ui| {
                egui::Grid::new(self.id().with("infill_options_ratio_grid"))
                    .num_columns(2)
                    .min_col_width(200.0)
                    .show(ui, |ui| {
                        ui.label("Target aspect ratio: ");
                        ui.horizontal(|ui| {
                            let (mut a, mut b) = p.infill_options.target_aspect_ratio;
                            ui.add(
                                egui::DragValue::new(&mut a)
                                    .fixed_decimals(0)
                                    .clamp_range(1..=32),
                            );
                            ui.label(" : ");
                            ui.add(
                                egui::DragValue::new(&mut b)
                                    .fixed_decimals(0)
                                    .clamp_range(1..=32),
                            );

                            p.infill_options.target_aspect_ratio = (a, b);
                        });
                        ui.end_row();
                    });

                ui.add_space(ui.style().spacing.item_spacing.y * 2.0);

                egui::Grid::new(self.id().with("infill_options_technique_grid"))
                    .num_columns(2)
                    .min_col_width(200.0)
                    .show(ui, |ui| {
                        let tech = &mut p.infill_options.technique;

                        ui.label("Technique: ");
                        ui.horizontal(|ui| {
                            choice(ui, tech, InfillTechnique::Automatic);
                            choice(ui, tech, InfillTechnique::Blur);
                            choice(ui, tech, InfillTechnique::Solid);
                        });
                        ui.end_row();

                        let is_blur = p.infill_options.technique == InfillTechnique::Blur;
                        ui.add_enabled(is_blur, egui::Label::new("Choose infill colour: "));
                        ui.end_row();

                        indent(ui, |ui| {
                            ui.add_enabled_ui(is_blur, |ui| {
                                ui.radio_value(
                                    &mut p.infill_options.use_auto_solid,
                                    true,
                                    "Automatic",
                                );
                            });
                        });
                        ui.end_row();

                        indent(ui, |ui| {
                            ui.add_enabled_ui(is_blur, |ui| {
                                ui.radio_value(
                                    &mut p.infill_options.use_auto_solid,
                                    false,
                                    "Manual: ",
                                );
                            });
                        });

                        ui.add_enabled_ui(is_blur, |ui| {
                            let mut srgba = self
                                .params
                                .infill_options
                                .manual_solid_colour
                                .to_srgba_unmultiplied();
                            ui.color_edit_button_srgba_unmultiplied(&mut srgba);
                            p.infill_options.manual_solid_colour = Color32::from_rgba_unmultiplied(
                                srgba[0], srgba[1], srgba[2], srgba[3],
                            );
                        });
                        ui.end_row();
                    });

                ui.add_space(ui.style().spacing.item_spacing.y * 2.0);

                egui::Grid::new(self.id().with("infill_options_postproc_grid"))
                    .num_columns(2)
                    .min_col_width(200.0)
                    .show(ui, |ui| {
                        ui.label("Post processing");
                        ui.end_row();

                        indent(ui, |ui| {
                            ui.checkbox(&mut p.infill_options.use_gaussian, "Gaussian blur: ");
                        });
                        ui.add_enabled(
                            p.infill_options.use_gaussian,
                            egui::Slider::new(&mut p.infill_options.gaussian_radius, 1..=64)
                                .suffix(" px"),
                        );
                        ui.end_row();

                        indent(ui, |ui| {
                            ui.checkbox(
                                &mut p.infill_options.use_brightness,
                                "Brightness change: ",
                            );
                        });
                        #[allow(clippy::cast_possible_truncation)]
                        ui.add_enabled(
                            p.infill_options.use_brightness,
                            egui::Slider::new(&mut p.infill_options.brightness_change, -1.0..=1.0)
                                .custom_formatter(|v, _| {
                                    format!("{}%", (v * 100.0).round() as isize)
                                })
                                .fixed_decimals(2),
                        );
                        ui.end_row();

                        indent(ui, |ui| {
                            ui.checkbox(&mut p.infill_options.use_contrast, "Contrast change: ");
                        });
                        #[allow(clippy::cast_possible_truncation)]
                        ui.add_enabled(
                            p.infill_options.use_contrast,
                            egui::Slider::new(&mut p.infill_options.contrast_change, -1.0..=1.0)
                                .custom_formatter(|v, _| {
                                    format!("{}%", (v * 100.0).round() as isize)
                                })
                                .fixed_decimals(2),
                        );
                        ui.end_row();
                    });
            });
        });

        ui.add_space(ui.style().spacing.item_spacing.y * 2.0);
    }

    fn compression_fragment(&mut self, ui: &mut egui::Ui, p: &mut TransformParams) {
        ui.checkbox(
            &mut p.compression_options.enabled,
            "Enable file type conversion",
        );

        ui.collapsing("Conversion options", |ui| {
            ui.add_enabled_ui(p.compression_options.enabled, |ui| {
                egui::Grid::new(self.id().with("compression_options_ratio_grid"))
                    .num_columns(2)
                    .min_col_width(200.0)
                    .show(ui, |ui| {
                        let file_type = &mut p.compression_options.file_type;

                        ui.label("File type: ");
                        ui.horizontal(|ui| {
                            choice(ui, file_type, CompressionFileType::Jpeg);
                            choice(ui, file_type, CompressionFileType::Png);
                            choice(ui, file_type, CompressionFileType::Webp);
                            choice(ui, file_type, CompressionFileType::Avif);
                        });
                        ui.end_row();

                        ui.label("Quality: ");
                        ui.add(egui::Slider::new(
                            &mut p.compression_options.quality,
                            0..=100,
                        ));
                        ui.end_row();

                        let chroma_id = self.id().with("chroma_select");
                        let chroma = &mut p.compression_options.chroma_subsampling;
                        ui.label("Chroma subsampling: ");
                        egui::ComboBox::new(chroma_id, "")
                            .selected_text(chroma.to_string())
                            .show_ui(ui, |ui| {
                                choice(ui, chroma, ChromaSubsampling::Chroma444);
                                choice(ui, chroma, ChromaSubsampling::Chroma440);
                                choice(ui, chroma, ChromaSubsampling::Chroma422);
                                choice(ui, chroma, ChromaSubsampling::Chroma420);
                            });
                        ui.end_row();
                    });
            });
        });
    }

    fn validate(&self) -> Result<(), &'static str> {
        Ok(())
    }
}

impl AppModal for TransformImages {
    fn id(&self) -> egui::Id {
        "transform_modal".into()
    }

    fn update(&mut self, ctx: &egui::Context, app_state: AppStateRef) -> &mut dyn AppModal {
        let modal = Modal::new(ctx, self.id().value()).with_style(&ModalStyle {
            default_width: Some(600.0),
            ..Default::default()
        });
        let mut state = State::load(ctx, self.id()).unwrap_or_default();
        self.params = std::mem::take(&mut state.params);
        self.app_state = app_state;

        let selected_items_new_last_frame = self.selected_items_updated;

        if !self.opened {
            self.update_selected_items(self.params.source_type)
                .expect("vault to exist");
        }

        modal.show(|ui| {
            modal.title(ui, "Transform");
            modal.frame(ui, |ui| {
                let padding_y = self.preview_grid.params.padding.y;
                egui::TopBottomPanel::top(self.id().with("preview_panel"))
                    .exact_height(256.0 + 2.0 * padding_y)
                    .show_inside(ui, |ui| {
                        egui::ScrollArea::horizontal()
                            .auto_shrink([false, false])
                            .show_viewport(ui, |ui, vp| {
                                self.preview_grid.params.max_row_height = 256.0;
                                self.preview_grid.params.container_width = f32::INFINITY;
                                self.preview_grid.params.last_row_align = egui::Align::Min;

                                self.preview_grid.update(
                                    ui,
                                    vp,
                                    self.app_state.clone(),
                                    &self.selected_item_ids,
                                    self.selected_items_updated,
                                );
                            });
                    });

                egui::ScrollArea::both()
                    .max_width(600.0)
                    .max_height(400.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.vertical(|ui| {
                            let mut global_params = std::mem::take(&mut self.params);

                            self.source_fragment(ui, &mut global_params);

                            self.destination_fragment(ui, &mut global_params);

                            ui.heading("Operations");

                            self.scaling_fragment(ui, &mut global_params);

                            self.infill_fragment(ui, &mut global_params);

                            self.compression_fragment(ui, &mut global_params);

                            self.params = global_params;

                            if let Some(msg) = &self.error_message {
                                ui.colored_label(Color32::RED, msg);
                            }
                        });
                    });
            });
            modal.buttons(ui, |ui| {
                if modal.suggested_button(ui, "Transform").clicked() {
                    info!("Clicked transform");
                }
                modal.button(ui, "Cancel");
                if modal.caution_button(ui, "Reset").clicked() {
                    self.params = Default::default();
                    modal.open();
                }
            });
        });

        if !self.opened {
            modal.open();
            self.opened = true;
        }

        state.params = std::mem::take(&mut self.params);
        state.store(ctx, self.id());

        if selected_items_new_last_frame {
            self.selected_items_updated = false;
        }
        self.modal = Some(modal);
        self
    }

    fn is_open(&self) -> bool {
        self.modal.as_ref().is_some_and(|m| m.is_open())
    }
}
