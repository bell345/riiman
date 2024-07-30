use crate::data::{FilterExpression, ShortcutAction, ThumbnailCacheItem};
use eframe::egui;
use eframe::egui::{vec2, FontData, FontDefinitions, KeyboardShortcut};
use eframe::epaint::FontFamily;
use poll_promise::Promise;
use std::cmp::Reverse;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, OnceLock};
use tracing::info;
use uuid::Uuid;

use crate::errors::AppError;

use crate::state::{AppState, AppStateRef};
use crate::tasks::{AsyncTaskResult, AsyncTaskReturn, ProgressSenderRef, ProgressState, TaskState};

use crate::tasks::sort::{SortDirection, SortExpression, SortType};
use crate::tasks::transform::load_image_preview;
use crate::time;
use crate::ui::item_panel::ItemPanel;
use crate::ui::stepwise_range::StepwiseRange;
use crate::ui::thumb_grid::{SelectMode, ThumbnailGrid};

mod cloneable_state;
mod input;
mod item_panel;
mod modals;
mod stepwise_range;
mod theme;
mod thumb_grid;
pub mod widgets;

pub use crate::ui::modals::AppModal;

static THUMBNAIL_SLIDER_RANGE: OnceLock<StepwiseRange> = OnceLock::new();

const MAX_RUNNING_TASKS: usize = 16;

pub fn indent<R>(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.horizontal(|ui| {
        ui.add_space(ui.style().spacing.indent);
        ui.vertical(add_contents).inner
    })
    .inner
}

pub fn choice<T: PartialEq + std::fmt::Display>(
    ui: &mut egui::Ui,
    value_ref: &mut T,
    alternative: T,
) {
    let label = alternative.to_string();
    ui.selectable_value(value_ref, alternative, label);
}

/// Buttons for modal windows. Must be declared before all other components. Note that the order
/// of buttons in the UI is reversed.
pub fn buttons(id: egui::Id, ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui)) {
    egui::TopBottomPanel::bottom(id.with("bottom_panel")).show_inside(ui, |ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            add_contents(ui);
        });
    });
}

pub(crate) struct App {
    state: AppStateRef,
    tasks: TaskState,

    modal_dialogs: HashMap<egui::Id, Box<dyn AppModal>>,

    thumbnail_grid: ThumbnailGrid,

    sort_type: SortType,
    sort_field_id: Option<Uuid>,
    sort_direction: SortDirection,
    search_text: String,

    expand_right_panel: bool,
    focused: Option<egui::Id>,
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct AppStorage {
    vault_name_to_file_paths: HashMap<String, String>,
    current_vault_name: Option<String>,
    thumbnail_row_height: f32,
    sorts: Vec<SortExpression>,
    filter: FilterExpression,
    search_text: String,
    shortcuts: Vec<(KeyboardShortcut, ShortcutAction)>,
}

impl AppStorage {
    const KEY: &'static str = "AppStorage";
}

impl App {
    pub fn new() -> Self {
        Self {
            state: AppStateRef::new(AppState::default()),
            tasks: Default::default(),
            modal_dialogs: Default::default(),
            thumbnail_grid: ThumbnailGrid::new("main_thumbnail_grid"),
            sort_type: Default::default(),
            sort_field_id: None,
            sort_direction: Default::default(),
            search_text: String::new(),
            expand_right_panel: false,
            focused: None,
        }
    }

    fn add_task(
        &mut self,
        name: impl Into<String>,
        task_factory: impl FnOnce(AppStateRef, ProgressSenderRef) -> Promise<AsyncTaskReturn>
            + Send
            + Sync
            + 'static,
    ) {
        self.state.add_task(name, task_factory);
    }

    fn add_queued_tasks(&mut self) {
        let capacity = MAX_RUNNING_TASKS - self.tasks.running_tasks_count();
        for info in self.state.drain_tasks(capacity) {
            let s = self.state.clone();
            let factory = info.task_factory;
            match info.request_id {
                Some(id) => self.tasks.add_request(id, info.name, |tx| factory(s, tx)),
                None => self.tasks.add(info.name, |tx| factory(s, tx)),
            }
        }
    }

