use chrono::{DateTime, TimeDelta, Utc};
use eframe::egui;
use eframe::emath::{vec2, Align2};
use poll_promise::Promise;

use std::ops::Add;
use std::path::Path;
use std::sync::{Arc, OnceLock};
use tracing::info;
use uuid::Uuid;

use crate::data::Vault;
use crate::errors::AppError::NotImplemented;
use crate::fields;
use crate::state::{AppState, AppStateRef};
use crate::tasks::compute::compute_thumbnails_grid;
use crate::tasks::image::{load_image_thumbnail, load_image_thumbnail_with_fs, ThumbnailParams};
use crate::tasks::sort::{
    get_filtered_and_sorted_items, FilterExpression, SortDirection, SortExpression, SortType,
};
use crate::tasks::AsyncTaskResult::{ImportComplete, ThumbnailLoaded, VaultLoaded, VaultSaved};
use crate::tasks::{
    AsyncTaskResult, AsyncTaskReturn, ProgressSenderRef, ProgressState, TaskState,
    ThumbnailGridInfo, ThumbnailGridParams,
};
use crate::ui::item_cache::{ItemCache, ItemCacheParams};
use crate::ui::modals::message::MessageDialog;
use crate::ui::modals::new_vault::NewVaultDialog;
use crate::ui::stepwise_range::StepwiseRange;
use crate::ui::thumb_cache::{ThumbnailCache, ThumbnailCacheItem};

mod item_cache;
mod modals;
mod stepwise_range;
mod thumb_cache;

static THUMBNAIL_SLIDER_RANGE: OnceLock<StepwiseRange> = OnceLock::new();

const THUMBNAIL_CACHE_SIZE: u64 = 512 * 1024 * 1024; // 512 MiB
const THUMBNAIL_LOAD_INTERVAL_MS: i64 = 50;
const THUMBNAIL_LQ_LOAD_INTERVAL_MS: i64 = 10;
const THUMBNAIL_LOW_QUALITY_HEIGHT: usize = 128;
const THUMBNAIL_VISIBLE_THRESHOLD: f32 = 20.0;
const THUMBNAIL_SCROLL_COOLDOWN_INTERVAL_MS: i64 = 1500;

#[derive(Default)]
pub(crate) struct App {
    state: AppStateRef,
    tasks: TaskState,

    vault_loading: bool,
    thumbnail_params: ThumbnailGridParams,
    thumbnail_height_selection: f64,
    thumbnail_grid: ThumbnailGridInfo,
    thumbnail_grid_middle_item: Option<String>,
    thumbnail_grid_scroll_cooldown: Option<DateTime<Utc>>,
    thumbnail_grid_set_scroll: bool,
    thumbnail_grid_last_vp: Option<egui::Rect>,

    sorts: Vec<SortExpression>,
    filter: FilterExpression,

    item_list_cache: ItemCache,
    lq_thumbnail_cache: ThumbnailCache,
    thumbnail_cache: ThumbnailCache,

    msg_dialogs: Vec<MessageDialog>,
    new_vault_dialog: NewVaultDialog,

