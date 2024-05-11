use crate::data::{kind, FieldDefinition, FieldStore, FieldType, FieldValue};
use crate::fields;
use crate::state::AppStateRef;
use crate::ui::modals::AppModal;
use crate::ui::widgets;
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

    adding_parent: bool,
    new_parent_id: Option<Uuid>,
    removed_parents: Vec<Uuid>,

    adding_child: bool,
    new_child_id: Option<Uuid>,
    removed_children: Vec<Uuid>,

    adding_alias: bool,
    new_alias: String,

    tag_name: String,
    opened: bool,
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
    fn update(&mut self, ctx: &egui::Context, state: AppStateRef) -> &mut dyn AppModal {
        let modal = Modal::new(ctx, "edit_tag_modal");

        modal.show(|ui| {
            modal.title(ui, if self.is_new { "New tag" } else { "Edit tag" });
            modal.frame(ui, |ui| {
                ui.heading("Choose a tag");
                ui.horizontal(|ui| {
                    ui.label("Search for tag: ");
                    let mut tag_id = self.definition.as_ref().map(|def| def.id);
                    if ui
                        .add(widgets::FindTag::new(
                            "edit_tag_find",
                            &mut tag_id,
                            state.clone(),
                        ))
                        .changed()
                    {
                        if let Some(tag_id) = tag_id {
                            let r = state.blocking_read();
                            let Ok(vault) = r.catch(|| r.current_vault()) else {
                                return;
                            };
                            let Some(def) = vault.get_definition(&tag_id) else {
                                return;
                            };
                            self.definition = Some(def.clone());
                            self.removed_children = vec![];
                            self.removed_parents = vec![];
                        } else {
                            self.definition = None;
                        }
                    }
                });

                ui.separator();

                if let Some(def) = self.definition.as_mut() {
                    ui.heading("Preview");

                    ui.add(widgets::Tag::new(def.clone()));

                    ui.separator();

                    ui.heading("Edit properties");

                    ui.horizontal(|ui| {
                        ui.label("Name: ");
                        ui.text_edit_singleline(&mut def.name);
                    });

                    ui.horizontal(|ui| {
                        ui.label("Type: ");
                        egui::ComboBox::new("edit_tag_type_box", "")
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

                    ui.group(|ui| {
                        ui.label("Parents:");

                        egui::Grid::new("edit_tag_parent_grid")
                            .num_columns(2)
                            .spacing([40.0, 4.0])
                            .striped(true)
                            .show(ui, |ui| {
                                let parent_ids: Vec<Uuid> =
                                    def.iter_parent_ids().map(|u| *u).collect();

                                let r = state.blocking_read();
                                let Ok(vault) = r.catch(|| r.current_vault()) else {
                                    return;
                                };

                                for parent_def in vault.resolve_field_defs(parent_ids.iter()) {
                                    let id = (*parent_def).id;
                                    if ui.add(egui::Button::new("\u{274c}").frame(false)).clicked()
                                    {
                                        def.remove_parent(id);
                                        self.removed_parents.push(id);
                                    }
                                    ui.add(widgets::Tag::new(parent_def.clone()));
                                    ui.end_row();
                                }

                                if self.adding_parent {
                                    if ui.add(egui::Button::new("\u{2714}").frame(false)).clicked()
                                        && self.new_parent_id.is_some()
                                    {
                                        def.add_parent(self.new_parent_id.unwrap());
                                    }
                                    ui.add(widgets::FindTag::new(
                                        "edit_tag_new_parent",
                                        &mut self.new_parent_id,
                                        state.clone(),
                                    ));
                                    ui.end_row();
                                }
                            });

                        if ui.button("Add parent").clicked() {
                            self.adding_parent = true;
                            self.new_parent_id = None;
                        }
                    });

                    ui.group(|ui| {
                        ui.label("Children:");

                        egui::Grid::new("edit_tag_child_grid")
                            .num_columns(2)
                            .spacing([40.0, 4.0])
                            .striped(true)
                            .show(ui, |ui| {
                                let child_ids: Vec<Uuid> =
                                    def.iter_child_ids().map(|u| *u).collect();

                                let r = state.blocking_read();
                                let Ok(vault) = r.catch(|| r.current_vault()) else {
                                    return;
                                };

                                for child_def in vault.resolve_field_defs(child_ids.iter()) {
                                    let id = (*child_def).id;
                                    if ui.add(egui::Button::new("\u{274c}").frame(false)).clicked()
                                    {
                                        def.remove_child(id);
                                        self.removed_children.push(id);
                                    }
                                    ui.add(widgets::Tag::new(child_def.clone()));
                                    ui.end_row();
                                }

                                if self.adding_child {
                                    if ui.add(egui::Button::new("\u{2714}").frame(false)).clicked()
                                        && self.new_child_id.is_some()
                                    {
                                        def.add_child(self.new_child_id.unwrap());
                                    }
                                    ui.add(widgets::FindTag::new(
                                        "edit_tag_new_child",
                                        &mut self.new_child_id,
                                        state.clone(),
                                    ));
                                    ui.end_row();
                                }
                            });

                        if ui.button("Add child").clicked() {
                            self.adding_child = true;
                            self.new_child_id = None;
                        }
                    });

                    ui.group(|ui| {
                        ui.label("Aliases:");

                        egui::Grid::new("edit_tag_alias_grid")
                            .num_columns(2)
                            .spacing([40.0, 4.0])
                            .striped(true)
                            .show(ui, |ui| {
                                let r = state.blocking_read();

                                let Ok(aliases) = r.catch::<Vec<_>, anyhow::Error>(|| {
                                    Ok(def
                                        .get_or_insert_known_field_value(
                                            fields::meta::ALIASES,
                                            vec![],
                                        )?
                                        .into_iter()
                                        .filter_map(|v| v.as_str_opt().map(|s| s.to_string()))
                                        .collect())
                                }) else {
                                    return;
                                };

                                let mut removed_aliases = vec![];
                                for alias in aliases.iter() {
                                    ui.label(alias);
                                    if ui.add(egui::Button::new("\u{274c}").frame(false)).clicked()
                                    {
                                        removed_aliases.push(alias.clone());
                                    }
                                    ui.end_row();
                                }

                                let mut added_alias = None;
                                if self.adding_alias {
                                    ui.text_edit_singleline(&mut self.new_alias);
                                    if ui.add(egui::Button::new("\u{2714}").frame(false)).clicked()
                                        && !self.new_alias.as_str().is_empty()
                                    {
                                        added_alias = Some(self.new_alias.clone());
                                    }
                                    ui.end_row();
                                }

                                if added_alias.is_some() || !removed_aliases.is_empty() {
                                    let mut new_aliases = vec![];
                                    for alias in aliases {
                                        if !removed_aliases.contains(&alias) {
                                            new_aliases.push(FieldValue::from(kind::Str::from(
                                                alias.to_string(),
                                            )));
                                        }
                                    }
                                    if let Some(alias) = added_alias {
                                        new_aliases.push(FieldValue::from(kind::Str::from(alias)));
                                    }

                                    def.set_known_field_value(fields::meta::ALIASES, new_aliases);
                                }
                            });

                        if ui.button("Add child").clicked() {
                            self.adding_child = true;
                        }
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

        if !self.opened {
            modal.open();
            self.opened = true;
        }

        self.modal = Some(modal);
        self
    }

    fn is_open(&self) -> bool {
        self.modal.as_ref().is_some_and(|m| m.is_open())
    }
}
