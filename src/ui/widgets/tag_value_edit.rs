use std::str::FromStr;

use chrono::{NaiveTime, TimeZone, Utc};
use eframe::egui;
use eframe::egui::{Response, Ui, Widget};
use ordered_float::OrderedFloat;

use crate::data::{kind, FieldType, FieldValue};
use crate::ui::cloneable_state::CloneableTempState;

pub struct TagValueEdit<'a> {
    id: egui::Id,
    state: State,
    field_type: FieldType,
    value: &'a mut Option<FieldValue>,
}

impl<'a> TagValueEdit<'a> {
    pub fn new(
        id: impl std::hash::Hash,
        field_type: FieldType,
        value: &'a mut Option<FieldValue>,
    ) -> Self {
        #[allow(clippy::default_trait_access)]
        Self {
            id: egui::Id::new(id),
            state: Default::default(),
            field_type,
            value,
        }
    }
}

#[derive(Debug, Clone, Default)]
struct State {
    typed_value: String,
}

impl CloneableTempState for State {}

impl<'a> TagValueEdit<'a> {
    fn tag_ui(&mut self) {
        *self.value = Some(FieldValue::Tag);
    }

    fn container_ui(&mut self) {
        *self.value = Some(FieldValue::Container);
    }

    fn boolean_ui(&mut self, ui: &mut Ui) -> Response {
        let mut b = match self.value {
            Some(FieldValue::Boolean(b)) => *b,
            _ => false,
        };

        let res = ui.checkbox(&mut b, "");

        *self.value = Some(FieldValue::Boolean(b));

        res
    }

    fn int_ui(&mut self, ui: &mut Ui) -> Response {
        if let Some(i) = match self.value {
            Some(FieldValue::Int(i)) => Some(*i),
            #[allow(clippy::cast_possible_truncation)]
            Some(FieldValue::Float(OrderedFloat(f))) => Some(*f as i64),
            Some(FieldValue::String(s)) => i64::from_str(s).ok(),
            _ => None,
        } {
            self.state.typed_value = i.to_string();
        }

        let res = ui.text_edit_singleline(&mut self.state.typed_value);

        *self.value = i64::from_str(self.state.typed_value.as_str())
            .ok()
            .map(FieldValue::Int);

        res
    }

    fn float_ui(&mut self, ui: &mut Ui) -> Response {
        if let Some(f) = match self.value {
            #[allow(clippy::cast_precision_loss)]
            Some(FieldValue::Int(i)) => Some(*i as f64),
            Some(FieldValue::Float(OrderedFloat(f))) => Some(*f),
            Some(FieldValue::String(s)) => f64::from_str(s).ok(),
            _ => None,
        } {
            self.state.typed_value = f.to_string();
        }

        let res = ui.text_edit_singleline(&mut self.state.typed_value);

        *self.value = if self.state.typed_value.ends_with('.') {
            None
        } else {
            f64::from_str(self.state.typed_value.as_str())
                .ok()
                .map(|f| FieldValue::Float(OrderedFloat(f)))
        };

        res
    }

    fn string_ui(&mut self, ui: &mut Ui) -> Response {
        if let Some(s) = match self.value {
            Some(FieldValue::Int(i)) => Some(i.to_string()),
            Some(FieldValue::Float(OrderedFloat(f))) => Some(f.to_string()),
            Some(FieldValue::String(s)) => Some(s.to_string()),
            Some(FieldValue::ItemRef(r)) => Some(format!("{}:{}", r.0, r.1)),
            _ => None,
        } {
            self.state.typed_value = s;
        }

        let res = ui.text_edit_singleline(&mut self.state.typed_value);
        *self.value = Some(match self.field_type {
            FieldType::String => FieldValue::String(self.state.typed_value.to_string().into()),
            FieldType::ItemRef => self
                .state
                .typed_value
                .to_string()
                .parse::<kind::ItemRef>()
                .unwrap_or(kind::ItemRef::from((
                    String::new().into(),
                    String::new().into(),
                )))
                .into(),
            _ => unreachable!(),
        });

        res
    }

