use crate::data::transform::{
    ChromaSubsampling, CompressionFileType, DestinationExistingBehaviour, DestinationKind,
    EsrganModel, FitAlgorithm, InfillTechnique, ScaleAlgorithm, SourceKind,
};
use crate::data::{ItemId, TransformBulkParams, TransformImageParams};
use crate::errors::AppError;
use crate::state::AppStateRef;
use crate::tasks::sort::sort_items_unstable;
use crate::tasks::transform::{get_transformed_size, load_transformed_image_preview};
use crate::tasks::AsyncTaskResult;
use crate::ui::cloneable_state::CloneablePersistedState;
use crate::ui::modals::AppModal;
use crate::ui::thumb_grid::ThumbnailGrid;
use crate::ui::{behaviour_select, buttons, choice, indent};
use eframe::egui;
use egui_modal::{Modal, ModalStyle};
use poll_promise::Promise;
use serde::{Deserialize, Serialize};
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
    Scale,
    Infill,
    Compression,
}

pub struct TransformImages {
    modal: Option<Modal>,
    preview_grid: ThumbnailGrid,
    source_item_ids: Vec<ItemId>,
    source_items_updated: bool,
    selected_item_id: Option<ItemId>,
    selected_preview_hndl: Option<egui::TextureHandle>,
    params_of_selected_preview: Option<TransformImageParams>,
    error_message: Option<String>,
    state: Option<State>,
    app_state: AppStateRef,
    opened: bool,
    is_open: bool,
}