    sort_type: SortType,
    sort_field_id: Option<Uuid>,
    sort_direction: SortDirection,
    search_text: String,
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
            thumbnail_params: ThumbnailGridParams {
                max_row_height: 120.0,
                container_width: 0.0,
            },
            filter: FilterExpression::TagMatch(fields::image::NAMESPACE.id),
            sorts: vec![SortExpression::Path(SortDirection::Ascending)],
            thumbnail_cache: ThumbnailCache::new(
                THUMBNAIL_CACHE_SIZE,
                TimeDelta::milliseconds(THUMBNAIL_LOAD_INTERVAL_MS),
                false,
            ),
            lq_thumbnail_cache: ThumbnailCache::new(
                THUMBNAIL_CACHE_SIZE,
                TimeDelta::milliseconds(THUMBNAIL_LQ_LOAD_INTERVAL_MS),
                true,
            ),
            ..Default::default()
        }
    }

    fn add_task(
        &mut self,
        name: &'static str,
        task_factory: impl FnOnce(AppStateRef, ProgressSenderRef) -> Promise<AsyncTaskReturn>,
    ) {
        self.tasks
            .add_task_with_progress(name, |tx| task_factory(self.state.clone(), tx));
    }

    fn error(&mut self, message: String) {
        let dialog = MessageDialog::error(message);
        self.msg_dialogs.push(dialog);
    }

    fn success(&mut self, title: String, message: String) {
        let dialog = MessageDialog::success(message).with_title(title);
        self.msg_dialogs.push(dialog);
    }

    fn state(&self) -> tokio::sync::RwLockReadGuard<AppState> {
        self.state.blocking_read()
    }

    fn load_persistent_state(&mut self, storage: Option<&dyn eframe::Storage>) -> Option<()> {
        let stored_state: AppStorage =
            serde_json::from_str(&storage?.get_string(AppStorage::KEY)?).ok()?;

        if let Some(path) = stored_state.current_vault_file_path {
            self.vault_loading = true;
            self.add_task("Load vault", move |s, p| {
                Promise::spawn_async(crate::tasks::vault::load_vault_from_path(path, s, p))
            });
        }

        self.thumbnail_params.max_row_height = stored_state.thumbnail_row_height;
        self.filter = stored_state.filter;
        self.sorts = stored_state.sorts;
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
                .get_current_vault()?
                .file_path
                .as_ref()?
                .to_str()?
                .to_string(),
        )
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.msg_dialogs
            .retain_mut(|dialog| dialog.update(ctx).is_open());

        if let Some(new_vault_name) = self.new_vault_dialog.update(ctx).ready() {
            self.vault_loading = true;
            self.add_task("Create vault", move |s, p| {
                Promise::spawn_async(crate::tasks::vault::save_new_vault(
                    s,
                    Vault::new(new_vault_name),
                    p,
                ))
            });
        }

        for result in self.tasks.iter_ready() {
            match result {
                Ok(AsyncTaskResult::None) => {}
                Ok(VaultLoaded(name)) => {
                    self.vault_loading = false;
                    self.state.blocking_write().set_current_vault_name(name)
                }
                Ok(VaultSaved(_)) => self.vault_loading = false,
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
                    if params.height == THUMBNAIL_LOW_QUALITY_HEIGHT {
                        self.lq_thumbnail_cache
                            .commit(params.clone(), ThumbnailCacheItem::Loaded(hndl.clone()));
                    }
                    self.thumbnail_cache
                        .commit(params, ThumbnailCacheItem::Loaded(hndl));
                }
                Err(e) if NotImplemented.is_err(&e) => self.error("Not implemented".to_string()),
                Err(e) => self.error(format!("{e:#}")),
            }
            ctx.request_repaint();
        }

        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("Vault", |ui| {
                    if ui
                        .add_enabled(!self.vault_loading, egui::Button::new("New"))
                        .clicked()
                    {
                        info!("New vault clicked!");

                        self.new_vault_dialog.open();

                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(!self.vault_loading, egui::Button::new("Open"))
                        .clicked()
                    {
                        info!("Open vault clicked!");

                        self.vault_loading = true;
                        self.add_task("Load vault", |s, p| {
                            Promise::spawn_async(crate::tasks::vault::choose_and_load_vault(s, p))
                        });

                        ui.close_menu();
                    }

                    if self.state.blocking_read().get_current_vault().is_some()
                        && ui
                            .add_enabled(!self.vault_loading, egui::Button::new("Save"))
                            .clicked()
                    {
                        info!("Save vault clicked!");

                        self.vault_loading = true;
                        self.add_task("Save vault", |state, p| {
                            Promise::spawn_async(crate::tasks::vault::save_current_vault(state, p))
                        });

                        ui.close_menu();
                    }

                    ui.separator();

                    if ui.button("Quit").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });

                if self.state().get_current_vault().is_some() {
                    ui.menu_button("Import", |ui| {
                        if ui.button("Import all").clicked() {
                            info!("Import all clicked!");

                            self.add_task("Import to vault", |state, p| {
                                Promise::spawn_async(
                                    crate::tasks::import::import_images_recursively(state, p),
                                )
                            });

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
                        slider_range.lerp_in(self.thumbnail_params.max_row_height);

                    ui.add(
                        egui::widgets::Slider::new(&mut slider_value, slider_range.input_range())
                            .step_by(1.0)
                            .show_value(false),
                    );

                    self.thumbnail_params.max_row_height = slider_range.lerp_out(slider_value);

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
                        ui.label("Field goes here");
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

                    self.sorts = match self.sort_type {
                        SortType::Path => vec![SortExpression::Path(self.sort_direction)],
                        SortType::Field => vec![],
                    };

                    let mut layouter =
                        |ui: &egui::Ui, text: &str, _wrap_width: f32| -> Arc<egui::Galley> {
                            let mut job = egui::text::LayoutJob::default();
                            let style = ui.style();

                            job.append(
                                text,
                                16.0,
                                egui::TextFormat::simple(
                                    egui::TextStyle::Body.resolve(style),
                                    style.visuals.text_color(),
                                ),
                            );

                            ui.fonts(|f| f.layout_job(job))
                        };

                    let output = ui.add(
                        egui::TextEdit::singleline(&mut self.search_text)
                            .desired_width(f32::INFINITY)
                            .layouter(&mut layouter),
                    );

                    let style = ui.style();
                    let painter = ui.painter_at(output.rect);

                    let icon_width = painter
                        .text(
                            output.rect.min.add(vec2(
                                style.spacing.button_padding.x,
                                output.rect.size().y / 2.0,
                            )),
                            Align2::LEFT_CENTER,
                            "\u{1f50d}",
                            egui::TextStyle::Button.resolve(style),
                            style.visuals.strong_text_color(),
                        )
                        .width();

                    if self.search_text.is_empty() {
                        painter.text(
                            output.rect.min.add(vec2(
                                style.spacing.button_padding.x
                                    + icon_width
                                    + style.spacing.button_padding.x,
                                output.rect.size().y / 2.0,
                            )),
                            Align2::LEFT_CENTER,
                            "Search...",
                            egui::TextStyle::Body.resolve(style),
                            style.visuals.weak_text_color(),
                        );
                    }

                    ui.text_edit_singleline(&mut self.search_text);

                    self.filter = FilterExpression::TextSearch(self.search_text.clone());
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

                ctx.request_repaint();
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
            let state = self.state.blocking_read();
            let Some(current_vault) = state.get_current_vault() else {
                return;
            };

            let mut err = None;

            ui.label(format!("Current vault: {}", current_vault.name));

            let item_cache_params = ItemCacheParams {
                vault_name: current_vault.name.to_string(),
                filter: self.filter.clone(),
                sorts: self.sorts.clone(),
            };

            let new_item_list = self.item_list_cache.params != item_cache_params;
            if new_item_list {
                match get_filtered_and_sorted_items(&current_vault, &self.filter, &self.sorts) {
                    Ok(items) => {
                        self.item_list_cache = ItemCache::from_items(item_cache_params, &items);
                    }
                    Err(e) => err = Some(format!("{e:#}")),
                };
            }

            self.thumbnail_params.container_width = ui.available_width();

            let new_thumbnail_grid = self.thumbnail_grid.params != self.thumbnail_params;
            if new_item_list || new_thumbnail_grid {
                self.thumbnail_grid_set_scroll = true;
                ctx.request_repaint();
                let params = self.thumbnail_params.clone();

                match compute_thumbnails_grid(
                    params,
                    &self.item_list_cache.resolve_refs(&current_vault),
                ) {
                    Ok(info) => self.thumbnail_grid = info,
                    Err(e) => err = Some(format!("{e:#}")),
                };
            }

            drop(current_vault);
            drop(state);

            if let Some(e) = err {
                self.error(e);
                return;
            }

            egui::ScrollArea::vertical()
                .auto_shrink([false; 2])
                .show_viewport(ui, |ui, vp| {
                    let grid = &self.thumbnail_grid;
                    if grid.thumbnails.is_empty() {
                        return;
                    }

                    const PADDING: f32 = 8.0;
                    const ROUNDING: f32 = 4.0;

                    let abs_min = ui.min_rect().min.to_vec2();
                    let abs_vp = vp.translate(abs_min);
                    let vp_middle = (vp.min + vp.max.to_vec2()) / 2.0;
                    let vp_changed = self.thumbnail_grid_last_vp != Some(vp);
                    let vp_scrolled = vp_changed
                        && vp.size()
                            == self
                                .thumbnail_grid_last_vp
                                .map(|v| v.size())
                                .unwrap_or(vp.size());
                    let vp_resized = vp_changed && !vp_scrolled;
                    let max_y = grid.thumbnails.last().unwrap().bounds.max.y;
                    ui.set_width(ui.available_width());
                    ui.set_height(max_y);
                    ui.set_clip_rect(abs_vp);

                    let middle_item = self.thumbnail_grid_middle_item.as_ref();
                    let mut next_middle: Option<String> = None;

                    for item in grid.thumbnails.iter() {
                        let abs_bounds = item.bounds.translate(abs_min);
                        let text = egui::Label::new(item.path.clone());

                        // scroll to item if resize event has occurred
                        if (self.thumbnail_grid_set_scroll || vp_resized)
                            && Some(&item.path) == middle_item
                        {
                            info!("do scroll to {} at {:?}", &item.path, &abs_bounds);
                            ui.scroll_to_rect(abs_bounds, Some(egui::Align::Center));
                            self.thumbnail_grid_set_scroll = false;
                            self.thumbnail_grid_scroll_cooldown = Some(Utc::now().add(
                                TimeDelta::milliseconds(THUMBNAIL_SCROLL_COOLDOWN_INTERVAL_MS),
                            ));
                        }
                        // mark current item as item to scroll to when resize occurs
                        else if self.thumbnail_grid_scroll_cooldown.unwrap_or(Utc::now())
                            <= Utc::now()
                            && vp_scrolled
                            && next_middle.is_none()
                            && item.bounds.contains(vp_middle)
                        {
                            next_middle = Some(item.path.clone());
                        }

                        if vp.intersects(item.bounds) {
                            let path: Box<Path> = Path::new(item.path.as_str()).into();
                            let height = self.thumbnail_params.max_row_height as usize;

                            let mut thumb = ThumbnailCacheItem::Loading;
                            if height > THUMBNAIL_LOW_QUALITY_HEIGHT {
                                thumb = self.thumbnail_cache.read(&ThumbnailParams {
                                    path: path.clone(),
                                    last_modified: item.last_modified,
                                    height,
                                });
                            }
                            if thumb == ThumbnailCacheItem::Loading {
                                thumb = self.lq_thumbnail_cache.read(&ThumbnailParams {
                                    path: path.clone(),
                                    last_modified: item.last_modified,
                                    height: THUMBNAIL_LOW_QUALITY_HEIGHT,
                                });
                            }

                            match thumb {
                                ThumbnailCacheItem::Loading => {
                                    ui.put(abs_bounds.shrink(PADDING), text);
                                    ui.put(abs_bounds, egui::Spinner::new());
                                }
                                ThumbnailCacheItem::Loaded(hndl) => {
                                    egui::Image::new(egui::ImageSource::Texture(
                                        egui::load::SizedTexture::from_handle(&hndl),
                                    ))
                                    .rounding(egui::epaint::Rounding::from(ROUNDING))
                                    .shrink_to_fit()
                                    .paint_at(ui, abs_bounds.shrink(PADDING));
                                }
                            }
                        }
                    }

                    if next_middle.is_some() {
                        self.thumbnail_grid_middle_item = next_middle;
                    }

                    self.thumbnail_grid_last_vp = Some(vp);

                    for params in self.lq_thumbnail_cache.drain_requests() {
                        self.add_task("Load thumbnail", move |s, p| {
                            Promise::spawn_async(load_image_thumbnail_with_fs(s, p, params))
                        });
                    }

                    for params in self.thumbnail_cache.drain_requests() {
                        self.add_task("Load thumbnail", move |s, p| {
                            Promise::spawn_async(load_image_thumbnail(s, p, params))
                        });
                    }
                });
        });
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        let stored_state = AppStorage {
            current_vault_file_path: self.get_current_vault_file_path(),
            thumbnail_row_height: self.thumbnail_params.max_row_height,
            sorts: self.sorts.clone(),
            filter: self.filter.clone(),
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
