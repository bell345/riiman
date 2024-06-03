use std::str::FromStr;

use chrono::{NaiveTime, TimeZone, Utc};
use eframe::egui;
use eframe::egui::{Response, Ui, Widget};
use ordered_float::OrderedFloat;

use crate::data::kind::KindType;
use crate::data::{kind, FieldType, FieldValue};
use crate::ui::cloneable_state::CloneableTempState;

pub struct TagValueEdit<'a> {
    id: egui::Id,
    field_type: FieldType,
    value: &'a mut Option<FieldValue>,
}

impl<'a> TagValueEdit<'a> {
    pub fn new(
        id: impl std::hash::Hash,
        field_type: FieldType,
        value: &'a mut Option<FieldValue>,
    ) -> Self {
        Self {
            id: egui::Id::new(id),
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

impl<'a> Widget for TagValueEdit<'a> {
    fn ui(self, ui: &mut Ui) -> Response {
        let mut res = ui.label("");
        let mut state = State::load(ui.ctx(), self.id).unwrap_or_default();

        match self.field_type {
            KindType::Tag => {
                *self.value = Some(FieldValue::Tag);
            }
            KindType::Container => {
                *self.value = Some(FieldValue::Container);
            }
            KindType::Boolean => {
                let mut b = match self.value {
                    Some(FieldValue::Boolean(b)) => *b,
                    _ => false,
                };

                res = ui.checkbox(&mut b, "");

                *self.value = Some(FieldValue::Boolean(b));
            }
            KindType::Int => {
                if let Some(i) = match self.value {
                    Some(FieldValue::Int(i)) => Some(*i),
                    Some(FieldValue::UInt(u)) => {
                        Some(i64::try_from(*u).unwrap_or(if *u > 0 { i64::MAX } else { i64::MIN }))
                    }
                    Some(FieldValue::Float(OrderedFloat(f))) => Some(*f as i64),
                    Some(FieldValue::String(s)) => i64::from_str(s).ok(),
                    _ => None,
                } {
                    state.typed_value = i.to_string();
                }

                res = ui.text_edit_singleline(&mut state.typed_value);

                *self.value = i64::from_str(state.typed_value.as_str())
                    .ok()
                    .map(FieldValue::Int);
            }
            KindType::UInt => {
                if let Some(u) = match self.value {
                    Some(FieldValue::Int(i)) => Some(u64::try_from(*i).unwrap_or(0)),
                    Some(FieldValue::UInt(u)) => Some(*u),
                    Some(FieldValue::Float(OrderedFloat(f))) => Some(*f as u64),
                    Some(FieldValue::String(s)) => u64::from_str(s).ok(),
                    _ => None,
                } {
                    state.typed_value = u.to_string();
                }

                res = ui.text_edit_singleline(&mut state.typed_value);

                *self.value = u64::from_str(state.typed_value.as_str())
                    .ok()
                    .map(FieldValue::UInt);
            }
            KindType::Float => {
                if let Some(f) = match self.value {
                    Some(FieldValue::Int(i)) => Some(*i as f64),
                    Some(FieldValue::UInt(u)) => Some(*u as f64),
                    Some(FieldValue::Float(OrderedFloat(f))) => Some(*f),
                    Some(FieldValue::String(s)) => f64::from_str(s).ok(),
                    _ => None,
                } {
                    state.typed_value = f.to_string();
                }

                res = ui.text_edit_singleline(&mut state.typed_value);

                *self.value = if state.typed_value.ends_with('.') {
                    None
                } else {
                    f64::from_str(state.typed_value.as_str())
                        .ok()
                        .map(|f| FieldValue::Float(OrderedFloat(f)))
                }
            }
            KindType::String | KindType::ItemRef => {
                if let Some(s) = match self.value {
                    Some(FieldValue::Int(i)) => Some(i.to_string()),
                    Some(FieldValue::UInt(u)) => Some(u.to_string()),
                    Some(FieldValue::Float(OrderedFloat(f))) => Some(f.to_string()),
                    Some(FieldValue::String(s)) => Some(s.to_string()),
                    Some(FieldValue::ItemRef(r)) => Some(format!("{}:{}", r.0, r.1)),
                    _ => None,
                } {
                    state.typed_value = s;
                }

                res = ui.text_edit_singleline(&mut state.typed_value);
                *self.value = Some(match self.field_type {
                    KindType::String => FieldValue::String(state.typed_value.to_string().into()),
                    KindType::ItemRef => state
                        .typed_value
                        .to_string()
                        .parse::<kind::ItemRef>()
                        .unwrap_or(kind::ItemRef::from((
                            "".to_string().into(),
                            "".to_string().into(),
                        )))
                        .into(),
                    _ => unreachable!(),
                });
            }
            KindType::List => {
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
                ui.label("Unimplemented");
            }
            KindType::Colour => {
                let mut rgb = match self.value {
                    Some(FieldValue::Colour(sc)) => sc.0,
                    _ => [0, 0, 0],
                };

                res = ui.color_edit_button_srgb(&mut rgb);

                *self.value = Some(FieldValue::Colour(rgb.into()));
            }
            KindType::Dictionary => {
                // TODO
                ui.label("Unimplemented");
            }
            KindType::DateTime => {
                let dt = match self.value {
                    Some(FieldValue::DateTime(dt)) => *dt,
                    _ => Utc::now(),
                };

                let naive = dt.naive_local();
                if state.typed_value.is_empty() {
                    state.typed_value = naive.time().format("%H:%M:%S").to_string();
                }

                let mut date = naive.date();
                res = ui
                    .horizontal(|ui| {
                        ui.add(egui_extras::DatePickerButton::new(&mut date));
                        ui.text_edit_singleline(&mut state.typed_value);
                    })
                    .response;

                *self.value = Some(FieldValue::DateTime(
                    if let Ok(time) =
                        NaiveTime::parse_from_str(state.typed_value.as_str(), "%H:%M:%S")
                    {
                        Utc.from_local_datetime(&date.and_time(time)).unwrap()
                    } else {
                        Utc.from_local_datetime(&date.and_time(naive.time()))
                            .unwrap()
                    },
                ));
            }
        };

        state.store(ui.ctx(), self.id);

        res
    }
}