    fn add_modal_dialog(&mut self, dialog: impl AppModal + 'static) {
        let b = Box::new(dialog);
        self.modal_dialogs.insert(b.id(), b);
    }

    fn error(&mut self, message: String) {
        self.add_modal_dialog(modals::Message::error(message));
    }

    fn success(&mut self, title: String, message: String) {
        self.add_modal_dialog(modals::Message::success(message).with_title(title));
    }

    fn load_persistent_state(&mut self, storage: Option<&dyn eframe::Storage>) -> Option<()> {
        let stored_state: AppStorage =
            serde_json::from_str(&storage?.get_string(AppStorage::KEY)?).ok()?;

        let mut vault_name_to_file_paths: Vec<(String, String)> =
            stored_state.vault_name_to_file_paths.into_iter().collect();
        vault_name_to_file_paths.sort_by_key(|(name, _)| {
            Reverse(stored_state.current_vault_name.as_ref() == Some(name))
        });
        for (name, path) in vault_name_to_file_paths {
            let set_as_current = stored_state.current_vault_name.as_ref() == Some(&name);
            if set_as_current {
                self.state.set_vault_loading();
            }

            self.add_task(format!("Load vault {name}"), move |s, p| {
                Promise::spawn_async(crate::tasks::vault::load_vault_from_path(
                    path,
                    s,
                    p,
                    set_as_current,
                ))
            });
        }

        self.search_text = stored_state.search_text;

        self.thumbnail_grid.params.init_row_height = stored_state.thumbnail_row_height;

        self.state
            .set_filter_and_sorts(stored_state.filter, stored_state.sorts);
        self.state.set_shortcuts(stored_state.shortcuts);

        Some(())
    }

    fn setup(&mut self, ctx: &egui::Context, storage: Option<&dyn eframe::Storage>) {
        ctx.style_mut(|style| style.animation_time = 0.0);
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
        self.load_persistent_state(storage);
    }

    fn process_tasks(&mut self, ctx: &egui::Context) {
        self.add_queued_tasks();

        let (results, request_results) = self.tasks.iter_ready();
        for result in results {
            match result {
                Ok(
                    AsyncTaskResult::None
                    | AsyncTaskResult::FoundGalleryDl { .. }
                    | AsyncTaskResult::SelectedDirectory(_)
                    | AsyncTaskResult::SelectedFile(_),
                ) => {}
                Ok(AsyncTaskResult::VaultLoaded {
                    name,
                    set_as_current,
                }) if set_as_current => {
                    self.state.reset_vault_loading();
                    if self.state.set_current_vault_name(name.clone()).is_err() {
                        self.error(format!(
                            "Failed to set current vault with name '{name}' \
                             as it could not be found"
                        ));
                    }
                }
                Ok(AsyncTaskResult::VaultLoaded { .. } | AsyncTaskResult::VaultSaved(_)) => {
                    self.state.reset_vault_loading();
                }
                Ok(AsyncTaskResult::ImportComplete { path, results }) => {
                    let total = results.len();
                    let success = results.iter().filter(|r| r.is_ok()).count();
                    let body = format!(
                        "Import of {} complete. {success}/{total} images imported successfully.",
                        path.display()
                    );
                    // update thumbnail grid
                    self.thumbnail_grid.params.container_width = 0.0;
                    self.success("Import complete".to_string(), body);
                }
                Ok(AsyncTaskResult::LinkComplete {
                    other_vault_name,
                    results,
                }) => {
                    self.state.save_current_vault();
                    self.state.save_vault_by_name(&other_vault_name);

                    let total = results.len();
                    let success = results.iter().filter(|r| r.is_ok()).count();
                    let body = format!(
                        "Link to vault {other_vault_name} complete. \
                        {success}/{total} images linked successfully.",
                    );
                    self.success("Link complete".to_string(), body);
                }
                Ok(AsyncTaskResult::ThumbnailLoaded { params, image }) => {
                    let hndl =
                        ctx.load_texture(params.tex_name(), image, egui::TextureOptions::default());
                    self.state
                        .commit_thumbnail(params, ThumbnailCacheItem::Loaded(hndl));
                }
                Ok(AsyncTaskResult::PreviewReady {
                    id,
                    image,
                    viewport_class,
                }) => {
                    let hndl = ctx.load_texture(
                        "preview",
                        image,
                        egui::TextureOptions {
                            wrap_mode: egui::TextureWrapMode::ClampToEdge,
                            magnification: egui::TextureFilter::Nearest,
                            minification: egui::TextureFilter::Linear,
                        },
                    );
                    self.add_modal_dialog(modals::Preview::new(id, hndl, *viewport_class));
                }
                Err(e) if AppError::NotImplemented.is_err(&e) => {
                    self.error("Not implemented".to_string());
                }
                Err(e) => self.error(format!("{e:#}")),
            }
            ctx.request_repaint();
        }

        self.state.push_request_results(request_results);
    }