    fn list_ui(&mut self, ui: &mut Ui) -> Response {
        /*let mut l = match self.value {
            Some(FieldValue::List(l)) => l.clone(),
            Some(FieldValue::Dictionary(d)) => d.iter().map(|(_, v)| v.clone()).collect(),
            Some(v) => vec![v.clone()],
            _ => vec![],
        };

        if let Some(v) = l.first() {
            state.list_field_type = v.get_type();
        }

        if state.list_field_type == FieldType::Tag {
            state.list_field_type = FieldType::Str;
        }

        res = ui
            .vertical(|ui| {
                ui.add(
                    widgets::TagTypeCombo::new(
                        self.id.with("list_tag_type"),
                        &mut state.list_field_type,
                    )
                    .exclude_types(&[FieldType::Tag, FieldType::Container]),
                );

                let mut result = Default::default();
                ui.add(
                    widgets::ListEdit::new(self.id.with("list_edit"), &l, &mut result)
                        .item_ui(|ui, item| {
                            let mut value = Some(item.clone());
                            ui.add(TagValueEdit::new(
                                ui.id(),
                                state.list_field_type,
                                &mut value,
                            ));
                            value
                        })
                        .create_label("Add".into())
                        .create_ui(|ui, create_state| {
                            ui.add(TagValueEdit::new(
                                ui.id(),
                                state.list_field_type,
                                create_state,
                            ));
                            if ui.add(egui::Button::new("\u{2714}").frame(false)).clicked()
                                && create_state.is_some()
                            {
                                Some(create_state.as_ref().unwrap().clone())
                            } else {
                                None
                            }
                        }),
                );

                match result {
                    ListEditResult::None => {}
                    ListEditResult::Add(item) => l.push(item),
                    ListEditResult::Remove(i) => {
                        l.remove(i);
                    }
                    ListEditResult::Edit(i, new_item) => l.insert(i, new_item),
                }
            })
            .response;

        *self.value = Some(FieldValue::List(l));*/
        // TODO needs fixing
        ui.label("Unimplemented")
    }

    fn colour_ui(&mut self, ui: &mut Ui) -> Response {
        let mut rgb = match self.value {
            Some(FieldValue::Colour(sc)) => sc.0,
            _ => [0, 0, 0],
        };

        let res = ui.color_edit_button_srgb(&mut rgb);

        *self.value = Some(FieldValue::Colour(rgb.into()));

        res
    }

    fn dictionary_ui(&mut self, ui: &mut Ui) -> Response {
        // TODO
        ui.label("Unimplemented")
    }

    fn datetime_ui(&mut self, ui: &mut Ui) -> Response {
        let dt = match self.value {
            Some(FieldValue::DateTime(dt)) => *dt,
            _ => Utc::now(),
        };

        let naive = dt.naive_local();
        if self.state.typed_value.is_empty() {
            self.state.typed_value = naive.time().format("%H:%M:%S").to_string();
        }

        let mut date = naive.date();
        let res = ui
            .horizontal(|ui| {
                ui.add(egui_extras::DatePickerButton::new(&mut date));
                ui.text_edit_singleline(&mut self.state.typed_value);
            })
            .response;

        *self.value = Some(FieldValue::DateTime(
            if let Ok(time) = NaiveTime::parse_from_str(self.state.typed_value.as_str(), "%H:%M:%S")
            {
                Utc.from_local_datetime(&date.and_time(time)).unwrap()
            } else {
                Utc.from_local_datetime(&date.and_time(naive.time()))
                    .unwrap()
            },
        ));

        res
    }
}

impl<'a> Widget for TagValueEdit<'a> {
    fn ui(mut self, ui: &mut Ui) -> Response {
        self.state = State::load(ui.ctx(), self.id).unwrap_or_default();
        let label_res = ui.label("");

        let res = match self.field_type {
            FieldType::Tag => {
                self.tag_ui();
                label_res
            }
            FieldType::Container => {
                self.container_ui();
                label_res
            }
            FieldType::Boolean => self.boolean_ui(ui),
            FieldType::Int => self.int_ui(ui),
            FieldType::Float => self.float_ui(ui),
            FieldType::String | FieldType::ItemRef => self.string_ui(ui),
            FieldType::List => self.list_ui(ui),
            FieldType::Colour => self.colour_ui(ui),
            FieldType::Dictionary => self.dictionary_ui(ui),
            FieldType::DateTime => self.datetime_ui(ui),
        };

        std::mem::take(&mut self.state).store(ui.ctx(), self.id);

        res
    }
}
