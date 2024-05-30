use eframe::egui;
use eframe::egui::{Color32, Widget};
use uuid::Uuid;

use crate::data::{FieldDefinition, FieldStore, FieldType, FieldValue};
use crate::state::AppStateRef;
use crate::ui::cloneable_state::CloneableTempState;
use crate::ui::modals::AppModal;
use crate::ui::widgets;
use crate::ui::widgets::ListEditResult;
use crate::{fields, shortcut};

#[derive(Default)]
pub struct EditTagDialog {
    definition: Option<FieldDefinition>,
    error_message: Option<String>,
    is_new: bool,

    removed_parents: Vec<Uuid>,
    removed_children: Vec<Uuid>,

    widget_state: State,
    opened: bool,
    updated: bool,
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

impl CloneableTempState for State {}

impl EditTagDialog {
    pub fn select() -> Self {
        Self {
            ..Default::default()
        }
    }

    pub fn create() -> Self {
        let mut res = Self::default();
        res.new_definition();
        res
    }

    pub fn edit(definition: &FieldDefinition) -> Self {
        let mut res = Self::default();
        res.set_existing_definition(definition);
        res
    }
}

impl EditTagDialog {
    fn set_definition(&mut self, definition: FieldDefinition, is_new: bool) {
        self.definition = Some(definition);
        self.is_new = is_new;
        self.removed_children = vec![];
        self.removed_parents = vec![];
        self.updated = true;

        if is_new {
            self.widget_state.selected_tag_ids = vec![];
        }
    }

    fn new_definition(&mut self) {
        self.set_definition(FieldDefinition::new(), true);
    }

    fn new_definition_with_parent(&mut self, parent_id: Uuid) {
        let def = FieldDefinition::new();
        def.add_parent(parent_id);
        self.set_definition(def, true);
    }

    fn new_definition_with_child(&mut self, child_id: Uuid) {
        let def = FieldDefinition::new();
        def.add_child(child_id);
        self.set_definition(def, true);
    }

    fn set_existing_definition(&mut self, definition: &FieldDefinition) {
        self.set_definition(definition.clone(), false);
    }

    fn save(&mut self, app_state: AppStateRef) -> bool {
        self.error_message = None;
        let id = self.definition.as_ref().unwrap().id;
        return match self.verify(app_state.clone()) {
            Ok(_) => {
                let r = app_state.blocking_read();
                let vault = r.current_vault().expect("vault exists");
                vault.set_definition(self.definition.as_ref().unwrap().clone());
                for parent in std::mem::take(&mut self.removed_parents) {
                    let parent_def = vault.get_definition(&parent);
                    if let Some(def) = parent_def {
                        def.remove_child(id);
                    }
                }
                for child in std::mem::take(&mut self.removed_children) {
                    let child_def = vault.get_definition(&child);
                    if let Some(def) = child_def {
                        def.remove_parent(id);
                    }
                }
                self.is_new = false;
                self.updated = true;
                true
            }
            Err(e) => {
                self.error_message = Some(e);
                false
            }
        };
    }