impl Default for TransformImages {
    fn default() -> Self {
        Self {
            modal: None,
            preview_grid: ThumbnailGrid::new("transform_modal_preview_grid"),
            source_item_ids: Default::default(),
            source_items_updated: false,
            selected_item_id: None,
            selected_preview_hndl: None,
            params_of_selected_preview: None,
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
    transform_params: TransformImageParams,
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

impl TransformImages {
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

        self.source_item_ids = items
            .into_iter()
            .map(|i| ItemId::from_item(&vault, &i))
            .collect();
        self.source_items_updated = true;

        Ok(())
    }

    fn type_choice_grid_inner(&mut self, ui: &mut egui::Ui) {
        let (form_section, params) = {
            let state = self.state_mut();
            (&mut state.form_section, &mut state.transform_params)
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
                        ui.checkbox(&mut params.scale.enabled, "");
                    });
                    row.col(|ui| choice(ui, form_section, FormSection::Scale));
                });
                body.row(row_height, |mut row| {
                    row.col(|ui| {
                        ui.checkbox(&mut params.infill.enabled, "");
                    });
                    row.col(|ui| choice(ui, form_section, FormSection::Infill));
                });
                body.row(row_height, |mut row| {
                    row.col(|ui| {
                        ui.checkbox(&mut params.compression.enabled, "");
                    });
                    row.col(|ui| choice(ui, form_section, FormSection::Compression));
                });
            });
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
            let old_source_kind = *source_kind;

            self.source_choice(ui, source_kind, SourceKind::Selection);
            self.source_choice(ui, source_kind, SourceKind::Filtered);
            self.source_choice(ui, source_kind, SourceKind::All);

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

    fn scaling_fragment(&mut self, ui: &mut egui::Ui, p: &mut TransformImageParams) {
        ui.add_enabled_ui(p.scale.enabled, |ui| {
            egui::Grid::new(self.id().with("scaling_options_width_height_grid"))
                .num_columns(2)
                .min_col_width(200.0)
                .show(ui, |ui| {
                    ui.checkbox(&mut p.scale.use_target_width, "Target width: ");
                    ui.add_enabled(
                        p.scale.use_target_width,
                        egui::DragValue::new(&mut p.scale.target_width).suffix(" px"),
                    );
                    ui.end_row();

                    ui.checkbox(&mut p.scale.use_target_height, "Target height: ");
                    ui.add_enabled(
                        p.scale.use_target_height,
                        egui::DragValue::new(&mut p.scale.target_height).suffix(" px"),
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
                    let fit_enabled = p.scale.use_target_width && p.scale.use_target_height;
                    let fit = &mut p.scale.fit_algorithm;
                    ui.add_enabled(fit_enabled, egui::Label::new("Fit algorithm: "));
                    ui.add_enabled_ui(fit_enabled, |ui| {
                        ui.horizontal(|ui| {
                            choice(ui, fit, FitAlgorithm::Fill);
                            choice(ui, fit, FitAlgorithm::Fit);
                            choice(ui, fit, FitAlgorithm::Stretch);
                        });
                    });
                    ui.end_row();

                    Self::scaling_algorithm_fragment(ui, prefix_id, p);

                    ui.checkbox(&mut p.scale.integer_scaling, "Use integer scaling");
                    ui.end_row();

                    ui.checkbox(&mut p.scale.scale_down, "Scale down oversized images");
                    ui.end_row();

                    ui.checkbox(&mut p.scale.use_maximum_scaling, "Use maximum scaling:");
                    ui.add_enabled(
                        p.scale.use_maximum_scaling,
                        egui::Slider::new(&mut p.scale.maximum_scaling.0, 1.0..=16.0)
                            .fixed_decimals(1),
                    );
                    ui.end_row();
                });
        });
    }

    fn scaling_algorithm_fragment(
        ui: &mut egui::Ui,
        prefix_id: egui::Id,
        p: &mut TransformImageParams,
    ) {
        let combo_id = prefix_id.with("scaling_algorithm_choice");
        let esrgan_combo_id = prefix_id.with("esrgan_model_choice");
        let scale_algo = &mut p.scale.scale_algorithm;
        let esrgan_model = &mut p.scale.esrgan_model;

        ui.label("Scaling algorithm: ");
        egui::ComboBox::new(combo_id, "")
            .selected_text(scale_algo.to_string())
            .show_ui(ui, |ui| {
                choice(ui, scale_algo, ScaleAlgorithm::NearestNeighbour);
                choice(ui, scale_algo, ScaleAlgorithm::Bilinear);
                choice(ui, scale_algo, ScaleAlgorithm::Bicubic);
                choice(ui, scale_algo, ScaleAlgorithm::Xbrz);
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
    fn infill_fragment(&mut self, ui: &mut egui::Ui, p: &mut TransformImageParams) {
        ui.add_enabled_ui(p.infill.enabled, |ui| {
            egui::Grid::new(self.id().with("infill_options_ratio_grid"))
                .num_columns(2)
                .min_col_width(200.0)
                .show(ui, |ui| {
                    ui.label("Target aspect ratio: ");
                    ui.horizontal(|ui| {
                        let (mut a, mut b) = p.infill.target_aspect_ratio;
                        ui.add(
                            egui::DragValue::new(&mut a.0)
                                .fixed_decimals(0)
                                .clamp_range(1..=32),
                        );
                        ui.label(" : ");
                        ui.add(
                            egui::DragValue::new(&mut b.0)
                                .fixed_decimals(0)
                                .clamp_range(1..=32),
                        );

                        p.infill.target_aspect_ratio = (a, b);
                    });
                    ui.end_row();
                });

            ui.add_space(ui.style().spacing.item_spacing.y * 2.0);

            egui::Grid::new(self.id().with("infill_options_technique_grid"))
                .num_columns(2)
                .min_col_width(200.0)
                .show(ui, |ui| {
                    let tech = &mut p.infill.technique;

                    ui.label("Technique: ");
                    ui.horizontal(|ui| {
                        choice(ui, tech, InfillTechnique::Automatic);
                        choice(ui, tech, InfillTechnique::Blur);
                        choice(ui, tech, InfillTechnique::Solid);
                    });
                    ui.end_row();

                    let is_blur = p.infill.technique == InfillTechnique::Blur;
                    ui.add_enabled(is_blur, egui::Label::new("Choose infill colour: "));
                    ui.end_row();

                    indent(ui, |ui| {
                        ui.add_enabled_ui(is_blur, |ui| {
                            ui.radio_value(&mut p.infill.use_auto_solid, true, "Automatic");
                        });
                    });
                    ui.end_row();

                    indent(ui, |ui| {
                        ui.add_enabled_ui(is_blur, |ui| {
                            ui.radio_value(&mut p.infill.use_auto_solid, false, "Manual: ");
                        });
                    });

                    ui.add_enabled_ui(is_blur, |ui| {
                        let mut srgba = p.infill.manual_solid_colour.to_srgba_unmultiplied();
                        ui.color_edit_button_srgba_unmultiplied(&mut srgba);
                        p.infill.manual_solid_colour = egui::Color32::from_rgba_unmultiplied(
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
                        ui.checkbox(&mut p.infill.use_gaussian, "Gaussian blur: ");
                    });
                    ui.add_enabled(
                        p.infill.use_gaussian,
                        egui::Slider::new(&mut p.infill.gaussian_radius, 1..=64).suffix(" px"),
                    );
                    ui.end_row();

                    indent(ui, |ui| {
                        ui.checkbox(&mut p.infill.use_brightness, "Brightness change: ");
                    });
                    #[allow(clippy::cast_possible_truncation)]
                    ui.add_enabled(
                        p.infill.use_brightness,
                        egui::Slider::new(&mut p.infill.brightness_change.0, -1.0..=1.0)
                            .custom_formatter(|v, _| format!("{}%", (v * 100.0).round() as isize))
                            .fixed_decimals(2),
                    );
                    ui.end_row();

                    indent(ui, |ui| {
                        ui.checkbox(&mut p.infill.use_contrast, "Contrast change: ");
                    });
                    #[allow(clippy::cast_possible_truncation)]
                    ui.add_enabled(
                        p.infill.use_contrast,
                        egui::Slider::new(&mut p.infill.contrast_change.0, -1.0..=1.0)
                            .custom_formatter(|v, _| format!("{}%", (v * 100.0).round() as isize))
                            .fixed_decimals(2),
                    );
                    ui.end_row();
                });
        });
    }

    fn compression_fragment(&mut self, ui: &mut egui::Ui, p: &mut TransformImageParams) {
        ui.add_enabled_ui(p.compression.enabled, |ui| {
            egui::Grid::new(self.id().with("compression_options_ratio_grid"))
                .num_columns(2)
                .min_col_width(200.0)
                .show(ui, |ui| {
                    let file_type = &mut p.compression.file_type;

                    ui.label("File type: ");
                    ui.horizontal(|ui| {
                        choice(ui, file_type, CompressionFileType::Jpeg);
                        choice(ui, file_type, CompressionFileType::Png);
                        choice(ui, file_type, CompressionFileType::Webp);
                        choice(ui, file_type, CompressionFileType::Avif);
                    });
                    ui.end_row();

                    ui.label("Quality: ");
                    ui.add(egui::Slider::new(&mut p.compression.quality, 0..=100));
                    ui.end_row();

                    let chroma_id = self.id().with("chroma_select");
                    let chroma = &mut p.compression.chroma_subsampling;
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

    fn update_preview_image(&mut self) -> Option<()> {
        let id = self.id().with(request::LOAD_PREVIEW);
        let item_id = self.preview_grid.get_first_selected_id()?;
        let params = self.state().transform_params.clone();
        if self.selected_item_id.as_ref() == Some(&item_id)
            && self.params_of_selected_preview.as_ref() == Some(&params)
        {
            return None;
        }
        self.selected_item_id = Some(item_id);
        self.params_of_selected_preview = Some(params.clone());

        let vault = self.app_state.current_vault_opt()?;
        let item = vault.get_item_opt_by_id(item_id)?;
        let path = vault.resolve_abs_path(Path::new(item.path())).ok()?;

        self.selected_preview_hndl = None;
        self.app_state
            .add_task_request(id, "Load transformed preview", move |_, _| {
                Promise::spawn_blocking(move || load_transformed_image_preview(path, &params))
            });

        Some(())
    }

    fn top_preview_panel(&mut self, ui: &mut egui::Ui) {
        if let Ok(image) = self.handle_request(request::LOAD_PREVIEW, |res| match res {
            AsyncTaskResult::PreviewReady { image, .. } => Ok(image),
            _ => Err(res),
        }) {
            let hndl = ui.ctx().load_texture(
                "transform_preview",
                image,
                egui::TextureOptions {
                    wrap_mode: egui::TextureWrapMode::ClampToEdge,
                    magnification: egui::TextureFilter::Nearest,
                    minification: egui::TextureFilter::Linear,
                },
            );
            self.selected_preview_hndl = Some(hndl);
        }
        egui::SidePanel::right("preview_dest_panel")
            .max_width(200.0)
            .show_inside(ui, |ui| -> Option<()> {
                let Some(item_id) = self.selected_item_id else {
                    ui.label("Select an image to see a preview");
                    return None;
                };

                let vault = self.app_state.current_vault_opt()?;
                let item = vault.get_item_opt_by_id(item_id)?;
                let src_img_size = item.get_image_size().ok().flatten()?;
                let params = &self.state().transform_params;
                let dst_img_size = get_transformed_size(src_img_size, params);

                let width = ui.available_width();
                let img_rect =
                    egui::Rect::from_min_size(ui.min_rect().min, egui::vec2(width, 150.0));

                match self.selected_preview_hndl.as_ref() {
                    Some(hndl) => {
                        let img =
                            egui::Image::from_texture(egui::load::SizedTexture::from_handle(hndl))
                                .bg_fill(egui::Color32::from_gray(20))
                                .max_height(150.0)
                                .shrink_to_fit();

                        ui.put(img_rect, img);
                    }
                    None => {
                        ui.put(img_rect, egui::Spinner::new());
                    }
                }

                ui.heading(item.path());

                ui.label(format!(
                    "Original: {}\u{00d7}{}",
                    src_img_size.x, src_img_size.y
                ));

                ui.label(format!(
                    "Transformed: {}\u{00d7}{}",
                    dst_img_size.x, dst_img_size.y
                ));

                Some(())
            });

        egui::ScrollArea::horizontal()
            .auto_shrink([false, false])
            .show_viewport(ui, |ui, vp| {
                self.preview_grid.params.init_row_height = 256.0;
                self.preview_grid.params.container_width = 256.0;
                self.preview_grid.params.last_row_align = egui::Align::Min;
                self.preview_grid.params.main_axis = egui::Direction::TopDown;
                let params = self.state().transform_params.clone();
                self.preview_grid.transform_params = Some(params.clone());

                self.preview_grid.update(
                    ui,
                    vp,
                    self.app_state.clone(),
                    &self.source_item_ids,
                    self.source_items_updated,
                );

                if let Some(abs_path) = self.preview_grid.get_double_clicked_item_path() {
                    self.app_state.add_task("Load image preview", move |_, _| {
                        Promise::spawn_blocking(move || {
                            load_transformed_image_preview(abs_path, &params)
                        })
                    });
                }

                self.update_preview_image();

                Some(())
            });
    }

    fn modal_contents(&mut self, ui: &mut egui::Ui) {
        let padding_y = self.preview_grid.params.item_padding.y;

        egui::TopBottomPanel::top(self.id().with("preview_panel"))
            .exact_height(256.0 + 2.0 * padding_y)
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
                                FormSection::Scale => self.scaling_fragment(ui, &mut global_params),
                                FormSection::Infill => self.infill_fragment(ui, &mut global_params),
                                FormSection::Compression => {
                                    self.compression_fragment(ui, &mut global_params);
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
