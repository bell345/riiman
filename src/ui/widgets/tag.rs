use std::f32::consts::{FRAC_1_SQRT_2, SQRT_2};
use std::ops::Deref;
use std::sync::Arc;

use crate::data::{FieldDefinition, FieldStore, FieldType, FieldValue};
use crate::fields;
use crate::ui::theme;
use eframe::egui::{
    vec2, Align, Color32, FontSelection, Galley, Painter, Pos2, Rect, Response, Rounding, Sense,
    Stroke, TextStyle, Ui, Vec2, Widget, WidgetInfo, WidgetText, WidgetType,
};
use eframe::emath::Rot2;
use eframe::epaint::{self, ClippedShape, Hsva, HsvaGamma, Primitive};
use relativetime::RelativeTime;

pub struct Tag<'a> {
    definition: &'a FieldDefinition,
    value: Option<&'a FieldValue>,
    use_small: bool,
    selected: bool,
}

impl<'a> Tag<'a> {
    pub fn new(definition: &'a FieldDefinition) -> Self {
        Self {
            definition,
            value: None,
            use_small: false,
            selected: false,
        }
    }

    pub fn small(mut self, small: bool) -> Self {
        self.use_small = small;
        self
    }

    pub fn value(mut self, value: &'a FieldValue) -> Self {
        self.value = Some(value);
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }
}

const TAG_PADDING: Vec2 = vec2(4.0, 4.0);

const BORDER_RADIUS: f32 = 4.0;

const SELECTED_STROKE_WIDTH: f32 = 1.0;
const SELECTED_COLOUR: Color32 = Color32::from_rgb(255, 255, 255);

impl<'a> Tag<'a> {
    fn text_to_galley(&self, ui: &Ui, text: WidgetText, container_width: f32) -> Arc<Galley> {
        let mut layout_job = text.into_layout_job(
            ui.style(),
            if self.use_small {
                FontSelection::Style(TextStyle::Small)
            } else {
                FontSelection::Default
            },
            ui.layout().vertical_align(),
        );

        layout_job.wrap.max_width = container_width;
        layout_job.halign = ui.layout().horizontal_placement();
        layout_job.justify = ui.layout().horizontal_justify();

        ui.fonts(|f| f.layout_job(layout_job))
    }

    fn line_height(&self, ui: &Ui) -> f32 {
        self.text_to_galley(ui, WidgetText::from("i"), f32::INFINITY)
            .size()
            .y
    }

    fn galley(&self, ui: &Ui) -> Arc<Galley> {
        self.text_to_galley(
            ui,
            WidgetText::from(&*self.definition.name),
            ui.available_width(),
        )
    }

    fn value_galley(&self, ui: &Ui, label_width: f32) -> Option<Arc<Galley>> {
        let value = self.value.as_ref()?;

        let text = WidgetText::from(match value {
            FieldValue::Tag | FieldValue::Container => return None,
            FieldValue::Boolean(b) => {
                if *b {
                    "\u{2714}".into()
                } else {
                    "\u{274c}".into()
                }
            }
            FieldValue::Int(i) => format!("{i}"),
            FieldValue::Float(f) => format!("{f}"),
            FieldValue::String(s) => s.deref().into(),
            FieldValue::ItemRef((v, p)) => format!("{v}:{p}"),
            FieldValue::Colour(_) => "     ".into(),
            FieldValue::DateTime(dt) => dt.to_relative(),
            FieldValue::List(_) | FieldValue::Dictionary(_) => return None,
        });

        let hanger_width = self.line_height(ui) + 2.0 * TAG_PADDING.y;
        let padding = 2.0 * TAG_PADDING.x;
        Some(self.text_to_galley(
            ui,
            text,
            ui.available_width() - label_width - padding - hanger_width - padding,
        ))
    }