    fn vault_menu_ui(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        let vault_text = if self.state.has_unresolved_vaults() {
            egui::RichText::new("Vault \u{ff01}").color(theme::ERROR_TEXT)
        } else {
            egui::RichText::new("Vault")
        };

        ui.menu_button(vault_text, |ui| {
            let vault_loading = self.state.vault_loading();
            if ui
                .add_enabled(!vault_loading, egui::Button::new("New..."))
                .clicked()
            {
                info!("New vault clicked!");

                self.add_modal_dialog(modals::NewVault::default());

                ui.close_menu();
            }
            if ui
                .add_enabled(!vault_loading, egui::Button::new("Open..."))
                .clicked()
            {
                info!("Open vault clicked!");

                self.state.set_vault_loading();
                self.add_task("Load vault", |s, p| {
                    Promise::spawn_async(crate::tasks::vault::choose_and_load_vault(s, p, true))
                });

                ui.close_menu();
            }

            if self.state.current_vault().is_ok()
                && ui
                    .add_enabled(!vault_loading, egui::Button::new("Save"))
                    .clicked()
            {
                info!("Save vault clicked!");

                self.state.save_current_vault();

                ui.close_menu();
            }

            let manage_text = if self.state.has_unresolved_vaults() {
                egui::RichText::new("Manage... \u{ff01}").color(theme::ERROR_TEXT)
            } else {
                egui::RichText::new("Manage...")
            };

            if !self.state.known_vault_names().is_empty()
                && ui
                    .add_enabled(!vault_loading, egui::Button::new(manage_text))
                    .clicked()
            {
                self.add_modal_dialog(modals::ManageVaults::default());

                ui.close_menu();
            }

            ui.separator();

            if ui.button("Quit").clicked() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
    }

    fn import_menu_ui(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Import", |ui| -> Result<(), ()> {
            if ui.button("Import...").clicked() {
                let vault = self.state.current_vault_catch()?;
                self.add_task("Import one", |_, p| {
                    Promise::spawn_async(crate::tasks::import::select_and_import_one(vault, p))
                });

                ui.close_menu();
            }

            if ui.button("Import all files").clicked() {
                info!("Import all clicked!");

                let vault = self.state.current_vault_catch()?;
                self.add_task("Import to vault", |_, p| {
                    Promise::spawn_async(crate::tasks::import::import_images_recursively(vault, p))
                });

                ui.close_menu();
            }

            if ui.button("Download...").clicked() {
                self.add_modal_dialog(modals::Download::default());

                ui.close_menu();
            }

            Ok(())
        });
    }

