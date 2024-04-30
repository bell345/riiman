use eframe::egui;
use eframe::egui::Color32;
use poll_promise::Promise;
use rand::Rng;
use rand_seeder::{Seeder, SipRng};
use tracing::info;

use crate::data::Vault;
use crate::state::{AppState, AppStateRef};
use crate::tasks::TaskResult::{ImportComplete, ThumbnailGrid, VaultLoaded};
use crate::tasks::{
    DummyProgressSender, ProgressSenderRef, ProgressState, TaskError, TaskReturn, TaskState,
    ThumbnailGridInfo, ThumbnailGridParams,
};
use crate::ui::modals::message::MessageDialog;
use crate::ui::modals::new_vault::NewVaultDialog;

mod modals;

#[derive(Default)]
pub(crate) struct App {
    state: AppStateRef,
    tasks: TaskState,

    thumbnail_params: ThumbnailGridParams,
    thumbnail_grid: ThumbnailGridInfo,

    msg_dialogs: Vec<MessageDialog>,
    new_vault_dialog: NewVaultDialog,
}

impl App {
    pub fn new() -> Self {
        Self {
            thumbnail_params: ThumbnailGridParams {
                max_row_height: 120.0,
                container_width: 0.0
            },
            ..Default::default()
        }
    }

    fn add_task(
        &mut self,
        name: &'static str,
        task_factory: impl FnOnce(AppStateRef, ProgressSenderRef) -> Promise<TaskReturn>,
    ) {
        self.tasks
            .add_task_with_progress(name, |tx| task_factory(self.state.clone(), tx));
    }

    fn run_task(
        &self,
        task_factory: impl FnOnce(AppStateRef, ProgressSenderRef) -> TaskReturn,
    ) -> TaskReturn {
        task_factory(self.state.clone(), DummyProgressSender::new())
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
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.msg_dialogs
            .retain_mut(|dialog| dialog.update(ctx).is_open());

        if let Some(new_vault_name) = self.new_vault_dialog.update(ctx).ready() {
            self.add_task("Create vault", move |_, p| {
                Promise::spawn_async(crate::tasks::vault::save_new_vault(
                    Vault::new(new_vault_name),
                    p,
                ))
            });
        }

        for result in self.tasks.iter_ready() {
            match result {
                Ok(VaultLoaded(vault)) => {
                    self.state_mut().load_vault(*vault);
                },
                Ok(ImportComplete { path, results }) => {
                    let total = results.len();
                    let success = results.iter().filter(|r| r.is_ok()).count();
                    let body = format!(
                        "Import of {} complete. {success}/{total} images imported successfully.",
                        path.display()
                    );
                    self.success("Import complete".to_string(), body);
                }
                Ok(ThumbnailGrid(info)) => self.thumbnail_grid = info,
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
                    if ui.button("New").clicked() {
                        info!("New vault clicked!");

                        self.new_vault_dialog.open();

                        ui.close_menu();
                    }
                    if ui.button("Open").clicked() {
                        info!("Open vault clicked!");

                        self.add_task("Load vault", |_, p| {
                            Promise::spawn_async(crate::tasks::vault::choose_and_load_vault(p))
                        });

                        ui.close_menu();
                    }

                    if self.state.blocking_read().get_current_vault().is_some()
                        && ui.button("Save").clicked()
                    {
                        info!("Save vault clicked!");

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
                                .text(format!("{}% {name}", (progress * 100.0).floor() as u32))
                        );
                    }
                    [(name, ProgressState::DeterminateWithMessage(progress, msg)), ..] => {
                        ui.add(
                            egui::ProgressBar::new(*progress)
                                .text(format!("{}% {name}: {msg}", (progress * 100.0).floor() as u32))
                        );
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

            self.thumbnail_params.container_width = ui.available_width();
            ui.add(egui::widgets::Slider::new(
                &mut self.thumbnail_params.max_row_height,
                24.0..=1024.0,
            ));

            if !self.thumbnail_grid.is_loading
                && self.thumbnail_params != self.thumbnail_grid.params
            {
                let params = self.thumbnail_params.clone();
                self.thumbnail_grid.is_loading = true;

                if let Ok(ThumbnailGrid(info)) = self.run_task(
                    |state, p| crate::tasks::compute::compute_thumbnails_grid(params, state, p)) {
                    self.thumbnail_grid = info;
                }
            }

            egui::ScrollArea::vertical()
                .auto_shrink([false; 2])
                .show_viewport(ui, |ui, vp| {
                    if self.thumbnail_grid.thumbnails.is_empty() {
                        return;
                    }
                    
                    const PADDING: f32 = 8.0;
                    const ROUNDING: f32 = 4.0;

                    let abs_min = ui.min_rect().min.to_vec2();
                    let abs_vp = vp.translate(abs_min);
                    let max_y = self.thumbnail_grid.thumbnails.last().unwrap().bounds.max.y;
                    ui.set_width(ui.available_width());
                    ui.set_height(max_y);
                    ui.set_clip_rect(abs_vp);

                    for item in self.thumbnail_grid.thumbnails.iter() {
                        let abs_bounds = item.bounds.translate(abs_min);
                        let text = egui::Label::new(format!(
                            "{}\nui_min: {:?}\nvp: {:?}\nabs_vp: {:?}\nbounds: {:?}\nabs_bound: {:?}",
                            item.path.as_str(),
                            abs_min.y,
                            vp.min.y..vp.max.y,
                            abs_vp.min.y..abs_vp.max.y,
                            item.bounds.min.y..item.bounds.max.y,
                            abs_bounds.min.y..item.bounds.max.y
                        ));
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
}

impl App {
    pub(crate) fn run(&mut self) -> Result<(), eframe::Error> {
        let options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default().with_inner_size([600.0, 800.0]),
            ..Default::default()
        };

        eframe::run_native(
            "riiman",
            options,
            Box::new(|cc| {
                egui_extras::install_image_loaders(&cc.egui_ctx);
                Box::<App>::default()
            }),
        )
    }
}
