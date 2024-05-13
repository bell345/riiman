use std::f32::consts::{FRAC_1_SQRT_2, SQRT_2};
use std::sync::Arc;

use eframe::egui::{
    vec2, Align, Color32, FontSelection, Galley, Rect, Response, Rounding, Sense, TextStyle, Ui,
    Vec2, Widget, WidgetInfo, WidgetText, WidgetType,
};
use eframe::emath::Rot2;
use eframe::epaint::{self, ClippedShape, Hsva, HsvaGamma, Primitive};
use relativetime::RelativeTime;

use crate::data::kind::Value;
use crate::data::{FieldDefinition, FieldStore};
use crate::fields;

pub struct Tag<'a> {
    definition: &'a FieldDefinition,
    value: Option<&'a Value>,
    use_small: bool,
}

impl<'a> Tag<'a> {
    pub fn new(definition: &'a FieldDefinition) -> Self {
        Self {
            definition,
            value: None,
            use_small: false,
        }
    }

    pub fn small(mut self, small: bool) -> Self {
        self.use_small = small;
        self
    }

    pub fn value(mut self, value: &'a Value) -> Self {
        self.value = Some(value);
        self
    }
}

const TAG_PADDING: Vec2 = vec2(4.0, 4.0);
const WHITE_TEXT: Color32 = Color32::from_rgb(240, 240, 240);
const BLACK_TEXT: Color32 = Color32::from_rgb(20, 20, 20);

const BORDER_RADIUS: f32 = 4.0;

impl<'a> Tag<'a> {
    fn text_to_galley(&self, ui: &Ui, text: WidgetText) -> Arc<Galley> {
        let mut layout_job = text.into_layout_job(
            ui.style(),
            if self.use_small {
                FontSelection::Style(TextStyle::Small)
            } else {
                FontSelection::Default
            },
            ui.layout().vertical_align(),
        );

        layout_job.wrap.max_width = f32::INFINITY;
        layout_job.halign = ui.layout().horizontal_placement();
        layout_job.justify = ui.layout().horizontal_justify();

        ui.fonts(|f| f.layout_job(layout_job))
    }

    fn galley(&self, ui: &Ui) -> Arc<Galley> {
        self.text_to_galley(ui, WidgetText::from(&self.definition.name))
    }

    fn value_galley(&self, ui: &Ui) -> Option<Arc<Galley>> {
        let value = self.value.as_ref()?;

        let text = WidgetText::from(match value {
            Value::Tag => return None,
            Value::Boolean(b) => {
                if *b {
                    "\u{2714}".into()
                } else {
                    "\u{274c}".into()
                }
            }
            Value::Int(i) => format!("{i}"),
            Value::UInt(i) => format!("{i}"),
            Value::Float(f) => format!("{f}"),
            Value::Str(s) => s.into(),
            Value::ItemRef(s) => s.into(),
            Value::List(_) => return None,
            Value::Colour(_) => " ".into(),
            Value::Dictionary(_) => return None,
            Value::DateTime(dt) => dt.to_relative(),
        });

        Some(self.text_to_galley(ui, text))
    }

    /// returns hanger size, label size, value size, total size
    fn sizes(
        &self,
        galley: &Arc<Galley>,
        value_galley: &Option<Arc<Galley>>,
    ) -> (Vec2, Vec2, Vec2, Vec2) {
        let label_size = galley.size() + 2.0 * TAG_PADDING;
        let mut total_size = label_size;

        let hanger_size = vec2(label_size.y / 2.0, label_size.y);
        let hanger_offset = vec2(hanger_size.x, 0.0);
        total_size += hanger_offset;

        let mut value_size = Vec2::ZERO;
        if let Some(value_galley) = value_galley.as_ref() {
            value_size = value_galley.size() + 2.0 * TAG_PADDING;
            let value_offset = vec2(value_size.x, 0.0);
            total_size += value_offset;
        }
        (hanger_size, label_size, value_size, total_size)
    }

    pub fn size(&self, ui: &Ui) -> Vec2 {
        self.sizes(&self.galley(ui), &self.value_galley(ui)).3
    }

