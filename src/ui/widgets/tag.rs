use crate::data::FieldDefinition;
use eframe::egui::{
    Align, FontSelection, Response, Sense, Ui, Widget, WidgetInfo, WidgetText, WidgetType,
};
use eframe::epaint;

pub struct Tag<'a> {
    definition: &'a FieldDefinition,
}

impl<'a> Tag<'a> {
    pub fn new(definition: &'a FieldDefinition) -> Self {
        Self { definition }
    }
}

impl<'a> Widget for Tag<'a> {
    fn ui(self, ui: &mut Ui) -> Response {
        let label_text = WidgetText::from(&self.definition.name);
        let mut layout_job = label_text.into_layout_job(
            ui.style(),
            FontSelection::Default,
            ui.layout().vertical_align(),
        );

        layout_job.wrap.max_width = f32::INFINITY;
        layout_job.halign = ui.layout().horizontal_placement();
        layout_job.justify = ui.layout().horizontal_justify();

        let galley = ui.fonts(|f| f.layout_job(layout_job));
        let (rect, res) = ui.allocate_exact_size(galley.size(), Sense::click_and_drag());
        let galley_pos = match galley.job.halign {
            Align::LEFT => rect.left_top(),
            Align::Center => rect.center_top(),
            Align::RIGHT => rect.right_top(),
        };

        res.widget_info(|| WidgetInfo::labeled(WidgetType::Label, galley.text()));

        if ui.is_rect_visible(res.rect) {
            ui.painter().add(epaint::TextShape::new(
                galley_pos,
                galley,
                ui.style().visuals.text_color(),
            ));
        }

        res
    }
}
