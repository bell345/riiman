use eframe::{egui, Frame};
use eframe::egui::Context;
use poll_promise::Promise;
use tracing::info;

use crate::data::Vault;
use crate::state::AppState;
use crate::tasks::{TaskError, TaskState};
use crate::tasks::TaskResult::VaultLoaded;
use crate::ui::modals::error::ErrorDialog;
use crate::ui::modals::new_vault::NewVaultDialog;

mod modals;

#[derive(Default)]
pub(crate) struct App {
    state: AppState,
    tasks: TaskState,

    error_dialog: ErrorDialog,
    new_vault_dialog: NewVaultDialog,

    vault_load_promise: Option<Promise<Option<String>>>,
}

impl eframe::App for App {
    fn update(&mut self, ctx: &Context, _frame: &mut Frame) {
        if let Some(new_vault_name) = self.new_vault_dialog.update(ctx).ready() {
            let vault = Vault::new(new_vault_name);
            self.tasks.add_task(Promise::spawn_async(crate::tasks::save_new_vault(vault)))
        }

        for result in self.tasks.iter_ready() {
            match result {
                Ok(VaultLoaded(vault)) => self.state.load_vault(*vault),
                Err(TaskError::Error(e)) => self.error_dialog.open(e.to_string()),
                _ => {}
            }
        }

        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button(
                    "File",
                    |ui| {
                        if ui.button("New vault").clicked() {
                            info!("New vault clicked!");

                            self.new_vault_dialog.open();

                            ui.close_menu();
                        }
                        if ui.button("Open vault").clicked() {
                            info!("Open vault clicked!");

                            self.tasks.add_task(Promise::spawn_async(crate::tasks::choose_and_load_vault()));

                            ui.close_menu();
                        }

                        ui.separator();

                        if ui.button("Quit").clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    },
                );
                ui.add_space(16.0);
            });
        });

        egui::TopBottomPanel::bottom("bottom_panel").show(ctx, |ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(format!("{} {} ({}) compiled {}",
                                 crate::built_info::PKG_NAME,
                                 crate::built_info::PKG_VERSION,
                                 crate::built_info::GIT_COMMIT_HASH_SHORT.unwrap_or("?"),
                                 crate::built_info::built_time()
                ));
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(current_vault) = &self.state.current_vault {
                ui.label(format!("Current vault: {current_vault}"));
            }
            egui::ScrollArea::both().show(ui, |ui| {
                ui.image(egui::include_image!("../../res/ferris.svg"));
            })
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
