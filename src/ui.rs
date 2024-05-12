use std::sync::OnceLock;

use crate::data::FieldStore;
use eframe::egui;
use poll_promise::Promise;
use tracing::info;
use uuid::Uuid;

use crate::errors::AppError;

use crate::state::{AppState, AppStateRef};
use crate::tasks::filter::FilterExpression;
use crate::tasks::AsyncTaskResult::{ImportComplete, ThumbnailLoaded, VaultLoaded, VaultSaved};
use crate::tasks::{AsyncTaskResult, AsyncTaskReturn, ProgressSenderRef, ProgressState, TaskState};

use crate::tasks::sort::{SortDirection, SortExpression, SortType};
use crate::ui::item_cache::ItemCache;
use crate::ui::modals::edit_tag::EditTagDialog;
use crate::ui::modals::message::MessageDialog;
use crate::ui::modals::new_vault::NewVaultDialog;
use crate::ui::modals::AppModal;
use crate::ui::stepwise_range::StepwiseRange;
use crate::ui::thumb_cache::ThumbnailCacheItem;
use crate::ui::thumb_grid::{SelectMode, ThumbnailGrid};

mod item_cache;
mod modals;
mod stepwise_range;
mod theme;
mod thumb_cache;
mod thumb_grid;
mod widgets;

static THUMBNAIL_SLIDER_RANGE: OnceLock<StepwiseRange> = OnceLock::new();

const THUMBNAIL_LOW_QUALITY_HEIGHT: usize = 128;
const MAX_RUNNING_TASKS: usize = 16;

#[derive(Default)]
pub(crate) struct App {
    state: AppStateRef,
    tasks: TaskState,

    modal_dialogs: Vec<Box<dyn AppModal>>,

    item_list_cache: ItemCache,
    thumbnail_grid: ThumbnailGrid,

    sort_type: SortType,
    sort_field_id: Option<Uuid>,
    sort_direction: SortDirection,
    search_text: String,

    expand_right_panel: bool,
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct AppStorage {
    current_vault_file_path: Option<String>,
    thumbnail_row_height: f32,
    sorts: Vec<SortExpression>,
    filter: FilterExpression,
}

impl AppStorage {
    const KEY: &'static str = "AppStorage";
}

impl App {
    pub fn new() -> Self {
        Self {
            ..Default::default()
        }
    }

    fn add_task(
        &mut self,
        name: String,
        task_factory: impl FnOnce(AppStateRef, ProgressSenderRef) -> Promise<AsyncTaskReturn>
            + Send
            + Sync
            + 'static,
    ) {
        self.state.blocking_read().add_task(name, task_factory);
    }

    fn add_queued_tasks(&mut self) {
        let capacity = MAX_RUNNING_TASKS - self.tasks.running_tasks_count();
        for (name, task_factory) in self.state.blocking_write().drain_tasks(capacity) {
            self.tasks
                .add(name, |tx| task_factory(self.state.clone(), tx));
        }
    }

    fn add_modal_dialog(&mut self, dialog: impl AppModal + 'static) {
        self.modal_dialogs.push(Box::new(dialog));
    }

    fn error(&mut self, message: String) {
        self.add_modal_dialog(MessageDialog::error(message));
    }

    fn success(&mut self, title: String, message: String) {
        self.add_modal_dialog(MessageDialog::success(message).with_title(title));
    }

    fn state(&self) -> tokio::sync::RwLockReadGuard<AppState> {
        self.state.blocking_read()
    }

    fn load_persistent_state(&mut self, storage: Option<&dyn eframe::Storage>) -> Option<()> {
        let stored_state: AppStorage =
            serde_json::from_str(&storage?.get_string(AppStorage::KEY)?).ok()?;

        if let Some(path) = stored_state.current_vault_file_path {
            self.state.blocking_write().vault_loading = true;
            self.add_task("Load vault".into(), move |s, p| {
                Promise::spawn_async(crate::tasks::vault::load_vault_from_path(path, s, p))
            });
        }

        self.thumbnail_grid.params.max_row_height = stored_state.thumbnail_row_height;
        {
            let mut wr = self.state.blocking_write();
            wr.filter = stored_state.filter;
            wr.sorts = stored_state.sorts;
        }
        Some(())
    }

    fn setup(&mut self, ctx: &egui::Context, storage: Option<&dyn eframe::Storage>) {
        ctx.style_mut(|style| style.animation_time = 0.0);
        self.load_persistent_state(storage);
    }

