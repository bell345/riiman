use crate::state::AppStateRef;
use crate::tasks::transform::{TransformResult, TransformReturn};
use crate::ui::modals::AppModal;
use crate::ui::{buttons, modals};
use anyhow::anyhow;
use eframe::egui;
use egui_modal::{Modal, ModalStyle};

pub struct TransformResults {
    modal: Option<Modal>,
    results: Vec<anyhow::Result<TransformResult>>,
    app_state: AppStateRef,
    opened: bool,
    is_open: bool,
}

#[derive(Copy, Clone, PartialEq, Eq, derive_more::Display)]
enum ResultType {
    Move,
    Copy,
    InPlace,
    Skip,
    Delete,
    Error,
}

impl From<&anyhow::Result<TransformResult>> for ResultType {
    fn from(value: &anyhow::Result<TransformResult>) -> Self {
        match value {
            Ok(TransformResult::CopySuccess { .. }) => Self::Copy,
            Ok(TransformResult::MoveSuccess { .. }) => Self::Move,
            Ok(TransformResult::InPlaceTransform(_)) => Self::InPlace,
            Ok(TransformResult::RemovedWithoutTransform(_)) => Self::Delete,
            Ok(TransformResult::NoTransform(_)) => Self::Skip,
            Err(_) => Self::Error,
        }
    }
}

impl TransformResults {
    pub fn new(results: Vec<anyhow::Result<TransformResult>>) -> Self {
        Self {
            modal: None,
            results,
            app_state: Default::default(),
            opened: false,
            is_open: true,
        }
    }

    fn modal_contents(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui_extras::TableBuilder::new(ui)
                    .column(egui_extras::Column::exact(100.0))
                    .column(egui_extras::Column::exact(800.0))
                    .column(egui_extras::Column::remainder())
                    .header(18.0, |mut row| {
                        row.col(|ui| {
                            ui.label("");
                        });
                        row.col(|ui| {
                            ui.label("Old");
                        });
                        row.col(|ui| {
                            ui.label("New");
                        });
                    })
                    .body(|body| {
                        body.rows(24.0, self.results.len(), |mut row| {
                            let Some(item) = self.results.get(row.index()) else {
                                return;
                            };

                            row.col(|ui| {
                                let label = ResultType::from(item).to_string();
                                match item {
                                    Ok(_) => {
                                        ui.label(label);
                                    }
                                    Err(e) => {
                                        if ui.button(label).clicked() {
                                            self.app_state
                                                .add_dialog(modals::Message::error(format!("{e}")));
                                        }
                                    }
                                };
                            });
                            row.col(|ui| {
                                if let Some(s) = item.orig_path().map(|p| p.to_string_lossy()) {
                                    ui.label(s);
                                }
                            });
                            row.col(|ui| {
                                if let Some(s) = item.new_path().map(|p| p.to_string_lossy()) {
                                    ui.label(s);
                                }
                            });
                        });
                    });
            });
    }
}

impl AppModal for TransformResults {
    fn id(&self) -> egui::Id {
        "transform_results_modal".into()
    }

    fn update(&mut self, ctx: &egui::Context, app_state: AppStateRef) {
        let modal = Modal::new(ctx, self.id().value()).with_style(&ModalStyle {
            default_width: Some(600.0),
            default_height: Some(280.0),
            ..Default::default()
        });
        self.app_state = app_state;
        let mut is_open = self.is_open;
        let mut do_close = false;

        egui::Window::new("Transform Results")
            .id(self.id())
            .open(&mut is_open)
            .min_size([700.0, 250.0])
            .show(ctx, |ui| {
                buttons(self.id(), ui, |ui| {
                    if ui.button("Close").clicked() {
                        do_close = true;
                    }
                });

                self.modal_contents(ui);
            });

        if do_close {
            is_open = false;
        }

        self.is_open = is_open;
        self.opened = is_open;

        self.modal = Some(modal);
    }

    fn is_open(&self) -> bool {
        self.is_open
    }
}
