use crate::data::{FieldDefinition, FieldStore};
use crate::fields;
use eframe::egui::{
    vec2, Align, ClippedPrimitive, Color32, FontSelection, Rect, Response, Rounding, Sense, Ui,
    Vec2, Widget, WidgetInfo, WidgetText, WidgetType,
};
use eframe::emath::Rot2;
use eframe::epaint;
use eframe::epaint::{ClippedShape, Hsva, Pos2, Primitive};
use std::f32::consts::{FRAC_1_SQRT_2, SQRT_2};

pub struct Tag<'a> {
    definition: &'a FieldDefinition,
}

impl<'a> Tag<'a> {
    pub fn new(definition: &'a FieldDefinition) -> Self {
        Self { definition }
    }
}

const TAG_PADDING: Vec2 = vec2(4.0, 4.0);
const WHITE_TEXT: Color32 = Color32::from_rgb(240, 240, 240);
const BLACK_TEXT: Color32 = Color32::from_rgb(20, 20, 20);

const INV_2SQRT2: f32 = 0.353_553_38_f32;
const ONE_MINUS_INV_SQRT2: f32 = 0.292_893_23_f32;

const BORDER_RADIUS: f32 = 4.0;

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
        let mut bg_size = galley.size() + 2.0 * TAG_PADDING;

        let hanger_size = vec2(bg_size.y / 2.0, bg_size.y);
        let hanger_offset = vec2(hanger_size.x, 0.0);
        bg_size += hanger_offset;
        let (rect, res) = ui.allocate_exact_size(bg_size, Sense::click_and_drag());

        res.widget_info(|| WidgetInfo::labeled(WidgetType::Label, galley.text()));

        if ui.is_rect_visible(res.rect) {
            let visuals = ui.style().visuals.widgets.inactive;
            let bg = self
                .definition
                .get_known_field_value(fields::meta::COLOUR)
                .ok()
                .flatten()
                .map(|[r, g, b]| Color32::from_rgb(r, g, b))
                .unwrap_or(visuals.bg_fill);

            let fg = if Hsva::from(bg).v > 0.5 {
                BLACK_TEXT
            } else {
                WHITE_TEXT
            };

            let hanger_bbox = Rect::from_min_size(rect.min, hanger_size);
            let hanger_rect_width =
                FRAC_1_SQRT_2 * bg_size.y + 2.0 * (1.0 - FRAC_1_SQRT_2) * BORDER_RADIUS;
            let hanger_rect =
                Rect::from_center_size(hanger_bbox.right_center(), Vec2::splat(hanger_rect_width));
            let hanger_shape =
                epaint::RectShape::filled(hanger_rect, Rounding::same(BORDER_RADIUS), bg);
            let hanger_prim = ui
                .ctx()
                .tessellate(
                    vec![ClippedShape {
                        shape: hanger_shape.into(),
                        clip_rect: hanger_rect,
                    }],
                    ui.ctx().pixels_per_point(),
                )
                .swap_remove(0);
            let mut hanger_mesh = match hanger_prim.primitive {
                Primitive::Mesh(m) => m,
                Primitive::Callback(_) => {
                    return res;
                }
            };
            hanger_mesh.rotate(
                Rot2::from_angle(std::f32::consts::FRAC_PI_4),
                hanger_bbox.right_center(),
            );

            let label_bbox = Rect::from_min_size(rect.min + hanger_offset, bg_size - hanger_offset);
            let text_rect = label_bbox.shrink2(TAG_PADDING);
            let galley_pos = match galley.job.halign {
                Align::LEFT => text_rect.left_top(),
                Align::Center => text_rect.center_top(),
                Align::RIGHT => text_rect.right_top(),
            };

            let p = ui.painter_at(rect);
            p.add(hanger_mesh);
            p.add(epaint::RectShape::filled(
                label_bbox,
                Rounding {
                    ne: BORDER_RADIUS,
                    nw: 0.0,
                    se: BORDER_RADIUS,
                    sw: 0.0,
                },
                bg,
            ));
            p.add(epaint::CircleShape::filled(
                hanger_bbox.left_center() + vec2((SQRT_2 + 0.5) * BORDER_RADIUS, 0.0),
                BORDER_RADIUS / 2.0,
                fg,
            ));
            p.add(epaint::TextShape::new(galley_pos, galley, fg));
        }

        res
    }
}