    fn get_current_vault_file_path(&self) -> Option<String> {
        Some(
            self.state
                .blocking_read()
                .current_vault_opt()?
                .file_path
                .as_ref()?
                .to_str()?
                .to_string(),
        )
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.modal_dialogs
            .retain_mut(|dialog| dialog.update(ctx, self.state.clone()).is_open());

        self.add_queued_tasks();

        for result in self.tasks.iter_ready() {
            match result {
                Ok(AsyncTaskResult::None) => {}
                Ok(VaultLoaded(name)) => {
                    let mut wr = self.state.blocking_write();
                    wr.current_vault_name = Some(name);
                    wr.vault_loading = false;
                }
                Ok(VaultSaved(_)) => self.state.blocking_write().vault_loading = false,
                Ok(ImportComplete { path, results }) => {
                    let total = results.len();
                    let success = results.iter().filter(|r| r.is_ok()).count();
                    let body = format!(
                        "Import of {} complete. {success}/{total} images imported successfully.",
                        path.display()
                    );
                    self.success("Import complete".to_string(), body);
                }
                Ok(ThumbnailLoaded { params, image }) => {
                    let hndl = ctx.load_texture(params.tex_name(), image, Default::default());
                    self.thumbnail_grid
                        .commit(params, ThumbnailCacheItem::Loaded(hndl));
                }
                Err(e) if AppError::NotImplemented.is_err(&e) => {
                    self.error("Not implemented".to_string())
                }
                Err(e) => self.error(format!("{e:#}")),
            }
            ctx.request_repaint();
        }

        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("Vault", |ui| {
                    let vault_loading = self.state.blocking_read().vault_loading;
                    if ui
                        .add_enabled(!vault_loading, egui::Button::new("New"))
                        .clicked()
                    {
                        info!("New vault clicked!");

                        self.add_modal_dialog(NewVaultDialog::default());

                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(!vault_loading, egui::Button::new("Open"))
                        .clicked()
                    {
                        info!("Open vault clicked!");

                        self.state.blocking_write().vault_loading = true;
                        self.add_task("Load vault".into(), |s, p| {
                            Promise::spawn_async(crate::tasks::vault::choose_and_load_vault(s, p))
                        });

                        ui.close_menu();
                    }

                    if self.state().current_vault().is_ok()
                        && ui
                            .add_enabled(!vault_loading, egui::Button::new("Save"))
                            .clicked()
                    {
                        info!("Save vault clicked!");

                        self.state.blocking_write().vault_loading = true;
                        self.add_task("Save vault".into(), |state, p| {
                            Promise::spawn_async(crate::tasks::vault::save_current_vault(state, p))
                        });

                        ui.close_menu();
                    }

                    ui.separator();

                    if ui.button("Quit").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });

                if self.state().current_vault().is_ok() {
                    ui.menu_button("Import", |ui| {
                        if ui.button("Import all").clicked() {
                            info!("Import all clicked!");

                            self.add_task("Import to vault".into(), |state, p| {
                                Promise::spawn_async(
                                    crate::tasks::import::import_images_recursively(state, p),
                                )
                            });

                            ui.close_menu();
                        }
                    });

                    ui.menu_button("Tags", |ui| {
                        if ui.button("Edit tag").clicked() {
                            self.add_modal_dialog(EditTagDialog::select());
                            ui.close_menu();
                        }
                    });
                }

                ui.add_space(16.0);
            });
        });

        egui::TopBottomPanel::top("search_panel")
            .max_height(24.0)
            .show(ctx, |ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let slider_range = THUMBNAIL_SLIDER_RANGE.get_or_init(|| {
                        StepwiseRange::new(&[0.0, 1.0, 2.0, 3.0], &[128.0, 256.0, 512.0, 1024.0])
                    });
                    let mut slider_value =
                        slider_range.lerp_in(self.thumbnail_grid.params.max_row_height);

                    ui.add(
                        egui::widgets::Slider::new(&mut slider_value, slider_range.input_range())
                            .step_by(1.0)
                            .show_value(false),
                    );

                    self.thumbnail_grid.params.max_row_height = slider_range.lerp_out(slider_value);

                    // square four corners
                    ui.label("\u{26f6}");

                    ui.add_space(16.0);

                    if ui
                        .add(egui::Button::new(self.sort_direction.to_icon()).frame(false))
                        .clicked()
                    {
                        self.sort_direction = !self.sort_direction;
                    }

                    if self.sort_type == SortType::Field {
                        ui.add(
                            widgets::FindTag::new(
                                "sort_field",
                                &mut self.sort_field_id,
                                self.state.clone(),
                            )
                            .show_tag(true),
                        );
                    }

                    egui::ComboBox::from_label("Sort by")
                        .selected_text(self.sort_type.to_string())
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.sort_type,
                                SortType::Path,
                                SortType::Path.to_string(),
                            );
                            ui.selectable_value(
                                &mut self.sort_type,
                                SortType::Field,
                                SortType::Field.to_string(),
                            );

                            ui.style_mut().visuals.widgets.inactive.rounding.ne = 0.0;
                            ui.style_mut().visuals.widgets.inactive.rounding.se = 0.0;
                        });

                    ui.add(
                        widgets::SearchBox::new(&mut self.search_text).desired_width(f32::INFINITY),
                    );

                    {
                        let mut wr = self.state.blocking_write();
                        wr.sorts = match self.sort_type {
                            SortType::Path => vec![SortExpression::Path(self.sort_direction)],
                            SortType::Field => {
                                if let Some(field_id) = self.sort_field_id {
                                    vec![SortExpression::Field(field_id, self.sort_direction)]
                                } else {
                                    vec![]
                                }
                            }
                        };
                        wr.filter = FilterExpression::TextSearch(self.search_text.clone().into());
                    }
                });
            });

        egui::TopBottomPanel::bottom("bottom_panel").show(ctx, |ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(format!(
                    "{} {} ({}) compiled {}",
                    crate::built_info::PKG_NAME,
                    crate::built_info::PKG_VERSION,
                    crate::built_info::GIT_COMMIT_HASH_SHORT.unwrap_or("?"),
                    crate::built_info::built_time()
                ));

                let progresses = self.tasks.iter_progress();
                match &progresses[..] {
                    [] => {}
                    [(name, ProgressState::NotStarted), ..]
                    | [(name, ProgressState::Indeterminate), ..] => {
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

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::SidePanel::right("right_panel").show_animated_inside(
                ui,
                self.expand_right_panel,
                |ui| {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, true])
                        .show_viewport(ui, |ui, _vp| {
                            ui.horizontal(|ui| {
                                ui.label("Select: ");
                                ui.selectable_value(
                                    &mut self.thumbnail_grid.select_mode,
                                    SelectMode::Single,
                                    "Single",
                                );
                                ui.selectable_value(
                                    &mut self.thumbnail_grid.select_mode,
                                    SelectMode::Multiple,
                                    "Multiple",
                                );
                            });

                            let r = self.state.blocking_read();
                            let Some(vault) = r.current_vault_opt() else {
                                return;
                            };

                            let items = self.thumbnail_grid.view_selected_paths(|paths| {
                                self.item_list_cache.resolve_refs(&vault, paths)
                            });

                            if items.len() == 1 {
                                let item = items.first().unwrap();
                                ui.vertical(|ui| {
                                    ui.label(
                                        egui::RichText::new(item.path())
                                            .text_style(egui::TextStyle::Heading),
                                    );

                                    for def in item.iter_fields_with_defs(&vault) {
                                        ui.add(
                                            widgets::Tag::new(def.definition()).value(def.value()),
                                        );
                                    }
                                });
                            } else {
                                ui.label(format!(
                                    "{} item{}",
                                    items.len(),
                                    if items.len() == 1 { "" } else { "s" }
                                ));
                            }
                        });
                },
            );

            let scroll_area_rect = egui::CentralPanel::default()
                .show_inside(ui, |ui| {
                    let mut update =
                        || -> anyhow::Result<Option<egui::scroll_area::ScrollAreaOutput<()>>> {
                            let is_new_item_list =
                                self.item_list_cache.update(self.state.clone())?;
                            self.thumbnail_grid.update(
                                ui,
                                self.state.clone(),
                                &self.item_list_cache,
                                is_new_item_list,
                            )
                        };

                    let mut scroll_area_rect: Option<egui::Rect> = None;
                    match update() {
                        Ok(Some(egui::scroll_area::ScrollAreaOutput { inner_rect, .. })) => {
                            scroll_area_rect = Some(inner_rect);
                        }
                        Ok(_) => {}
                        Err(e) if AppError::NoCurrentVault.is_err(&e) => {}
                        Err(e) => self.error(format!("{e:?}")),
                    }
                    scroll_area_rect
                })
                .inner;

            const EXPAND_BTN_SIZE: egui::Vec2 = egui::vec2(16.0, 16.0);
            const EXPAND_BTN_ROUNDING: egui::Rounding = egui::Rounding {
                ne: 0.0,
                nw: 0.0,
                se: 0.0,
                sw: 4.0,
            };

            let btn_text = if self.expand_right_panel {
                // right pointing triangle
                "\u{25b6}"
            } else {
                // left pointing triangle
                "\u{25c0}"
            };
            let expand_btn = egui::Button::new(
                egui::RichText::new(btn_text).line_height(Some(EXPAND_BTN_SIZE.y)),
            )
            .rounding(EXPAND_BTN_ROUNDING)
            .min_size(EXPAND_BTN_SIZE);

            let btn_rect = egui::Align2::RIGHT_TOP
                .align_size_within_rect(EXPAND_BTN_SIZE, scroll_area_rect.unwrap_or(ui.min_rect()));

            if ui.put(btn_rect, expand_btn).clicked() {
                self.expand_right_panel ^= true;
            }
        });
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        let state = self.state.blocking_read();
        let stored_state = AppStorage {
            current_vault_file_path: self.get_current_vault_file_path(),
            thumbnail_row_height: self.thumbnail_grid.params.max_row_height,
            sorts: state.sorts.clone(),
            filter: state.filter.clone(),
        };

        storage.set_string(
            AppStorage::KEY,
            serde_json::to_string(&stored_state).expect("state to serialise properly"),
        );
    }
}

impl App {
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
                self.setup(&cc.egui_ctx, cc.storage);
                Box::new(self)
            }),
        )
    }
}