    /// returns hanger size, label size, value size, total size
    fn sizes(
        &self,
        line_height: f32,
        galley: &Arc<Galley>,
        value_galley: &Option<Arc<Galley>>,
    ) -> (Vec2, Vec2, Vec2, Vec2) {
        let mut inner_height = galley.size().y;
        if let Some(value_height) = value_galley.as_ref().map(|g| g.size().y) {
            inner_height = inner_height.max(value_height);
        }
        let outer_height = inner_height + 2.0 * TAG_PADDING.y;
        let outer_line_height = line_height + 2.0 * TAG_PADDING.y;

        let label_size = vec2(galley.size().x, inner_height) + 2.0 * TAG_PADDING;
        let mut total_size = label_size;

        let hanger_size = match self.definition.field_type {
            FieldType::Tag => vec2(outer_line_height / 2.0, outer_height),
            FieldType::Container => vec2(0.0, 0.0),
            _ => vec2(outer_line_height, outer_height),
        };
        let hanger_offset = vec2(hanger_size.x, 0.0);
        total_size += hanger_offset;

        let mut value_size = Vec2::ZERO;
        if let Some(value_galley) = value_galley.as_ref() {
            value_size = vec2(value_galley.size().x, inner_height) + 2.0 * TAG_PADDING;
            let value_offset = vec2(value_size.x, 0.0);
            total_size += value_offset;
        }
        (hanger_size, label_size, value_size, total_size)
    }

    fn galley_pos(&self, bbox: &Rect, galley: &Arc<Galley>, halign: Option<Align>) -> Pos2 {
        let text_rect = bbox.shrink2(TAG_PADDING);
        match halign.unwrap_or(galley.job.halign) {
            Align::LEFT => text_rect.left_top(),
            Align::Center => text_rect.center_top() - galley.rect.center_top().to_vec2(),
            Align::RIGHT => text_rect.right_top() - galley.rect.right_top().to_vec2(),
        }
    }

    pub fn size(&self, ui: &Ui) -> Vec2 {
        let galley = self.galley(ui);
        self.sizes(
            self.line_height(ui),
            &galley,
            &self.value_galley(ui, galley.size().x),
        )
        .3
    }

    fn paint_hanger(&self, ui: &Ui, p: &Painter, bbox: Rect, bg: Color32, fg: Color32) {
        match self.definition.field_type {
            FieldType::Tag => {
                let rect_width =
                    FRAC_1_SQRT_2 * bbox.size().y + 2.0 * (1.0 - FRAC_1_SQRT_2) * BORDER_RADIUS;
                let rect = Rect::from_center_size(bbox.right_center(), Vec2::splat(rect_width));
                let shape = epaint::RectShape::filled(rect, Rounding::same(BORDER_RADIUS), bg);
                let prim = ui
                    .ctx()
                    .tessellate(
                        vec![ClippedShape {
                            shape: shape.into(),
                            clip_rect: rect,
                        }],
                        ui.ctx().pixels_per_point(),
                    )
                    .swap_remove(0);
                let mut mesh = match prim.primitive {
                    Primitive::Mesh(m) => m,
                    Primitive::Callback(_) => {
                        return;
                    }
                };
                mesh.rotate(
                    Rot2::from_angle(std::f32::consts::FRAC_PI_4),
                    bbox.right_center(),
                );
                p.add(mesh);

                // hanger ring
                p.add(epaint::CircleShape::filled(
                    bbox.left_center() + vec2((SQRT_2 + 0.5) * BORDER_RADIUS, 0.0),
                    BORDER_RADIUS / 2.0,
                    fg,
                ));
            }
            FieldType::Container => {}
            typ => {
                let (text, (r, g, b)) = match typ {
                    FieldType::Tag | FieldType::Container => unreachable!(),
                    FieldType::Boolean => ("?", (255, 255, 0)),
                    FieldType::Int => ("#", (0, 127, 255)),
                    FieldType::Float => ("%", (0, 255, 0)),
                    FieldType::String => ("$", (255, 0, 0)),
                    FieldType::ItemRef => ("&", (128, 0, 0)),
                    FieldType::List => ("[]", (255, 127, 0)),
                    FieldType::Colour => ("\u{1f308}", (127, 127, 255)),
                    FieldType::Dictionary => ("{}", (255, 0, 255)),
                    FieldType::DateTime => ("\u{23f0}", (0, 204, 255)),
                };

                let bg = Color32::from_rgb(r, g, b);
                let galley = self.text_to_galley(ui, text.into(), f32::INFINITY);

                p.add(epaint::RectShape::filled(
                    bbox,
                    Rounding {
                        ne: 0.0,
                        se: 0.0,
                        nw: BORDER_RADIUS,
                        sw: BORDER_RADIUS,
                    },
                    bg,
                ));
                p.add(epaint::TextShape::new(
                    self.galley_pos(&bbox, &galley, Some(Align::Center)),
                    galley,
                    theme::WHITE_TEXT,
                ));
            }
        }
    }

