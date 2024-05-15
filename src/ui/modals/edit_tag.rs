use eframe::egui;
use eframe::egui::{Color32, Widget};
use uuid::Uuid;

use crate::data::{kind, FieldDefinition, FieldStore, FieldType, FieldValue};
use crate::fields;
use crate::state::AppStateRef;
use crate::ui::modals::AppModal;
use crate::ui::widgets;
use crate::ui::widgets::ListEditResult;

#[derive(Default)]
pub struct EditTagDialog {
    definition: Option<FieldDefinition>,
    error_message: Option<String>,
    is_new: bool,
    is_ready: bool,

    removed_parents: Vec<Uuid>,
    removed_children: Vec<Uuid>,

    opened: bool,
}

#[derive(Clone)]
struct State {
    selected_tag_ids: Vec<Uuid>,
    opened: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            selected_tag_ids: Default::default(),
            opened: true,
        }
    }
}

impl State {
    fn load(ctx: &egui::Context, id: egui::Id) -> Option<Self> {
        ctx.data(|r| r.get_temp(id))
    }

    fn store(self, ctx: &egui::Context, id: egui::Id) {
        ctx.data_mut(|wr| wr.insert_temp(id, self));
    }

    fn dispose(ctx: &egui::Context, id: egui::Id) {
        ctx.data_mut(|wr| wr.remove_temp::<Self>(id));
    }
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

impl EditTagDialog {
    fn id() -> egui::Id {
        "edit_tag_window".into()
    }