    pub fn paint(&self, ui: &Ui, rect: Rect, response: Option<Response>) {
        let p = ui.painter_at(rect);
        let galley = self.galley(ui);
        let value_galley = self.value_galley(ui);
        let has_value = value_galley.is_some();
        let (hanger_size, label_size, value_size, total_size) = self.sizes(&galley, &value_galley);

        let visuals = ui.style().visuals.widgets.inactive;
        let mut bg = self
            .definition
            .get_known_field_value(fields::meta::COLOUR)
            .ok()
            .flatten()
            .map(|[r, g, b]| Color32::from_rgb(r, g, b))
            .unwrap_or(visuals.bg_fill);

        let mut hsva = HsvaGamma::from(bg);

        if response
            .as_ref()
            .is_some_and(|r| r.is_pointer_button_down_on())
        {
            hsva.v *= 0.8;
        } else if response.as_ref().is_some_and(|r| r.hovered()) {
            hsva.v = 1.0 - 0.8 * (1.0 - hsva.v);
        }

        bg = Color32::from(hsva);

        let value_bg = bg.gamma_multiply(0.5).to_opaque();

        let fg = if Hsva::from(bg).v > 0.5 {
            BLACK_TEXT
        } else {
            WHITE_TEXT
        };

        // hanger

        let hanger_bbox = Rect::from_min_size(rect.min, hanger_size);
        let hanger_rect_width =
            FRAC_1_SQRT_2 * total_size.y + 2.0 * (1.0 - FRAC_1_SQRT_2) * BORDER_RADIUS;
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
                return;
            }
        };
        hanger_mesh.rotate(
            Rot2::from_angle(std::f32::consts::FRAC_PI_4),
            hanger_bbox.right_center(),
        );
        p.add(hanger_mesh);

        // hanger ring
        p.add(epaint::CircleShape::filled(
            hanger_bbox.left_center() + vec2((SQRT_2 + 0.5) * BORDER_RADIUS, 0.0),
            BORDER_RADIUS / 2.0,
            fg,
        ));

        // label
        let label_offset = vec2(total_size.x - label_size.x - value_size.x, 0.0);
        let label_bbox = Rect::from_min_size(rect.min + label_offset, label_size);
        p.add(epaint::RectShape::filled(
            label_bbox,
            Rounding {
                ne: if has_value { 0.0 } else { BORDER_RADIUS },
                nw: 0.0,
                se: if has_value { 0.0 } else { BORDER_RADIUS },
                sw: 0.0,
            },
            bg,
        ));

        let text_rect = label_bbox.shrink2(TAG_PADDING);
        let galley_pos = match galley.job.halign {
            Align::LEFT => text_rect.left_top(),
            Align::Center => text_rect.center_top(),
            Align::RIGHT => text_rect.right_top(),
        };
        p.add(epaint::TextShape::new(galley_pos, galley, fg));

        // value
        if let Some(value_galley) = value_galley {
            let value_offset = vec2(total_size.x - value_size.x, 0.0);
            let value_bbox = Rect::from_min_size(rect.min + value_offset, value_size);
            let value_text_rect = value_bbox.shrink2(TAG_PADDING);
            let value_galley_pos = match value_galley.job.halign {
                Align::LEFT => value_text_rect.left_top(),
                Align::Center => value_text_rect.center_top(),
                Align::RIGHT => value_text_rect.right_top(),
            };
            p.add(epaint::RectShape::filled(
                value_bbox,
                Rounding {
                    ne: BORDER_RADIUS,
                    nw: 0.0,
                    se: BORDER_RADIUS,
                    sw: 0.0,
                },
                value_bg,
            ));
            p.add(epaint::TextShape::new(value_galley_pos, value_galley, fg));
        }
    }
}

impl<'a> Widget for Tag<'a> {
    fn ui(self, ui: &mut Ui) -> Response {
        let galley = self.galley(ui);
        let value_galley = self.value_galley(ui);

        let (_, _, _, total_size) = self.sizes(&galley, &value_galley);

        let (rect, res) = ui.allocate_exact_size(total_size, Sense::click_and_drag());

        res.widget_info(|| WidgetInfo::labeled(WidgetType::Label, galley.text()));

        if ui.is_rect_visible(res.rect) {
            self.paint(ui, rect, Some(res.clone()));
        }

        res
    }
}
