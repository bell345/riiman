use eframe::egui;
use eframe::egui::{Response, Ui, Widget};
use std::ops::Add;
use std::sync::Arc;

pub struct SearchBox<'a> {
    text: &'a mut String,
}

impl<'a> SearchBox<'a> {
    pub fn new(text: &'a mut String) -> Self {
        Self { text }
    }
}

impl<'a> Widget for SearchBox<'a> {
    fn ui(self, ui: &mut Ui) -> Response {
        let mut layouter = |ui: &Ui, text: &str, _wrap_width: f32| -> Arc<egui::Galley> {
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
            egui::TextEdit::singleline(self.text)
                .desired_width(f32::INFINITY)
                .layouter(&mut layouter),
        );

        let style = ui.style();
        let painter = ui.painter_at(output.rect);

        let icon_width = painter
            .text(
                output.rect.min.add(egui::vec2(
                    style.spacing.button_padding.x,
                    output.rect.size().y / 2.0,
                )),
                egui::Align2::LEFT_CENTER,
                "\u{1f50d}",
                egui::TextStyle::Button.resolve(style),
                style.visuals.strong_text_color(),
            )
            .width();

        if self.text.is_empty() {
            painter.text(
                output.rect.min.add(egui::vec2(
                    style.spacing.button_padding.x + icon_width + style.spacing.button_padding.x,
                    output.rect.size().y / 2.0,
                )),
                egui::Align2::LEFT_CENTER,
                "Search...",
                egui::TextStyle::Body.resolve(style),
                style.visuals.weak_text_color(),
            );
        }

        output
    }
}