    fn tag_menu_ui(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Tags", |ui| {
            if ui.button("New...").clicked() {
                self.add_modal_dialog(modals::EditTag::create());
                ui.close_menu();
            }
            if ui.button("Edit...").clicked() {
                self.add_modal_dialog(modals::EditTag::select());
                ui.close_menu();
            }
            if ui.button("Shortcuts...").clicked() {
                self.add_modal_dialog(modals::TagShortcuts::default());
                ui.close_menu();
            }
        });
    }

    fn link_menu_ui(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Link", |ui| {
            if ui.button("Other Vault...").clicked() {
                self.add_modal_dialog(modals::LinkVault::default());
                ui.close_menu();
            }
            if ui.button("Sidecars").clicked() {
                self.add_task("Link sidecars", |state, p| {
                    Promise::spawn_async(crate::tasks::link::link_sidecars(state, p))
                });
                ui.close_menu();
            }
        });
    }

    fn transform_menu_ui(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Transform", |ui| {
            if ui.button("Images...").clicked() {
                self.add_modal_dialog(modals::TransformImages::default());
                ui.close_menu();
            }
            if ui.button("Paths...").clicked() {
                ui.close_menu();
            }
            if ui.button("Tasks...").clicked() {
                ui.close_menu();
            }
        });
    }

    fn top_panel_ui(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                self.vault_menu_ui(ctx, ui);

                if self.state.current_vault().is_ok() {
                    self.import_menu_ui(ui);

                    self.tag_menu_ui(ui);

                    self.link_menu_ui(ui);

                    self.transform_menu_ui(ui);
                }

                ui.add_space(16.0);
            });
        });
    }

    fn search_panel_ui(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("search_panel")
            .max_height(24.0)
            .show(ctx, |ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // contents are declared from right to left due to layout

                    let slider_range = THUMBNAIL_SLIDER_RANGE.get_or_init(|| {
                        StepwiseRange::new(
                            &[0.0, 1.0, 2.0, 3.0, 4.0],
                            &[128.0, 256.0, 512.0, 1024.0, 2048.0],
                        )
                    });
                    let mut slider_value =
                        slider_range.lerp_in(self.thumbnail_grid.params.init_row_height);

                    ui.add(
                        egui::widgets::Slider::new(&mut slider_value, slider_range.input_range())
                            .step_by(1.0)
                            .show_value(false),
                    );

                    self.thumbnail_grid.params.init_row_height =
                        slider_range.lerp_out(slider_value);

                    // square four corners
                    ui.label("\u{26f6}");

                    ui.add_space(16.0);

                    if ui
                        .add(egui::Button::new(self.sort_direction.to_icon()).frame(false))
                        .clicked()
                    {
                        self.sort_direction = !self.sort_direction;
                    }

                    let sort_type = &mut self.sort_type;
                    if *sort_type == SortType::Field {
                        if let Some(vault) = self.state.current_vault_opt() {
                            ui.add(
                                widgets::FindTag::new("sort_field", &mut self.sort_field_id, vault)
                                    .show_tag(true),
                            );
                        };
                    }

                    egui::ComboBox::from_label("Sort by")
                        .selected_text(sort_type.to_string())
                        .show_ui(ui, |ui| {
                            choice(ui, sort_type, SortType::Path);
                            choice(ui, sort_type, SortType::Field);

                            ui.style_mut().visuals.widgets.inactive.rounding.ne = 0.0;
                            ui.style_mut().visuals.widgets.inactive.rounding.se = 0.0;
                        });

                    let Ok(vault) = self.state.current_vault() else {
                        return;
                    };

                    let search_res =
                        widgets::SearchBox::new("main_search_box", &mut self.search_text, vault)
                            .desired_width(f32::INFINITY)
                            .interactive()
                            .show(ui);

                    let sorts = match self.sort_type {
                        SortType::Path => vec![SortExpression::Path(self.sort_direction)],
                        SortType::Field => {
                            if let Some(field_id) = self.sort_field_id {
                                vec![SortExpression::Field(field_id, self.sort_direction)]
                            } else {
                                vec![]
                            }
                        }
                    };

                    let filter = search_res
                        .expression
                        .map_or(FilterExpression::None, |expr| expr.expr);

                    self.state.set_filter_and_sorts(filter, sorts);
                });
            });
    }

    fn bottom_panel_ui(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("bottom_panel").show(ctx, |ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(format!(
                    "{} {} ({}) compiled {}",
                    crate::built_info::PKG_NAME,
                    crate::built_info::PKG_VERSION,
                    crate::built_info::GIT_COMMIT_HASH_SHORT.unwrap_or("?"),
                    crate::built_info::built_time()
                ));

                #[allow(clippy::cast_possible_truncation)]
                #[allow(clippy::cast_sign_loss)]
                match &self.tasks.iter_progress()[..] {
                    [] => {}
                    [(name, ProgressState::NotStarted | ProgressState::Indeterminate), ..] => {
                        ui.add(egui::ProgressBar::new(0.0).text(name).animate(true));
                    }
                    [(name, ProgressState::Determinate(progress)), ..] => {
                        ui.add(
                            egui::ProgressBar::new(*progress)
                                .text(format!("{}% {name}", (progress * 100.0).floor() as u32)),
                        );
                    }
                    [(name, ProgressState::DeterminateWithMessage(progress, msg)), ..] => {
                        ui.add(egui::ProgressBar::new(*progress).text(format!(
                            "{}% {name}: {msg}",
                            (progress * 100.0).floor() as u32
                        )));
                    }
                    [(name, ProgressState::Completed), ..] => {
                        ui.add(egui::ProgressBar::new(1.0).text(name));
                    }
                };
            });
        });
    }

    fn right_panel_ui(&mut self, ui: &mut egui::Ui) {
        egui::SidePanel::right("right_panel").show_animated_inside(
            ui,
            self.expand_right_panel,
            |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, true])
                    .max_width(350.0)
                    .show_viewport(ui, |ui, _vp| -> Option<()> {
                        let len = self.state.len_item_list();
                        ui.label(format!("{} item{}", len, if len == 1 { "" } else { "s" }));
                        ui.horizontal(|ui| {
                            ui.label("Select: ");

                            let mut select_mode = self.thumbnail_grid.select_mode(ui.ctx());

                            choice(ui, &mut select_mode, SelectMode::Single);
                            choice(ui, &mut select_mode, SelectMode::Multiple);

                            self.thumbnail_grid.set_select_mode(ui.ctx(), select_mode);
                        });

                        let vault = self.state.current_vault_opt()?;
                        let items = self.thumbnail_grid.get_selected_items(&vault);

                        ui.add(ItemPanel::new(
                            "item_panel",
                            &items,
                            Arc::clone(&vault),
                            self.state.clone(),
                        ));

                        Some(())
                    });
            },
        );
    }

    fn get_double_clicked_item_path(&self) -> Option<String> {
        let vault = self.state.current_vault_opt()?;
        let item = self.thumbnail_grid.get_double_clicked_item(&vault)?;
        let rel_path = Path::new(item.path());
        self.state
            .catch(
                || format!("resolving absolute path for {}", item.path()),
                || vault.resolve_abs_path(rel_path),
            )
            .ok()
    }

    fn central_panel_ui(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            let scroll_area_rect = egui::CentralPanel::default()
                .show_inside(ui, |ui| {
                    time!("Right panel UI", {
                        self.right_panel_ui(ui);
                    });

                    time!("Item list update", { self.state.update_item_list().ok() });

                    egui::ScrollArea::vertical()
                        .auto_shrink([false; 2])
                        .animated(false)
                        .show_viewport(ui, |ui, vp_rect| {
                            self.thumbnail_grid.params.container_width =
                                ui.available_width().floor();

                            time!("Thumbnail grid update", {
                                self.thumbnail_grid.update(
                                    ui,
                                    vp_rect,
                                    self.state.clone(),
                                    &self.state.item_list_ids(),
                                    self.state.item_list_is_new(),
                                );
                            });

                            self.state
                                .update_selection(self.thumbnail_grid.get_selected_ids());
                        })
                })
                .inner;

            if let Some(abs_path) = self.thumbnail_grid.get_double_clicked_item_path() {
                self.add_task("Load image preview", move |_, _| {
                    Promise::spawn_blocking(move || load_image_preview(abs_path))
                });
            }

            time!("Expand button UI", {
                const EXPAND_BTN_SIZE: egui::Vec2 = vec2(32.0, 32.0);
                const EXPAND_BTN_ROUNDING: egui::Rounding = egui::Rounding {
                    nw: 0.0,
                    ne: 0.0,
                    sw: 8.0,
                    se: 8.0,
                };
                const EXPAND_BTN_MARGIN: egui::Vec2 = vec2(16.0, 0.0);

                let btn_text = if self.expand_right_panel {
                    // right pointing triangle
                    "\u{25b6}"
                } else {
                    // left pointing triangle
                    "\u{25c0}"
                };
                let expand_btn = egui::Button::new(
                    egui::RichText::new(btn_text).text_style(egui::TextStyle::Heading),
                )
                .rounding(EXPAND_BTN_ROUNDING)
                .min_size(EXPAND_BTN_SIZE);

                let btn_rect = egui::Align2::RIGHT_TOP.align_size_within_rect(
                    EXPAND_BTN_SIZE,
                    scroll_area_rect.inner_rect.shrink2(EXPAND_BTN_MARGIN),
                );

                if ui.put(btn_rect, expand_btn).clicked() {
                    self.expand_right_panel ^= true;
                }
            });
        });
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(f) = self.focused.take() {
            ctx.memory_mut(|m| m.request_focus(f));
        }

        let errors = self.state.drain_errors();
        for error in errors {
            self.error(format!("{error}"));
        }

        for new_dialog in self.state.drain_dialogs() {
            if let Some(mut old_dialog) = self.modal_dialogs.remove(&new_dialog.id()) {
                old_dialog.dispose(ctx, self.state.clone());
            }
            self.modal_dialogs.insert(new_dialog.id(), new_dialog);
        }

        self.modal_dialogs
            .retain(|_, dialog| dialog.update_or_dispose(ctx, self.state.clone()));

        self.process_tasks(ctx);

        self.top_panel_ui(ctx);

        self.search_panel_ui(ctx);

        self.bottom_panel_ui(ctx);

        self.central_panel_ui(ctx);

        self.focused = ctx.memory(|m| m.focused());

        let selected_ids = self.thumbnail_grid.get_selected_ids();
        if let &[selected_id] = selected_ids.as_slice() {
            self.focused
                .get_or_insert(selected_id.to_egui_id(self.thumbnail_grid.id()));
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        let stored_state = AppStorage {
            current_vault_name: self.state.current_vault_name().map(|s| s.to_string()),
            vault_name_to_file_paths: self.state.vault_name_to_file_paths(),
            thumbnail_row_height: self.thumbnail_grid.params.init_row_height,
            sorts: self.state.sorts().clone(),
            filter: self.state.filter().clone(),
            search_text: self.search_text.clone(),
            shortcuts: self.state.shortcuts(),
        };

        storage.set_string(
            AppStorage::KEY,
            serde_json::to_string(&stored_state).expect("state to serialise properly"),
        );
    }
}