    //noinspection DuplicatedCode
    fn edit_ui(&mut self, ui: &mut egui::Ui, state: AppStateRef) {
        let Some(def) = self.definition.as_mut() else {
            return;
        };

        let r = state.blocking_read();
        let Ok(vault) = r.catch(|| r.current_vault()) else {
            return;
        };

        ui.heading("Preview");

        ui.add(widgets::Tag::new(def));

        ui.separator();

        ui.heading("Edit properties");

        egui_extras::TableBuilder::new(ui)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(egui_extras::Column::auto())
            .column(egui_extras::Column::remainder())
            .auto_shrink([false, true])
            .vscroll(false)
            .body(|mut body| {
                body.row(24.0, |mut row| {
                    row.col(|ui| {
                        ui.label("Name: ");
                    });
                    row.col(|ui| {
                        ui.text_edit_singleline(&mut def.name);
                    });
                });
                body.row(24.0, |mut row| {
                    row.col(|ui| {
                        ui.label("Type: ");
                    });
                    row.col(|ui| {
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
                });
                body.row(24.0, |mut row| {
                    row.col(|ui| {
                        ui.label("Colour: ");
                    });
                    row.col(|ui| {
                        let visuals = ui.style().visuals.widgets.inactive;
                        let r = state.blocking_read();
                        let Ok(mut colour) = r.catch(|| {
                            def.get_or_insert_known_field_value(
                                fields::meta::COLOUR,
                                visuals.bg_fill.into(),
                            )
                        }) else {
                            return;
                        };
                        ui.color_edit_button_srgb(colour.as_mut_slice());
                        def.set_known_field_value(fields::meta::COLOUR, colour);
                    });
                });
            });

        let parent_ids: Vec<Uuid> = def.iter_parent_ids().map(|u| *u).collect();
        let child_ids: Vec<Uuid> = def.iter_child_ids().map(|u| *u).collect();
        let mut exclude_ids = vec![def.id];
        exclude_ids.extend(&parent_ids);
        exclude_ids.extend(&child_ids);

        ui.group(|ui| {
            let mut result = ListEditResult::None;
            widgets::ListEdit::new("edit_tag_parent_table", &parent_ids, &mut result)
                .row_height(22.0)
                .header_label("Parents:".into())
                .item_ui(|ui, parent_id| {
                    let Some(parent_def) = vault.get_definition(parent_id) else {
                        return;
                    };
                    ui.add(widgets::Tag::new(&parent_def));
                })
                .create_label("Add parent".into())
                .create_ui(|mut row, create_state| {
                    let mut res = None;
                    row.col(|ui| {
                        if ui
                            .add(
                                widgets::FindTag::new(
                                    "edit_tag_new_parent",
                                    create_state,
                                    state.clone(),
                                )
                                .exclude_ids(&exclude_ids)
                                .filter_types(&[kind::KindType::Container]),
                            )
                            .changed()
                            && create_state.is_some()
                        {
                            res = Some(create_state.unwrap());
                        }
                    });
                    row.col(|ui| {
                        ui.label("");
                    });
                    res
                })
                .ui(ui);

            match result {
                ListEditResult::None => {}
                ListEditResult::Add(id) => def.add_parent(id),
                ListEditResult::Remove(id) => {
                    def.remove_parent(*id);
                    self.removed_parents.push(*id);
                }
            }
        });

        ui.group(|ui| {
            let mut result = ListEditResult::None;
            widgets::ListEdit::new("edit_tag_child_table", &child_ids, &mut result)
                .row_height(22.0)
                .header_label("Children:".into())
                .item_ui(|ui, child_id| {
                    let Some(child_def) = vault.get_definition(child_id) else {
                        return;
                    };
                    ui.add(widgets::Tag::new(&child_def));
                })
                .create_label("Add child".into())
                .create_ui(|mut row, create_state| {
                    let mut res = None;
                    row.col(|ui| {
                        if ui
                            .add(
                                widgets::FindTag::new(
                                    "edit_tag_new_child",
                                    create_state,
                                    state.clone(),
                                )
                                .exclude_ids(&exclude_ids),
                            )
                            .changed()
                            && create_state.is_some()
                        {
                            res = Some(create_state.unwrap());
                        }
                    });
                    row.col(|ui| {
                        ui.label("");
                    });
                    res
                })
                .ui(ui);

            match result {
                ListEditResult::None => {}
                ListEditResult::Add(id) => def.add_child(id),
                ListEditResult::Remove(id) => {
                    def.remove_child(*id);
                    self.removed_children.push(*id);
                }
            }
        });

        ui.group(|ui| {
            let Ok(aliases) = r.catch::<Vec<_>, anyhow::Error>(|| {
                Ok(def
                    .get_or_insert_known_field_value(fields::meta::ALIASES, vec![])?
                    .into_iter()
                    .filter_map(|v| v.as_str_opt().map(|s| s.to_string()))
                    .collect())
            }) else {
                return;
            };

            let mut result = ListEditResult::None;
            widgets::ListEdit::new("edit_tag_alias_table", &aliases, &mut result)
                .header_label("Aliases:".into())
                .item_ui(|ui, alias| {
                    ui.label(alias);
                })
                .create_label("Add alias".into())
                .create_ui(|mut row, create_state: &mut String| {
                    let mut res = None;
                    row.col(|ui| {
                        ui.text_edit_singleline(create_state);
                    });
                    row.col(|ui| {
                        if ui.add(egui::Button::new("\u{2714}").frame(false)).clicked()
                            && !create_state.is_empty()
                        {
                            res = Some(create_state.clone());
                        }
                    });
                    res
                })
                .ui(ui);

            let mut removed_alias = None;
            let mut added_alias = None;
            match result {
                ListEditResult::None => {}
                ListEditResult::Add(alias) => added_alias = Some(alias),
                ListEditResult::Remove(alias) => removed_alias = Some(alias.clone()),
            };

            if added_alias.is_some() || removed_alias.is_some() {
                let mut new_aliases = vec![];
                for alias in aliases {
                    if Some(&alias) == removed_alias.as_ref() {
                        new_aliases.push(FieldValue::from(kind::Str::from(alias)));
                    }
                }
                if let Some(alias) = added_alias {
                    new_aliases.push(FieldValue::from(kind::Str::from(alias)));
                }

                def.set_known_field_value(fields::meta::ALIASES, new_aliases);
            }
        });
    }
}

impl AppModal for EditTagDialog {
    fn update(&mut self, ctx: &egui::Context, app_state: AppStateRef) -> &mut dyn AppModal {
        let mut widget_state = State::load(ctx, Self::id()).unwrap_or_default();
        egui::Window::new("Edit tags")
            .id("edit_tag_window".into())
            .open(&mut widget_state.opened)
            .show(ctx, |ui| {
                egui::SidePanel::left("edit_tag_window_left").show_inside(ui, |ui| {
                    ui.vertical(|ui| {
                        ui.add(widgets::TagTree::new(
                            "edit_tag_tree",
                            &mut widget_state.selected_tag_ids,
                            app_state.clone(),
                        ));

                        if widget_state.selected_tag_ids.first()
                            != self.definition.as_ref().map(|d| &d.id)
                        {
                            if let Some(id) = widget_state.selected_tag_ids.first() {
                                let r = app_state.blocking_read();
                                let Ok(vault) = r.catch(|| r.current_vault()) else {
                                    return;
                                };
                                self.definition = vault.get_definition(id).map(|d| d.clone());
                            }
                        }
                    });
                });
                egui::TopBottomPanel::bottom("edit_tag_window_bottom").show_inside(ui, |ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.button("Save");
                        ui.button("Reset");
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            ui.button("Delete");
                        });
                    });
                });

                egui::CentralPanel::default().show_inside(ui, |ui| {
                    if self.definition.is_some() {
                        self.edit_ui(ui, app_state);
                    }

                    if let Some(msg) = &self.error_message {
                        ui.colored_label(Color32::RED, msg);
                    }
                });
            });

        self.opened = widget_state.opened;
        widget_state.store(ctx, Self::id());
        self
    }

    fn dispose(&mut self, ctx: &egui::Context, _state: AppStateRef) {
        State::dispose(ctx, Self::id());
    }

    fn is_open(&self) -> bool {
        self.opened
    }
}
