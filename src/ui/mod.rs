use eframe::egui;
use eframe::egui::Color32;
use poll_promise::Promise;
use rand::Rng;
use rand_seeder::{Seeder, SipRng};
use std::ops::Deref;
use std::path::Path;
use tracing::info;

use crate::data::{Item, Vault};
use crate::fields;
use crate::state::{AppState, AppStateRef};
use crate::tasks::compute::compute_thumbnails_grid;
use crate::tasks::sort::{
    get_filtered_and_sorted_items, FilterExpression, SortDirection, SortExpression,
};
use crate::tasks::AsyncTaskResult::{ImportComplete, VaultLoaded, VaultSaved};
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

#[derive(Default)]
pub(crate) struct App {
    state: AppStateRef,
    tasks: TaskState,

    vault_loading: bool,
    thumbnail_params: ThumbnailGridParams,
    thumbnail_grid: Option<ThumbnailGridInfo>,

    sorts: Vec<SortExpression>,
    filter: FilterExpression,

    item_list_cache: Option<ItemCache>,

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

    fn state_mut(&self) -> tokio::sync::RwLockWriteGuard<AppState> {
        self.state.blocking_write()
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

    fn setup(&mut self, _ctx: &egui::Context, storage: Option<&dyn eframe::Storage>) {
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
                24.0..=1024.0,
            ));

            if self.thumbnail_grid.is_none()
                || self.thumbnail_params != self.thumbnail_grid.as_ref().unwrap().params
            {
                let params = self.thumbnail_params.clone();

                if let Some(items) = &self.item_list_cache {
                    let mut err = None;
                    match compute_thumbnails_grid(params, &items.resolve_refs(&current_vault)) {
                        Ok(info) => self.thumbnail_grid = Some(info),
                        Err(e) => err = Some(format!("{e:#}")),
                    };

                    if let Some(e) = err {
                        drop(current_vault);
                        drop(state);
                        self.error(e);
                        return;
                    }
                }
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

                    for item in grid.thumbnails.iter() {
                        let abs_bounds = item.bounds.translate(abs_min);
                        let text = egui::Label::new(item.path.clone());
                        if vp.intersects(item.bounds) {
                            let mut rng: SipRng = Seeder::from(&item.path).make_rng();
                            let colour = Color32::from_rgb(
                                rng.gen::<u8>(),
                                rng.gen::<u8>(),
                                rng.gen::<u8>(),
                            );
                            ui.painter_at(abs_bounds)
                                .add(egui::epaint::RectShape::filled(
                                    abs_bounds.shrink(PADDING),
                                    egui::epaint::Rounding::from(ROUNDING),
                                    colour,
                                ));
                            ui.put(abs_bounds.shrink(PADDING), text);
                        }
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
