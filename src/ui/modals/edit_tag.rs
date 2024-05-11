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

const ROW_HEIGHT: f32 = 18.0;

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
                        ui.vertical_centered(|ui| {
                            ui.label("Parents:");
                            ui.push_id("edit_tag_parent_table", |ui| {
                                egui_extras::TableBuilder::new(ui)
                                    .striped(true)
                                    .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                                    .column(egui_extras::Column::remainder())
                                    .column(egui_extras::Column::exact(12.0))
                                    .auto_shrink([false, true])
                                    .vscroll(false)
                                    .body(|mut body| {
                                        let parent_ids: Vec<Uuid> =
                                            def.iter_parent_ids().map(|u| *u).collect();

                                        let r = state.blocking_read();
                                        let Ok(vault) = r.catch(|| r.current_vault()) else {
                                            return;
                                        };

                                        for parent_def in
                                            vault.resolve_field_defs(parent_ids.iter())
                                        {
                                            let id = (*parent_def).id;
                                            body.row(ROW_HEIGHT, |mut row| {
                                                row.col(|ui| {
                                                    ui.add(widgets::Tag::new(parent_def.clone()));
                                                });
                                                row.col(|ui| {
                                                    if ui
                                                        .add(
                                                            egui::Button::new("\u{274c}")
                                                                .frame(false),
                                                        )
                                                        .clicked()
                                                    {
                                                        def.remove_parent(id);
                                                        self.removed_parents.push(id);
                                                    }
                                                });
                                            });
                                        }

                                        if self.adding_parent {
                                            body.row(ROW_HEIGHT, |mut row| {
                                                let mut exclude_ids = parent_ids;
                                                exclude_ids.push(def.id);
                                                row.col(|ui| {
                                                    if ui
                                                        .add(
                                                            widgets::FindTag::new(
                                                                "edit_tag_new_parent",
                                                                &mut self.new_parent_id,
                                                                state.clone(),
                                                            )
                                                            .exclude_ids(exclude_ids),
                                                        )
                                                        .changed()
                                                        && self.new_parent_id.is_some()
                                                    {
                                                        self.adding_parent = false;
                                                        def.add_parent(self.new_parent_id.unwrap());
                                                    }
                                                });
                                                row.col(|ui| {
                                                    ui.label("");
                                                });
                                            });
                                        }
                                    });
                            });

                            if ui.button("Add parent").clicked() {
                                self.adding_parent = true;
                                self.new_parent_id = None;
                            }
                        });
                    });

                    ui.group(|ui| {
                        ui.vertical_centered(|ui| {
                            ui.label("Children:");

                            ui.push_id("edit_tag_child_table", |ui| {
                                egui_extras::TableBuilder::new(ui)
                                    .striped(true)
                                    .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                                    .column(egui_extras::Column::remainder())
                                    .column(egui_extras::Column::exact(12.0))
                                    .auto_shrink([false, true])
                                    .vscroll(false)
                                    .body(|mut body| {
                                        let child_ids: Vec<Uuid> =
                                            def.iter_child_ids().map(|u| *u).collect();

                                        let r = state.blocking_read();
                                        let Ok(vault) = r.catch(|| r.current_vault()) else {
                                            return;
                                        };

                                        for child_def in vault.resolve_field_defs(child_ids.iter())
                                        {
                                            let id = (*child_def).id;
                                            body.row(ROW_HEIGHT, |mut row| {
                                                row.col(|ui| {
                                                    ui.add(widgets::Tag::new(child_def.clone()));
                                                });
                                                row.col(|ui| {
                                                    if ui
                                                        .add(
                                                            egui::Button::new("\u{274c}")
                                                                .frame(false),
                                                        )
                                                        .clicked()
                                                    {
                                                        def.remove_child(id);
                                                        self.removed_children.push(id);
                                                    }
                                                });
                                            });
                                        }

                                        if self.adding_child {
                                            body.row(ROW_HEIGHT, |mut row| {
                                                let mut exclude_ids = child_ids;
                                                exclude_ids.push(def.id);
                                                row.col(|ui| {
                                                    if ui
                                                        .add(
                                                            widgets::FindTag::new(
                                                                "edit_tag_new_child",
                                                                &mut self.new_child_id,
                                                                state.clone(),
                                                            )
                                                            .exclude_ids(exclude_ids),
                                                        )
                                                        .changed()
                                                        && self.new_child_id.is_some()
                                                    {
                                                        self.adding_child = false;
                                                        def.add_child(self.new_child_id.unwrap());
                                                    }
                                                });
                                                row.col(|ui| {
                                                    ui.label("");
                                                });
                                            });
                                        }
                                    });
                            });

                            if ui.button("Add child").clicked() {
                                self.adding_child = true;
                                self.new_child_id = None;
                            }
                        });
                    });

                    ui.group(|ui| {
                        ui.vertical_centered(|ui| {
                            ui.label("Aliases:");

                            ui.push_id("edit_tag_alias_table", |ui| {
                                egui_extras::TableBuilder::new(ui)
                                    .striped(true)
                                    .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                                    .column(egui_extras::Column::remainder())
                                    .column(egui_extras::Column::exact(12.0))
                                    .auto_shrink([false, true])
                                    .vscroll(false)
                                    .body(|mut body| {
                                        let r = state.blocking_read();

                                        let Ok(aliases) = r.catch::<Vec<_>, anyhow::Error>(|| {
                                            Ok(def
                                                .get_or_insert_known_field_value(
                                                    fields::meta::ALIASES,
                                                    vec![],
                                                )?
                                                .into_iter()
                                                .filter_map(|v| {
                                                    v.as_str_opt().map(|s| s.to_string())
                                                })
                                                .collect())
                                        }) else {
                                            return;
                                        };

                                        let mut removed_aliases = vec![];
                                        for alias in aliases.iter() {
                                            body.row(ROW_HEIGHT, |mut row| {
                                                row.col(|ui| {
                                                    ui.label(alias);
                                                });
                                                row.col(|ui| {
                                                    if ui
                                                        .add(
                                                            egui::Button::new("\u{274c}")
                                                                .frame(false),
                                                        )
                                                        .clicked()
                                                    {
                                                        removed_aliases.push(alias.clone());
                                                    }
                                                });
                                            });
                                        }

                                        let mut added_alias = None;
                                        if self.adding_alias {
                                            body.row(ROW_HEIGHT, |mut row| {
                                                row.col(|ui| {
                                                    ui.text_edit_singleline(&mut self.new_alias);
                                                });
                                                row.col(|ui| {
                                                    if ui
                                                        .add(
                                                            egui::Button::new("\u{2714}")
                                                                .frame(false),
                                                        )
                                                        .clicked()
                                                        && !self.new_alias.as_str().is_empty()
                                                    {
                                                        self.adding_alias = false;
                                                        added_alias = Some(self.new_alias.clone());
                                                    }
                                                });
                                            });
                                        }

                                        if added_alias.is_some() || !removed_aliases.is_empty() {
                                            let mut new_aliases = vec![];
                                            for alias in aliases {
                                                if !removed_aliases.contains(&alias) {
                                                    new_aliases.push(FieldValue::from(
                                                        kind::Str::from(alias.to_string()),
                                                    ));
                                                }
                                            }
                                            if let Some(alias) = added_alias {
                                                new_aliases
                                                    .push(FieldValue::from(kind::Str::from(alias)));
                                            }

                                            def.set_known_field_value(
                                                fields::meta::ALIASES,
                                                new_aliases,
                                            );
                                        }
                                    });
                            });

                            if ui.button("Add alias").clicked() {
                                self.adding_alias = true;
                            }
                        });
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