mod font_names {
    pub const M_PLUS_ROUNDED: &str = "MPLUSRounded1c-Regular";
    pub const JETBRAINS_MONO: &str = "JetBrainsMono-Regular";
    pub const INTER: &str = "Inter-Regular";
    pub const NOTO_SANS: &str = "NotoSans-Regular";
    pub const NOTO_SANS_SC: &str = "NotoSansSC-Regular";
    pub const NOTO_SANS_TC: &str = "NotoSansTC-Regular";
    pub const NOTO_SANS_KR: &str = "NotoSansKR-Regular";
    pub const NOTO_SANS_SYMBOLS: &str = "NotoSansSymbols-Regular";
    pub const NOTO_SANS_SYMBOLS_2: &str = "NotoSansSymbols2-Regular";
    pub const NOTO_SANS_MATH: &str = "NotoSansMath-Regular";
    pub const NOTO_EMOJI: &str = "NotoEmoji-Regular";
    pub const DUMMY_TAG_REPLACEMENT: &str = "DummyTagReplacement-Regular";
}

pub const DUMMY_TAG_REPLACEMENT_FAMILY: &str = "DummyTagReplacement";

impl App {
    fn init_fonts() -> FontDefinitions {
        let mut fonts = FontDefinitions::default();

        fonts.font_data.insert(
            font_names::M_PLUS_ROUNDED.to_owned(),
            FontData::from_static(include_bytes!(
                "../res/font/MPLUSRounded1c/MPLUSRounded1c-Regular.ttf"
            )),
        );
        fonts.font_data.insert(
            font_names::JETBRAINS_MONO.to_owned(),
            FontData::from_static(include_bytes!(
                "../res/font/JetBrainsMono/JetBrainsMono-Regular.ttf"
            )),
        );
        fonts.font_data.insert(
            font_names::INTER.to_owned(),
            FontData::from_static(include_bytes!("../res/font/Inter/Inter-Regular.ttf")),
        );
        fonts.font_data.insert(
            font_names::NOTO_SANS.to_owned(),
            FontData::from_static(include_bytes!("../res/font/NotoSans/NotoSans-Regular.ttf")),
        );
        fonts.font_data.insert(
            font_names::NOTO_SANS_SC.to_owned(),
            FontData::from_static(include_bytes!(
                "../res/font/NotoSansSC/NotoSansSC-Regular.ttf"
            )),
        );
        fonts.font_data.insert(
            font_names::NOTO_SANS_TC.to_owned(),
            FontData::from_static(include_bytes!(
                "../res/font/NotoSansTC/NotoSansTC-Regular.ttf"
            )),
        );
        fonts.font_data.insert(
            font_names::NOTO_SANS_KR.to_owned(),
            FontData::from_static(include_bytes!(
                "../res/font/NotoSansKR/NotoSansKR-Regular.ttf"
            )),
        );
        fonts.font_data.insert(
            font_names::NOTO_SANS_SYMBOLS.to_owned(),
            FontData::from_static(include_bytes!(
                "../res/font/NotoSansSymbols/NotoSansSymbols-Regular.ttf"
            )),
        );
        fonts.font_data.insert(
            font_names::NOTO_SANS_SYMBOLS_2.to_owned(),
            FontData::from_static(include_bytes!(
                "../res/font/NotoSansSymbols2/NotoSansSymbols2-Regular.ttf"
            )),
        );
        fonts.font_data.insert(
            font_names::NOTO_SANS_MATH.to_owned(),
            FontData::from_static(include_bytes!(
                "../res/font/NotoSansMath/NotoSansMath-Regular.ttf"
            )),
        );
        fonts.font_data.insert(
            font_names::NOTO_EMOJI.to_owned(),
            FontData::from_static(include_bytes!(
                "../res/font/NotoEmoji/NotoEmoji-Regular.ttf"
            )),
        );

        fonts.font_data.insert(
            font_names::DUMMY_TAG_REPLACEMENT.to_owned(),
            FontData::from_static(include_bytes!("../res/font/DummyTagReplacement.ttf")),
        );

        fonts.families.insert(
            FontFamily::Name(DUMMY_TAG_REPLACEMENT_FAMILY.into()),
            vec![font_names::DUMMY_TAG_REPLACEMENT.to_owned()],
        );

        {
            let prop = fonts.families.get_mut(&FontFamily::Proportional).unwrap();
            // pushing to front (so highest priority is at the end here)
            prop.insert(0, font_names::NOTO_SANS_KR.to_owned());
            prop.insert(0, font_names::NOTO_SANS_TC.to_owned());
            prop.insert(0, font_names::NOTO_SANS_SC.to_owned());
            prop.insert(0, font_names::M_PLUS_ROUNDED.to_owned());
            prop.insert(0, font_names::INTER.to_owned());

            // fallback symbols
            prop.push(font_names::NOTO_SANS_SYMBOLS.to_owned());
            prop.push(font_names::NOTO_SANS_SYMBOLS_2.to_owned());
            prop.push(font_names::NOTO_SANS_MATH.to_owned());
            prop.push(font_names::NOTO_EMOJI.to_owned());
        }

        {
            let mono = fonts.families.get_mut(&FontFamily::Monospace).unwrap();
            mono.insert(0, font_names::JETBRAINS_MONO.to_owned());
        }

        fonts
    }

    pub(crate) fn run(mut self) -> Result<(), eframe::Error> {
        let options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default().with_inner_size([600.0, 800.0]),
            ..Default::default()
        };

        eframe::run_native(
            "riiman",
            options,
            Box::new(|cc| {
                egui_extras::install_image_loaders(&cc.egui_ctx);

                cc.egui_ctx.set_fonts(Self::init_fonts());

                self.setup(&cc.egui_ctx, cc.storage);
                Box::new(self)
            }),
        )
    }
}
