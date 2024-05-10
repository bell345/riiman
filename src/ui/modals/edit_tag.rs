use crate::data::{FieldDefinition, FieldStore, FieldType};
use crate::fields;
use crate::state::AppStateRef;
use crate::ui::modals::AppModal;
use eframe::egui;
use eframe::egui::Color32;
use egui_modal::Modal;
use uuid::Uuid;

#[derive(Default)]
pub struct EditTagDialog {
    modal: Option<Modal>,
    definition: Option<FieldDefinition>,
    error_message: Option<String>,
    is_new: bool,
    is_ready: bool,
    cancelled: bool,
    to_delete: bool,

    tag_name: String,
}

impl EditTagDialog {
    pub fn select() -> Self {
        Self {
            definition: None,
            is_new: false,
            ..Default::default()
        }
    }

    pub fn create() -> Self {
        Self {
            definition: Some(FieldDefinition::new(
                Uuid::new_v4(),
                Default::default(),
                Default::default(),
            )),
            is_new: true,
            ..Default::default()
        }
    }

    pub fn edit(definition: FieldDefinition) -> Self {
        Self {
            definition: Some(definition),
            is_new: false,
            ..Default::default()
        }
    }
}

impl AppModal for EditTagDialog {
    fn update(&mut self, ctx: &eframe::egui::Context, state: AppStateRef) -> &mut dyn AppModal {
        let modal = Modal::new(ctx, "edit_tag_modal");

        modal.show(|ui| {
            modal.title(ui, if self.is_new { "New tag" } else { "Edit tag" });
            modal.frame(ui, |ui| {
                ui.heading("Choose a tag");
                ui.horizontal(|ui| {
                    ui.label("Search for tag: ");
                    ui.text_edit_singleline(&mut self.tag_name);
                });

                ui.separator();

                if let Some(def) = self.definition.as_mut() {
                    ui.heading("Preview");

                    ui.label("Preview goes here");

                    ui.separator();

                    ui.heading("Edit properties");

                    ui.horizontal(|ui| {
                        ui.label("Name: ");
                        ui.text_edit_singleline(&mut def.name);
                    });

                    ui.horizontal(|ui| {
                        egui::ComboBox::from_label("Type: ")
                            .selected_text(def.field_type.to_string())
                            .show_ui(ui, |ui| {
                                macro_rules! option {
                                    ($name:ident) => {
                                        ui.selectable_value(
                                            &mut def.field_type,
                                            FieldType::$name,
                                            FieldType::$name.to_string(),
                                        );
                                    };
                                }

                                option!(Tag);
                                option!(Boolean);
                                option!(Int);
                                option!(UInt);
                                option!(Float);
                                option!(Colour);
                                option!(Str);
                                option!(ItemRef);
                                option!(List);
                                option!(Dictionary);
                                option!(DateTime);
                            });
                    });

                    ui.horizontal(|ui| {
                        ui.label("Colour: ");

                        let r = state.blocking_read();
                        let Ok(mut colour) = r.catch(|| {
                            def.get_or_insert_known_field_value(
                                fields::meta::COLOUR,
                                [255, 255, 255],
                            )
                        }) else {
                            return;
                        };
                        ui.color_edit_button_srgb(&mut colour);
                        def.set_known_field_value(fields::meta::COLOUR, colour);
                    });

                    if let Some(msg) = &self.error_message {
                        ui.colored_label(Color32::RED, msg);
                    }
                }
            });
            modal.buttons(ui, |ui| {
                if let Some(def) = self.definition.as_ref() {
                    if modal.suggested_button(ui, "Create").clicked() {
                        if def.name.trim().is_empty() {
                            self.error_message = "Please enter a tag name.".to_string().into();
                            modal.open();
                        } else {
                            self.is_ready = true;
                        }
                    }
                }
                modal.button(ui, "Cancel");
            });
        });

        self.modal = Some(modal);
        self
    }

    fn is_open(&self) -> bool {
        self.modal.as_ref().is_some_and(|m| m.is_open())
    }
}
