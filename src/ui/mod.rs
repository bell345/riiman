use eframe::egui;
use poll_promise::Promise;
use tracing::info;

use crate::data::Vault;
use crate::state::{AppState, AppStateRef};
use crate::tasks::{ProgressSender, ProgressState, TaskError, TaskReturn, TaskState};
use crate::tasks::TaskResult::VaultLoaded;
use crate::ui::modals::error::ErrorDialog;
use crate::ui::modals::new_vault::NewVaultDialog;

mod modals;

#[derive(Default)]
pub(crate) struct App {
    state: AppStateRef,
    tasks: TaskState,

    thumbnail_height: u32,

    error_dialog: ErrorDialog,
    new_vault_dialog: NewVaultDialog,
}

impl App {
    pub fn new() -> Self {
        Self {
            thumbnail_height: 128,
            ..Default::default()
        }
    }

    fn add_task(
        &mut self,
        name: &'static str,
        task_factory: impl FnOnce(AppStateRef, ProgressSender) -> Promise<TaskReturn>,
    ) {
        self.tasks
            .add_task_with_progress(name.to_string(), |tx| task_factory(self.state.clone(), tx));
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
        self.error_dialog.update(ctx);

        if let Some(new_vault_name) = self.new_vault_dialog.update(ctx).ready() {
            self.add_task("Create vault", move |_, p| {
                Promise::spawn_async(crate::tasks::vault::save_new_vault(Vault::new(
                    new_vault_name,
                ), p))
            });
        }

        for result in self.tasks.iter_ready() {
            match result {
                Ok(VaultLoaded(vault)) => self.state_mut().load_vault(*vault),
                Err(TaskError::Error(e)) => self.error_dialog.open(format!("{e:#}")),
                _ => {}
            }
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
                    [] => {},
                    [(name, ProgressState::NotStarted), ..] |
                    [(name, ProgressState::Indeterminate), ..] => {
                        ui.add(egui::ProgressBar::new(0.0).text(name).animate(true));
                    },
                    [(name, ProgressState::Determinate(progress)), ..] => {
                        ui.add(egui::ProgressBar::new(*progress).text(name).show_percentage());
                    },
                    [(name, ProgressState::Completed), ..] => {
                        ui.add(egui::ProgressBar::new(1.0).text(name));
                    },
                };
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(current_vault) = &self.state().current_vault {
                ui.label(format!("Current vault: {current_vault}"));
            }

            ui.add(egui::widgets::Slider::new(
                &mut self.thumbnail_height,
                24..=1024,
            ));

            /*egui::ScrollArea::both().show(ui, |ui| {
                ui.image(egui::include_image!("../../res/ferris.svg"));
            })*/

            egui::ScrollArea::vertical()
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
