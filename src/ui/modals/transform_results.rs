use crate::state::AppStateRef;
use crate::tasks::transform::{PathContext, PathTransformResult};
use crate::ui::buttons;
use crate::ui::modals::AppModal;
use eframe::egui;
use egui_modal::{Modal, ModalStyle};
use std::path::Path;

pub struct TransformResults {
    modal: Option<Modal>,
    results: Vec<anyhow::Result<PathTransformResult>>,
    app_state: AppStateRef,
    opened: bool,
    is_open: bool,
}

#[derive(Copy, Clone, PartialEq, Eq, derive_more::Display)]
enum ResultType {
    Move,
    Copy,
    Skip,
    Delete,
    Error,
}

impl From<&anyhow::Result<PathTransformResult>> for ResultType {
    fn from(value: &anyhow::Result<PathTransformResult>) -> Self {
        match value {
            Ok(PathTransformResult::CopySuccess { .. }) => Self::Copy,
            Ok(PathTransformResult::MoveSuccess { .. }) => Self::Move,
            Ok(PathTransformResult::RemovedWithoutTransform(_)) => Self::Delete,
            Ok(PathTransformResult::NoTransform(_)) => Self::Skip,
            Err(_) => Self::Error,
        }
    }
}

fn get_old_path(res: &anyhow::Result<PathTransformResult>) -> Option<&Path> {
    match res {
        Ok(
            PathTransformResult::NoTransform(buf)
            | PathTransformResult::RemovedWithoutTransform(buf)
            | PathTransformResult::MoveSuccess { removed: buf, .. }
            | PathTransformResult::CopySuccess { original: buf, .. },
        ) => Some(buf.as_path()),
        Err(e) => match e.downcast_ref::<PathContext>() {
            Some(PathContext(buf)) => Some(buf.as_path()),
            None => None,
        },
    }
}

fn get_new_path(res: &anyhow::Result<PathTransformResult>) -> Option<&Path> {
    match res {
        Ok(
            PathTransformResult::MoveSuccess { created: buf, .. }
            | PathTransformResult::CopySuccess { copy: buf, .. },
        ) => Some(buf.as_path()),
        _ => None,
    }
}

impl TransformResults {
    pub fn new(results: Vec<anyhow::Result<PathTransformResult>>) -> Self {
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
                    .body(|mut body| {
                        body.rows(24.0, self.results.len(), |mut row| {
                            let Some(item) = self.results.get(row.index()) else {
                                return;
                            };

                            row.col(|ui| {
                                ui.label(ResultType::from(item).to_string());
                            });
                            row.col(|ui| {
                                if let Some(s) = get_old_path(item).map(|p| p.to_string_lossy()) {
                                    ui.label(s);
                                }
                            });
                            row.col(|ui| {
                                if let Some(s) = get_new_path(item).map(|p| p.to_string_lossy()) {
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
