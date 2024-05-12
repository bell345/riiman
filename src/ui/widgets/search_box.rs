use crate::data::FieldDefinition;
use crate::ui::widgets;
use eframe::egui;
use eframe::egui::{vec2, Color32, Rect, Response, Ui, Widget};
use std::ops::Add;
use std::sync::Arc;

pub struct SearchBox<'a> {
    text: &'a mut String,
    desired_width: f32,
    tags: Option<&'a Vec<FieldDefinition>>,
}

impl<'a> SearchBox<'a> {
    pub fn new(text: &'a mut String) -> Self {
        Self {
            text,
            desired_width: 200.0,
            tags: None,
        }
    }

    pub fn desired_width(mut self, desired_width: f32) -> Self {
        self.desired_width = desired_width;
        self
    }

    pub fn tags(mut self, tags: &'a Vec<FieldDefinition>) -> Self {
        self.tags = Some(tags);
        self
    }
}

impl<'a> Widget for SearchBox<'a> {
    fn ui(self, ui: &mut Ui) -> Response {
        let empty_vec = vec![];
        let tags = self.tags.unwrap_or(&empty_vec);
        let tag_sizes: Vec<_> = tags
            .iter()
            .map(|t| widgets::Tag::new(t).small(true).size(ui))
            .collect();

        let style = ui.style();
        let icon_galley = ui.fonts(|f| {
            f.layout_job(egui::text::LayoutJob::simple_singleline(
                "\u{1f50d}".into(),
                egui::TextStyle::Button.resolve(style),
                style.visuals.strong_text_color(),
            ))
        });

        let icon_reserved_width = icon_galley.rect.width() + style.spacing.icon_spacing;

        let reserved_width = icon_reserved_width
            + tag_sizes.iter().map(|s| s.x).sum::<f32>()
            + style.spacing.item_spacing.x * tag_sizes.len().saturating_sub(1) as f32;

        let mut layouter = |ui: &Ui, text: &str, _wrap_width: f32| -> Arc<egui::Galley> {
            let mut job = egui::text::LayoutJob::default();
            let style = ui.style();

            job.append(
                text,
                reserved_width,
                egui::TextFormat::simple(
                    egui::TextStyle::Body.resolve(style),
                    style.visuals.text_color(),
                ),
            );

            ui.fonts(|f| f.layout_job(job))
        };

        let output = ui.add(
            egui::TextEdit::singleline(self.text)
                .desired_width(self.desired_width)
                .layouter(&mut layouter),
        );

        let style = ui.style();
        let mut tag_location = output.rect.left_center() + vec2(icon_reserved_width, 0.0);
        for (def, size) in tags.iter().zip(tag_sizes) {
            let rect = Rect::from_min_size(tag_location - vec2(0.0, size.y / 2.0), size);
            widgets::Tag::new(def).small(true).paint(ui, rect);
            tag_location += vec2(size.x + style.spacing.item_spacing.x, 0.0);
        }

        let style = ui.style();
        let painter = ui.painter_at(output.rect);

        painter.galley(
            output.rect.min.add(vec2(
                style.spacing.button_padding.x,
                (output.rect.height() / 2.0) - (icon_galley.rect.height() / 2.0),
            )),
            icon_galley,
            Color32::TRANSPARENT,
        );

        if self.text.is_empty() {
            painter.text(
                output.rect.min.add(egui::vec2(
                    reserved_width + style.spacing.icon_spacing,
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
