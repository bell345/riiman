use crate::data::FieldType;
use crate::ui::choice;
use eframe::egui;
use eframe::egui::{Response, Ui, Widget};

pub struct TagTypeCombo<'a> {
    id: egui::Id,
    field_type: &'a mut FieldType,
    exclude_types: Option<&'a [FieldType]>,
}

impl<'a> TagTypeCombo<'a> {
    pub fn new(id: impl std::hash::Hash, field_type: &'a mut FieldType) -> Self {
        Self {
            id: egui::Id::new(id),
            field_type,
            exclude_types: None,
        }
    }

    pub fn exclude_types(mut self, exclude_types: &'a [FieldType]) -> Self {
        self.exclude_types = Some(exclude_types);
        self
    }
}

impl<'a> Widget for TagTypeCombo<'a> {
    fn ui(self, ui: &mut Ui) -> Response {
        egui::ComboBox::new(self.id, "")
            .selected_text(self.field_type.to_string())
            .show_ui(ui, |ui| {
                macro_rules! option {
                    ($name:ident) => {
                        if !self
                            .exclude_types
                            .is_some_and(|types| types.contains(&FieldType::$name))
                        {
                            choice(ui, self.field_type, FieldType::$name);
                        }
                    };
                }

                option!(Tag);
                option!(Container);
                option!(Boolean);
                option!(Int);
                option!(Float);
                option!(Colour);
                option!(String);
                option!(ItemRef);
                option!(List);
                option!(Dictionary);
                option!(DateTime);
            })
            .response
    }
}
