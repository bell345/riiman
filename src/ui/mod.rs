use chrono::{DateTime, TimeDelta, Utc};
use eframe::egui;
use poll_promise::Promise;
use std::cell::RefCell;
use std::ops::{Add, Deref};
use std::path::Path;
use std::sync::Mutex;
use tracing::{info, warn};

use crate::data::{Item, Vault};
use crate::fields;
use crate::state::{AppState, AppStateRef};
use crate::tasks::compute::compute_thumbnails_grid;
use crate::tasks::image::{load_image_thumbnail, load_image_thumbnail_with_fs, ThumbnailParams};
use crate::tasks::sort::{
    get_filtered_and_sorted_items, FilterExpression, SortDirection, SortExpression,
};
use crate::tasks::AsyncTaskResult::{ImportComplete, ThumbnailLoaded, VaultLoaded, VaultSaved};
use crate::tasks::{
    AsyncTaskReturn, ProgressSenderRef, ProgressState, TaskError, TaskState, ThumbnailGridInfo,
    ThumbnailGridParams,
};
use crate::ui::modals::message::MessageDialog;
use crate::ui::modals::new_vault::NewVaultDialog;

mod modals;

#[derive(Debug, PartialEq, Eq)]
struct ItemCacheParams {
    vault_name: String,
    sorts: Vec<SortExpression>,
    filter: FilterExpression,
}

struct ItemCache {
    item_paths: Vec<String>,
    params: ItemCacheParams,
}

impl ItemCache {
    pub fn resolve_refs<'a, 'b: 'a>(
        &'a self,
        vault: &'b Vault,
    ) -> Vec<impl Deref<Target = Item> + 'a> {
        self.item_paths
            .iter()
            .filter_map(|p| vault.get_item(Path::new(p)).expect("valid path"))
            .collect()
    }
}

const THUMBNAIL_CACHE_SIZE: u64 = 512 * 1024 * 1024; // 512 MiB
const THUMBNAIL_LOAD_INTERVAL_MS: i64 = 50;
const THUMBNAIL_LQ_LOAD_INTERVAL_MS: i64 = 10;
const THUMBNAIL_LOW_QUALITY_HEIGHT: usize = 128;
const THUMBNAIL_VISIBLE_THRESHOLD: f32 = 20.0;
const THUMBNAIL_SCROLL_COOLDOWN_INTERVAL_MS: i64 = 1500;

#[derive(Clone, PartialEq, Eq)]
enum ThumbnailCacheItem {
    Loading,
    Loaded(egui::TextureHandle),
}

struct ThumbnailCache {
    cache: moka::sync::Cache<ThumbnailParams, ThumbnailCacheItem>,
    pending_inserts: Mutex<Vec<ThumbnailParams>>,

    is_loading: RefCell<bool>,
    next_load_utc: RefCell<Option<DateTime<Utc>>>,

    load_interval: TimeDelta,
    is_concurrent: bool,
}

impl ThumbnailCache {
    pub fn new(load_interval: TimeDelta, is_concurrent: bool) -> Self {
        Self {
            cache: moka::sync::CacheBuilder::new(THUMBNAIL_CACHE_SIZE)
                .weigher(|_, v| match v {
                    ThumbnailCacheItem::Loading => 0,
                    ThumbnailCacheItem::Loaded(hndl) => {
                        hndl.byte_size().try_into().unwrap_or(u32::MAX)
                    }
                })
                .build(),
            pending_inserts: Default::default(),
            is_loading: Default::default(),
            next_load_utc: Default::default(),
            load_interval,
            is_concurrent,
        }
    }

    pub fn read(&self, params: &ThumbnailParams) -> ThumbnailCacheItem {
        self.cache.get_with(params.clone(), || {
            self.pending_inserts.lock().unwrap().push(params.clone());
            ThumbnailCacheItem::Loading
        })
    }

    pub fn commit(&self, params: ThumbnailParams, item: ThumbnailCacheItem) {
        self.cache.insert(params, item);
        *self.is_loading.borrow_mut() = false;
    }

    pub fn drain_requests(&mut self) -> Vec<ThumbnailParams> {
        let mut requests = vec![];
        for params in self.pending_inserts.lock().unwrap().drain(..) {
            let conc_blocked = !self.is_concurrent && *self.is_loading.borrow();
            let time_blocked = self.next_load_utc.borrow().unwrap_or(Utc::now()) > Utc::now();

            if conc_blocked || time_blocked {
                self.cache.invalidate(&params);
                continue;
            }

            *self.is_loading.borrow_mut() = true;
            *self.next_load_utc.borrow_mut() = Some(Utc::now().add(self.load_interval));

            requests.push(params);
        }

        requests
    }
}

impl Default for ThumbnailCache {
    fn default() -> Self {
        Self::new(TimeDelta::milliseconds(THUMBNAIL_LOAD_INTERVAL_MS), false)
    }
}