    //noinspection DuplicatedCode
    fn edit_ui(&mut self, ui: &mut egui::Ui, state: AppStateRef) {
        let mut add_new_child = false;
        let mut add_new_parent = false;
        let id = self.id();
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

        egui::ScrollArea::vertical().show_viewport(ui, |ui, _vp| {
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
                            ui.add(widgets::TagTypeCombo::new(
                                id.with("tag_type"),
                                &mut def.field_type,
                            ));
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
                ui.vertical_centered_justified(|ui| {
                    let mut result = ListEditResult::None;
                    widgets::ListEdit::new("edit_tag_parent_table", &parent_ids, &mut result)
                        .row_height(22.0)
                        .header_label("Parents:".into())
                        .item_ui(|ui, parent_id| {
                            ui.add(widgets::Tag::new(&*vault.get_definition(parent_id)?));
                            None
                        })
                        .create_label("Add existing".into())
                        .create_ui(|ui, create_state| {
                            if ui
                                .add(
                                    widgets::FindTag::new(
                                        "edit_tag_new_parent",
                                        create_state,
                                        &vault,
                                    )
                                    .exclude_ids(&exclude_ids),
                                )
                                .changed()
                                && create_state.is_some()
                            {
                                Some(create_state.unwrap())
                            } else {
                                None
                            }
                        })
                        .updated(self.updated)
                        .ui(ui);

                    match result {
                        ListEditResult::None => {}
                        ListEditResult::Add(id) => def.add_parent(id),
                        ListEditResult::Remove(i) => {
                            let id = parent_ids[i];
                            def.remove_parent(id);
                            self.removed_parents.push(id);
                        }
                        ListEditResult::Edit(_, ()) => {}
                    }

                    if ui.button("Add new").clicked() {
                        add_new_parent = true;
                    }
                });
            });

            if def.field_type == FieldType::Container {
                ui.group(|ui| {
                    ui.vertical_centered_justified(|ui| {
                        let mut result = ListEditResult::None;
                        widgets::ListEdit::new("edit_tag_child_table", &child_ids, &mut result)
                            .row_height(22.0)
                            .header_label("Children:".into())
                            .item_ui(|ui, child_id| {
                                ui.add(widgets::Tag::new(&*vault.get_definition(child_id)?));
                                None
                            })
                            .create_label("Add existing".into())
                            .create_ui(|ui, create_state| {
                                if ui
                                    .add(
                                        widgets::FindTag::new(
                                            "edit_tag_new_child",
                                            create_state,
                                            &vault,
                                        )
                                        .exclude_ids(&exclude_ids),
                                    )
                                    .changed()
                                    && create_state.is_some()
                                {
                                    Some(create_state.unwrap())
                                } else {
                                    None
                                }
                            })
                            .updated(self.updated)
                            .ui(ui);

                        match result {
                            ListEditResult::None => {}
                            ListEditResult::Add(id) => def.add_child(id),
                            ListEditResult::Remove(i) => {
                                let id = child_ids[i];
                                def.remove_child(id);
                                self.removed_children.push(id);
                            }
                            ListEditResult::Edit(_, ()) => {}
                        }

                        if ui.button("Add new").clicked() {
                            add_new_child = true;
                        }
                    });
                });
            }

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
                        None
                    })
                    .create_label("Add alias".into())
                    .create_ui(|ui, create_state: &mut String| {
                        ui.text_edit_singleline(create_state);
                        if ui.add(egui::Button::new("\u{2714}").frame(false)).clicked()
                            && !create_state.is_empty()
                        {
                            Some(create_state.clone())
                        } else {
                            None
                        }
                    })
                    .ui(ui);

                let mut removed_alias = None;
                let mut added_alias = None;
                match result {
                    ListEditResult::None => {}
                    ListEditResult::Add(alias) => added_alias = Some(alias),
                    ListEditResult::Remove(i) => removed_alias = Some(aliases[i].clone()),
                    ListEditResult::Edit(_, ()) => {}
                };

                if added_alias.is_some() || removed_alias.is_some() {
                    let mut new_aliases = vec![];
                    for alias in aliases {
                        if Some(&alias) == removed_alias.as_ref() {
                            new_aliases.push(FieldValue::string(alias));
                        }
                    }
                    if let Some(alias) = added_alias {
                        new_aliases.push(FieldValue::string(alias));
                    }

                    def.set_known_field_value(fields::meta::ALIASES, new_aliases);
                }
            });
        });

        if add_new_parent && self.save(state.clone()) {
            self.new_definition_with_child(self.definition.as_ref().unwrap().id);
        }

        if add_new_child && self.save(state.clone()) {
            self.new_definition_with_parent(self.definition.as_ref().unwrap().id);
        }
    }

    fn verify(&self, state: AppStateRef) -> Result<(), String> {
        if let Some(def) = self.definition.as_ref() {
            if def.name.trim().is_empty() {
                return Err("Name must not be empty".into());
            }

            let r = state.blocking_read();
            let Ok(vault) = r.catch(|| r.current_vault()) else {
                return Err("No vault found".into());
            };

            if let Err(e) = vault.find_hierarchy_error(def) {
                return Err(format!("Hierarchy error: {}", e));
            }

            Ok(())
        } else {
            Err("Definition does not exist".into())
        }
    }
}

impl AppModal for EditTagDialog {
    fn id(&self) -> egui::Id {
        "edit_tag_window".into()
    }

