use crate::data::transform::{
    ChromaSubsampling, CompressionFileType, DestinationType, EsrganModel, FitAlgorithm,
    InfillTechnique, ScaleAlgorithm, SourceType,
};
use crate::data::{ItemId, TransformParams};
use crate::errors::AppError;
use crate::state::AppStateRef;
use crate::tasks::sort::sort_items_unstable;
use crate::tasks::AsyncTaskResult;
use crate::ui::cloneable_state::CloneableTempState;
use crate::ui::modals::AppModal;
use crate::ui::thumb_grid::ThumbnailGrid;
use crate::ui::{buttons, choice, indent};
use eframe::egui;
use eframe::egui::Color32;
use egui_modal::{Modal, ModalStyle};
use poll_promise::Promise;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::info;

#[derive(
    Debug, Default, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, derive_more::Display,
)]
enum FormSection {
    #[default]
    Source,
    Destination,
    Scale,
    Infill,
    Compression,
}

pub struct TransformImages {
    modal: Option<Modal>,
    preview_grid: ThumbnailGrid,
    selected_item_ids: Vec<ItemId>,
    selected_items_updated: bool,
    error_message: Option<String>,
    state: Option<State>,
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
            state: None,
            error_message: None,
            app_state: Default::default(),
            opened: false,
        }
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct State {
    is_open: bool,
    params: TransformParams,
    form_section: FormSection,
}

impl CloneableTempState for State {}

mod request {
    pub const LOAD_VAULT: &str = "load_vault";
    pub const CHOOSE_DIRECTORY: &str = "choose_directory";
    pub const CHOOSE_ARCHIVE: &str = "choose_archive";
}

impl TransformImages {
    fn state(&self) -> &State {
        self.state.as_ref().unwrap()
    }

    fn state_mut(&mut self) -> &mut State {
        self.state.as_mut().unwrap()
    }

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

    fn type_choice_grid_inner(&mut self, ui: &mut egui::Ui) {
        let (form_section, params) = {
            let state = self.state_mut();
            (&mut state.form_section, &mut state.params)
        };

        egui_extras::TableBuilder::new(ui)
            .column(egui_extras::Column::exact(18.0))
            .column(egui_extras::Column::remainder())
            .body(|mut body| {
                let row_height = 18.0;
                body.row(row_height, |mut row| {
                    row.col(|_| {});
                    row.col(|ui| choice(ui, form_section, FormSection::Source));
                });
                body.row(row_height, |mut row| {
                    row.col(|_| {});
                    row.col(|ui| choice(ui, form_section, FormSection::Destination));
                });
                body.row(row_height, |mut row| {
                    row.col(|ui| {
                        ui.checkbox(&mut params.scale_options.enabled, "");
                    });
                    row.col(|ui| choice(ui, form_section, FormSection::Scale));
                });
                body.row(row_height, |mut row| {
                    row.col(|ui| {
                        ui.checkbox(&mut params.infill_options.enabled, "");
                    });
                    row.col(|ui| choice(ui, form_section, FormSection::Infill));
                });
                body.row(row_height, |mut row| {
                    row.col(|ui| {
                        ui.checkbox(&mut params.compression_options.enabled, "");
                    });
                    row.col(|ui| choice(ui, form_section, FormSection::Compression));
                });
            });
    }