    pub fn paint(&self, ui: &Ui, clip_rect: Rect, loc: Pos2, response: &Option<Response>) {
        let p = ui.painter_at(clip_rect.expand(SELECTED_STROKE_WIDTH));
        let galley = self.galley(ui);
        let value_galley = self.value_galley(ui, galley.size().x);
        let has_value = value_galley.is_some();
        let (hanger_size, label_size, value_size, total_size) =
            self.sizes(self.line_height(ui), &galley, &value_galley);

        let visuals = ui.style().visuals.widgets.inactive;
        let mut bg = self
            .definition
            .get_known_field_value(fields::meta::COLOUR)
            .ok()
            .flatten()
            .map_or(visuals.bg_fill, |c| c.into());

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

        let value_bg = match self.value {
            Some(FieldValue::Colour(sc)) => (*sc).into(),
            _ => bg.gamma_multiply(0.5).to_opaque(),
        };

        let fg = if Hsva::from(bg).v > 0.5 {
            theme::BLACK_TEXT
        } else {
            theme::WHITE_TEXT
        };

        // hanger

        let hanger_bbox = Rect::from_min_size(loc, hanger_size);
        self.paint_hanger(ui, &p, hanger_bbox, bg, fg);

        // label

        let mut label_rounding = Rounding {
            nw: 0.0,
            sw: 0.0,
            ne: BORDER_RADIUS,
            se: BORDER_RADIUS,
        };

        if self.definition.field_type == FieldType::Container {
            label_rounding.nw = BORDER_RADIUS;
            label_rounding.sw = BORDER_RADIUS;
        }

        if has_value {
            label_rounding.ne = 0.0;
            label_rounding.se = 0.0;
        }

        let label_offset = vec2(total_size.x - label_size.x - value_size.x, 0.0);
        let label_bbox = Rect::from_min_size(loc + label_offset, label_size);
        p.add(epaint::RectShape::filled(label_bbox, label_rounding, bg));

        p.add(epaint::TextShape::new(
            self.galley_pos(&label_bbox, &galley, None),
            galley,
            fg,
        ));

        // value
        if let Some(value_galley) = value_galley {
            let value_offset = vec2(total_size.x - value_size.x, 0.0);
            let value_bbox = Rect::from_min_size(loc + value_offset, value_size);

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
            p.add(epaint::TextShape::new(
                self.galley_pos(&value_bbox, &value_galley, None),
                value_galley,
                fg,
            ));
        }

        // selected outline
        if self.selected {
            let total_rect = Rect::from_min_size(loc, total_size);
            p.add(epaint::RectShape::stroke(
                total_rect,
                Rounding::same(BORDER_RADIUS),
                Stroke::new(SELECTED_STROKE_WIDTH, SELECTED_COLOUR),
            ));
        }
    }
}

impl<'a> Widget for Tag<'a> {
    fn ui(self, ui: &mut Ui) -> Response {
        let galley = self.galley(ui);
        let value_galley = self.value_galley(ui, galley.size().x);

        let (_, _, _, total_size) = self.sizes(self.line_height(ui), &galley, &value_galley);

        let (rect, res) = ui.allocate_exact_size(total_size, Sense::click_and_drag());

        res.widget_info(|| WidgetInfo::labeled(WidgetType::Label, galley.text()));

        if ui.is_rect_visible(res.rect) {
            self.paint(ui, rect, rect.min, &Some(res.clone()));
        }

        res
    }
}