    fn update(&mut self, ctx: &egui::Context, app_state: AppStateRef) -> &mut dyn AppModal {
        self.widget_state = State::load(ctx, self.id()).unwrap_or_default();
        let prev_updated = self.updated;
        let mut opened = self.widget_state.opened;

        let mut do_add = false;
        let mut do_save = false;
        let mut do_close = false;
        let mut do_reset = false;

        egui::Window::new("Edit tags")
            .id("edit_tag_window".into())
            .open(&mut opened)
            .show(ctx, |ui| {
                do_add |= shortcut!(ui, CTRL + N);
                do_reset |= shortcut!(ui, CTRL + Z);
                do_close |= shortcut!(ui, Escape);
                do_save |= shortcut!(ui, CTRL + S);

                egui::SidePanel::left("edit_tag_window_left").show_inside(ui, |ui| {
                    ui.vertical_centered_justified(|ui| {
                        let mut widget_state = std::mem::take(&mut self.widget_state);
                        if ui.button("Add tag").clicked() {
                            do_add = true;
                        }

                        ui.label(" - or - ");

                        ui.vertical(|ui| {
                            ui.add(
                                widgets::TagTree::new(
                                    "edit_tag_tree",
                                    &mut widget_state.selected_tag_ids,
                                    app_state.clone(),
                                )
                                .updated(self.updated),
                            );
                        });

                        if widget_state.selected_tag_ids.first()
                            != self.definition.as_ref().map(|d| &d.id)
                            || self.updated
                        {
                            if let Some(id) = widget_state.selected_tag_ids.first() {
                                let r = app_state.blocking_read();
                                let Ok(vault) = r.catch(|| r.current_vault()) else {
                                    return;
                                };
                                let def = vault.get_definition(id);
                                if let Some(def) = def {
                                    self.set_existing_definition(&def);
                                }
                            }
                        }

                        self.widget_state = widget_state;
                    });
                });

                egui::TopBottomPanel::bottom("edit_tag_window_bottom").show_inside(ui, |ui| {
                    if self.definition.is_some() {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .button(if self.is_new { "Create" } else { "Save" })
                                .clicked()
                            {
                                do_save = true;
                            }

                            if ui.button("Cancel").clicked() {
                                do_close = true;
                            }

                            if ui.button("Reset").clicked() {
                                do_reset = true;
                            }

                            if !self.is_new && self.definition.is_some() {
                                ui.with_layout(
                                    egui::Layout::left_to_right(egui::Align::Center),
                                    |ui| {
                                        if ui.button("Delete").clicked() {
                                            let r = app_state.blocking_read();
                                            r.add_dialog(super::DeleteDefinition::new(
                                                std::mem::take(&mut self.definition).unwrap(),
                                            ))
                                        }
                                    },
                                );
                            }
                        });
                    }
                });

                egui::CentralPanel::default().show_inside(ui, |ui| {
                    let r = app_state.blocking_read();
                    let Ok(vault) = r.catch(|| r.current_vault()) else {
                        return;
                    };
                    if let Some(def) = self.definition.as_ref() {
                        if self.is_new || vault.get_definition(&def.id).is_some() {
                            self.edit_ui(ui, app_state.clone());
                        }
                    }

                    if let Some(msg) = &self.error_message {
                        ui.colored_label(Color32::RED, msg);
                    }
                });
            });

        if do_reset {
            if !self.is_new {
                let r = app_state.blocking_read();
                let vault = r.current_vault().expect("vault exists");
                let def = vault.get_definition(&self.definition.as_ref().unwrap().id);
                if let Some(def) = def {
                    self.set_existing_definition(&def);
                }
            } else {
                self.new_definition();
            }
        }

        if do_add {
            self.new_definition();
            self.widget_state.selected_tag_ids = vec![];
        }

        if do_save {
            self.save(app_state.clone());
            self.updated = true;
        }

        if prev_updated && self.updated {
            self.updated = false;
        }

        if do_close {
            opened = false;
        }

        self.widget_state.opened = opened;
        self.opened = self.widget_state.opened;
        std::mem::take(&mut self.widget_state).store(ctx, self.id());
        self
    }

    fn dispose(&mut self, ctx: &egui::Context, _state: AppStateRef) {
        State::dispose(ctx, self.id());
    }

    fn is_open(&self) -> bool {
        self.opened
    }
}