    fn source_fragment(&mut self, ui: &mut egui::Ui, p: &mut TransformParams) {
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
                    "Same vault",
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
                            ui.radio_value(&mut p.infill_options.use_auto_solid, true, "Automatic");
                        });
                    });
                    ui.end_row();

                    indent(ui, |ui| {
                        ui.add_enabled_ui(is_blur, |ui| {
                            ui.radio_value(&mut p.infill_options.use_auto_solid, false, "Manual: ");
                        });
                    });

                    ui.add_enabled_ui(is_blur, |ui| {
                        let mut srgba =
                            p.infill_options.manual_solid_colour.to_srgba_unmultiplied();
                        ui.color_edit_button_srgba_unmultiplied(&mut srgba);
                        p.infill_options.manual_solid_colour =
                            Color32::from_rgba_unmultiplied(srgba[0], srgba[1], srgba[2], srgba[3]);
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
                        ui.checkbox(&mut p.infill_options.use_brightness, "Brightness change: ");
                    });
                    #[allow(clippy::cast_possible_truncation)]
                    ui.add_enabled(
                        p.infill_options.use_brightness,
                        egui::Slider::new(&mut p.infill_options.brightness_change, -1.0..=1.0)
                            .custom_formatter(|v, _| format!("{}%", (v * 100.0).round() as isize))
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
                            .custom_formatter(|v, _| format!("{}%", (v * 100.0).round() as isize))
                            .fixed_decimals(2),
                    );
                    ui.end_row();
                });
        });
    }

    fn compression_fragment(&mut self, ui: &mut egui::Ui, p: &mut TransformParams) {
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
    }

    fn modal_contents(&mut self, ui: &mut egui::Ui) {
        let padding_y = self.preview_grid.params.item_padding.y;

        egui::TopBottomPanel::top(self.id().with("preview_panel"))
            .exact_height(256.0 + 2.0 * padding_y)
            .show_inside(ui, |ui| {
                egui::ScrollArea::horizontal()
                    .auto_shrink([false, false])
                    .show_viewport(ui, |ui, vp| {
                        self.preview_grid.params.init_row_height = 256.0;
                        self.preview_grid.params.container_width = 256.0;
                        self.preview_grid.params.last_row_align = egui::Align::Min;
                        self.preview_grid.params.main_axis = egui::Direction::TopDown;

                        self.preview_grid.update(
                            ui,
                            vp,
                            self.app_state.clone(),
                            &self.selected_item_ids,
                            self.selected_items_updated,
                        );
                    });
            });

        egui::CentralPanel::default().show_inside(ui, |ui| {
            egui::SidePanel::left(self.id().with("type_panel"))
                .min_width(100.0)
                .max_width(350.0)
                .show_inside(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            ui.with_layout(
                                egui::Layout::top_down(egui::Align::LEFT).with_cross_justify(true),
                                |ui| {
                                    self.type_choice_grid_inner(ui);
                                },
                            );
                        });
                });

            egui::CentralPanel::default().show_inside(ui, |ui| {
                egui::ScrollArea::both()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.vertical(|ui| {
                            let form_section = self.state().form_section;
                            let mut global_params = std::mem::take(&mut self.state_mut().params);

                            match form_section {
                                FormSection::Source => {
                                    self.source_fragment(ui, &mut global_params);
                                }
                                FormSection::Destination => {
                                    self.destination_fragment(ui, &mut global_params);
                                }
                                FormSection::Scale => {
                                    self.scaling_fragment(ui, &mut global_params);
                                }
                                FormSection::Infill => {
                                    self.infill_fragment(ui, &mut global_params);
                                }
                                FormSection::Compression => {
                                    self.compression_fragment(ui, &mut global_params);
                                }
                            }

                            self.state_mut().params = global_params;

                            if let Some(msg) = &self.error_message {
                                ui.colored_label(Color32::RED, msg);
                            }
                        });
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
        "transform_modal2".into()
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
        let mut opened = self.state().is_open;
        let mut do_close = false;

        let selected_items_new_last_frame = self.selected_items_updated;

        if !self.opened {
            self.update_selected_items(self.state().params.source_type)
                .expect("vault to exist");
        }

        egui::Window::new("Transform")
            .id(self.id())
            .open(&mut opened)
            .min_size([700.0, 250.0])
            .show(ctx, |ui| {
                buttons(self.id(), ui, |ui| {
                    if ui.button("Transform").clicked() {
                        info!("Clicked transform");
                    }
                    if ui.button("Close").clicked() {
                        do_close = false;
                    }
                    if ui.button("Reset").clicked() {
                        *self.state_mut() = Default::default();
                    }
                });

                self.modal_contents(ui);
            });

        if !self.opened {
            self.state_mut().is_open = true;
            self.opened = true;
        }

        self.state.take().unwrap().store(ctx, self.id());

        if selected_items_new_last_frame {
            self.selected_items_updated = false;
        }
        self.modal = Some(modal);
    }

    fn dispose(&mut self, ctx: &egui::Context, _state: AppStateRef) {
        State::dispose(ctx, self.id());
    }

    fn is_open(&self) -> bool {
        self.opened
    }
}
