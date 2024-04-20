use eframe::{egui, Frame};
use eframe::egui::Context;

#[derive(Default)]
pub(crate) struct App {}

impl eframe::App for App {
    fn update(&mut self, ctx: &Context, frame: &mut Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
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