#[derive(Default)]
pub(crate) struct App {
    state: AppStateRef,
    tasks: TaskState,

    vault_loading: bool,
    thumbnail_params: ThumbnailGridParams,
    thumbnail_grid: Option<ThumbnailGridInfo>,
    thumbnail_grid_first_visible: Option<String>,
    thumbnail_grid_scroll_cooldown: Option<DateTime<Utc>>,
    thumbnail_grid_set_scroll: bool,

    sorts: Vec<SortExpression>,
    filter: FilterExpression,

    item_list_cache: Option<ItemCache>,
    lq_thumbnail_cache: ThumbnailCache,
    thumbnail_cache: ThumbnailCache,

    msg_dialogs: Vec<MessageDialog>,
    new_vault_dialog: NewVaultDialog,
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
                TimeDelta::milliseconds(THUMBNAIL_LOAD_INTERVAL_MS),
                false,
            ),
            lq_thumbnail_cache: ThumbnailCache::new(
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
                Err(TaskError::WasmNotImplemented) => {
                    self.error("Not implemented in WASM".to_string())
                }
                Err(TaskError::Error(e)) => self.error(format!("{e:#}")),
                _ => {}
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
            let state = self.state.blocking_read();
            let Some(current_vault) = state.get_current_vault() else {
                return;
            };

            ui.label(format!("Current vault: {}", current_vault.name));

            let item_cache_params = ItemCacheParams {
                vault_name: current_vault.name.to_string(),
                filter: self.filter.clone(),
                sorts: self.sorts.to_vec(),
            };
            if self.item_list_cache.is_none()
                || self.item_list_cache.as_ref().unwrap().params != item_cache_params
            {
                let mut err = None;
                match get_filtered_and_sorted_items(&current_vault, &self.filter, &self.sorts) {
                    Ok(items) => {
                        self.item_list_cache = Some(ItemCache {
                            params: item_cache_params,
                            item_paths: items.into_iter().map(|i| i.path().to_string()).collect(),
                        })
                    }
                    Err(e) => err = Some(format!("{e:#}")),
                };

                if let Some(e) = err {
                    drop(current_vault);
                    drop(state);
                    self.error(e);
                    return;
                }
            }

            self.thumbnail_params.container_width = ui.available_width();
            ui.add(egui::widgets::Slider::new(
                &mut self.thumbnail_params.max_row_height,
                128.0..=1024.0,
            ));

            let mut err = None;

            if self.thumbnail_grid.is_none()
                || self.thumbnail_params != self.thumbnail_grid.as_ref().unwrap().params
            {
                self.thumbnail_grid_set_scroll = true;
                ctx.request_repaint();
                let params = self.thumbnail_params.clone();

                if let Some(items) = &self.item_list_cache {
                    match compute_thumbnails_grid(params, &items.resolve_refs(&current_vault)) {
                        Ok(info) => self.thumbnail_grid = Some(info),
                        Err(e) => err = Some(format!("{e:#}")),
                    };
                }
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
                    let Some(grid) = self.thumbnail_grid.as_ref() else {
                        return;
                    };
                    if grid.thumbnails.is_empty() {
                        return;
                    }

                    const PADDING: f32 = 8.0;
                    const ROUNDING: f32 = 4.0;

                    let abs_min = ui.min_rect().min.to_vec2();
                    let abs_vp = vp.translate(abs_min);
                    let max_y = grid.thumbnails.last().unwrap().bounds.max.y;
                    ui.set_width(ui.available_width());
                    ui.set_height(max_y);
                    ui.set_clip_rect(abs_vp);

                    let mut next_first_visible: Option<String> = None;

                    for item in grid.thumbnails.iter() {
                        let abs_bounds = item.bounds.translate(abs_min);
                        let text = egui::Label::new(item.path.clone());

                        // scroll to item if resize event has occurred
                        if self.thumbnail_grid_set_scroll
                            && &item.path
                                == self
                                    .thumbnail_grid_first_visible
                                    .as_ref()
                                    .unwrap_or(&"".to_string())
                        {
                            info!("do scroll to {} at {:?}", &item.path, &abs_bounds);
                            ui.scroll_to_rect(abs_bounds, Some(egui::Align::Min));
                            self.thumbnail_grid_set_scroll = false;
                            self.thumbnail_grid_scroll_cooldown = Some(Utc::now().add(
                                TimeDelta::milliseconds(THUMBNAIL_SCROLL_COOLDOWN_INTERVAL_MS),
                            ));
                        }
                        // mark current item as item to scroll to when resize occurs
                        else if self.thumbnail_grid_scroll_cooldown.unwrap_or(Utc::now())
                            <= Utc::now()
                            && item.bounds.max.y > (vp.min.y + THUMBNAIL_VISIBLE_THRESHOLD)
                            && next_first_visible.is_none()
                        {
                            info!("next first visible: {}", &item.path);
                            next_first_visible = Some(item.path.clone());
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

                    self.thumbnail_grid_first_visible = next_first_visible;

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
