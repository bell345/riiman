use eframe::egui;
use eframe::egui::{Color32, Widget};
use std::sync::Arc;
use uuid::Uuid;

use crate::data::{FieldDefinition, FieldStore, FieldType, FieldValue, Vault};
use crate::state::AppStateRef;
use crate::ui::cloneable_state::CloneableTempState;
use crate::ui::modals::AppModal;
use crate::ui::widgets;
use crate::ui::widgets::ListEditResult;
use crate::{fields, shortcut};

#[derive(Default)]
pub struct EditTag {
    definition: Option<FieldDefinition>,
    error_message: Option<String>,
    is_new: bool,

    removed_parents: Vec<Uuid>,
    removed_children: Vec<Uuid>,

    widget_state: State,
    app_state: AppStateRef,
    opened: bool,
    updated: bool,

    operation: Operation,
}

#[derive(Copy, Clone, Default, PartialEq, Eq)]
enum Operation {
    #[default]
    None,
    Add,
    Save,
    Close,
    Reset,
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

impl EditTag {
    pub fn select() -> Self {
        Self {
            ..Default::default()
        }
    }

    pub fn create() -> Self {
        let mut res = Self::select();
        res.new_definition();
        res
    }

    pub fn edit(definition: FieldDefinition) -> Self {
        let mut res = Self::select();
        res.set_existing_definition(definition);
        res
    }
}

impl EditTag {
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

    fn set_existing_definition(&mut self, definition: FieldDefinition) {
        self.set_definition(definition, false);
    }

    fn save(&mut self) -> bool {
        self.error_message = None;
        let id = self.definition.as_ref().unwrap().id;
        return match self.verify() {
            Ok(()) => {
                let r = self.app_state.blocking_read();
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

    fn edit_parents_ui(
        &mut self,
        ui: &mut egui::Ui,
        vault: Arc<Vault>,
        parent_ids: &Vec<Uuid>,
        exclude_ids: &[Uuid],
    ) -> bool {
        let mut add_new_parent = false;

        let Some(def) = self.definition.as_mut() else {
            return false;
        };

        ui.group(|ui| {
            ui.vertical_centered_justified(|ui| {
                let mut result = ListEditResult::None;
                widgets::ListEdit::new("edit_tag_parent_table", parent_ids, &mut result)
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
                                widgets::FindTag::new("edit_tag_new_parent", create_state, &vault)
                                    .exclude_ids(exclude_ids),
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
                    ListEditResult::None | ListEditResult::Edit(_, ()) => {}
                    ListEditResult::Add(id) => def.add_parent(id),
                    ListEditResult::Remove(i) => {
                        let id = parent_ids[i];
                        def.remove_parent(id);
                        self.removed_parents.push(id);
                    }
                }

                if ui.button("Add new").clicked() {
                    add_new_parent = true;
                }
            });
        });

        add_new_parent
    }

    fn edit_children_ui(
        &mut self,
        ui: &mut egui::Ui,
        vault: Arc<Vault>,
        child_ids: &Vec<Uuid>,
        exclude_ids: &[Uuid],
    ) -> bool {
        let mut add_new_child = false;

        let Some(def) = self.definition.as_mut() else {
            return false;
        };

        ui.group(|ui| {
            ui.vertical_centered_justified(|ui| {
                let mut result = ListEditResult::None;
                widgets::ListEdit::new("edit_tag_child_table", child_ids, &mut result)
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
                                widgets::FindTag::new("edit_tag_new_child", create_state, &vault)
                                    .exclude_ids(exclude_ids),
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
                    ListEditResult::None | ListEditResult::Edit(_, ()) => {}
                    ListEditResult::Add(id) => def.add_child(id),
                    ListEditResult::Remove(i) => {
                        let id = child_ids[i];
                        def.remove_child(id);
                        self.removed_children.push(id);
                    }
                }

                if ui.button("Add new").clicked() {
                    add_new_child = true;
                }
            });
        });

        add_new_child
    }

    fn edit_aliases_ui(&mut self, ui: &mut egui::Ui) {
        let r = self.app_state.blocking_read();
        let Some(def) = self.definition.as_mut() else {
            return;
        };

        ui.group(|ui| {
            let Ok(aliases) = r.catch::<Vec<_>, anyhow::Error>(
                || "edit_tag".into(),
                || {
                    Ok(def
                        .get_or_insert_known_field_value(fields::meta::ALIASES, vec![])?
                        .into_iter()
                        .filter_map(|v| v.as_str_opt().map(|s| s.to_string()))
                        .collect())
                },
            ) else {
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
                ListEditResult::None | ListEditResult::Edit(_, ()) => {}
                ListEditResult::Add(alias) => added_alias = Some(alias),
                ListEditResult::Remove(i) => removed_alias = Some(aliases[i].clone()),
            };

            if added_alias.is_some() || removed_alias.is_some() {
                let mut new_aliases = vec![];
                for alias in aliases {
                    if Some(&alias) != removed_alias.as_ref() {
                        new_aliases.push(FieldValue::string(alias.into()));
                    }
                }
                if let Some(alias) = added_alias {
                    new_aliases.push(FieldValue::string(alias.into()));
                }

                def.set_known_field_value(fields::meta::ALIASES, new_aliases);
            }
        });
    }

    //noinspection DuplicatedCode
    fn edit_ui(&mut self, ui: &mut egui::Ui) {
        let id = self.id();

        ui.heading("Preview");

        {
            let Some(def) = self.definition.as_ref() else {
                return;
            };
            ui.add(widgets::Tag::new(def));
        }

        ui.separator();

        ui.heading("Edit properties");

        egui::ScrollArea::vertical().show_viewport(ui, |ui, _vp| -> Result<(), ()> {
            let vault = self
                .app_state
                .blocking_current_vault(|| "edit tag".into())?;

            egui_extras::TableBuilder::new(ui)
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .column(egui_extras::Column::auto())
                .column(egui_extras::Column::remainder())
                .auto_shrink([false, true])
                .vscroll(false)
                .body(|mut body| {
                    let Some(def) = self.definition.as_mut() else {
                        return;
                    };

                    body.row(24.0, |mut row| {
                        row.col(|ui| {
                            ui.label("Name: ");
                        });
                        row.col(|ui| {
                            let mut name = def.name.to_string();
                            ui.text_edit_singleline(&mut name);
                            def.name = name.into();
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
                            let Ok(mut colour) = self.app_state.blocking_catch(
                                || "edit tag colour".into(),
                                |_| {
                                    def.get_or_insert_known_field_value(
                                        fields::meta::COLOUR,
                                        visuals.bg_fill.into(),
                                    )
                                },
                            ) else {
                                return;
                            };
                            ui.color_edit_button_srgb(colour.as_mut_slice());
                            def.set_known_field_value(fields::meta::COLOUR, colour);
                        });
                    });
                });

            let (parent_ids, child_ids, exclude_ids, field_type) = {
                let def = self.definition.as_ref().ok_or(())?;

                let parent_ids: Vec<Uuid> = def.iter_parent_ids().map(|u| *u).collect();
                let child_ids: Vec<Uuid> = def.iter_child_ids().map(|u| *u).collect();
                let mut exclude_ids = vec![def.id];
                exclude_ids.extend(&parent_ids);
                exclude_ids.extend(&child_ids);

                (parent_ids, child_ids, exclude_ids, def.field_type)
            };

            let add_new_parent =
                self.edit_parents_ui(ui, Arc::clone(&vault), &parent_ids, &exclude_ids);

            let add_new_child = if field_type == FieldType::Container {
                self.edit_children_ui(ui, Arc::clone(&vault), &child_ids, &exclude_ids)
            } else {
                false
            };

            self.edit_aliases_ui(ui);

            if add_new_parent && self.save() {
                self.new_definition_with_child(self.definition.as_ref().unwrap().id);
            }

            if add_new_child && self.save() {
                self.new_definition_with_parent(self.definition.as_ref().unwrap().id);
            }

            Ok(())
        });
    }

    fn verify(&self) -> Result<(), String> {
        if let Some(def) = self.definition.as_ref() {
            if def.name.trim().is_empty() {
                return Err("Name must not be empty".into());
            }

            let Ok(vault) = self
                .app_state
                .blocking_current_vault(|| "verify edit tag".into())
            else {
                return Err("No vault found".into());
            };

            if let Err(e) = vault.find_hierarchy_error(def) {
                return Err(format!("Hierarchy error: {e}"));
            }

            Ok(())
        } else {
            Err("Definition does not exist".into())
        }
    }

    fn side_tree_ui(&mut self, ui: &mut egui::Ui) {
        egui::SidePanel::left("edit_tag_window_left").show_inside(ui, |ui| -> Result<(), ()> {
            ui.vertical_centered_justified(|ui| {
                let mut widget_state = std::mem::take(&mut self.widget_state);
                if ui.button("Add tag").clicked() {
                    self.operation = Operation::Add;
                }

                ui.label(" - or - ");

                ui.vertical(|ui| {
                    ui.add(
                        widgets::TagTree::new(
                            "edit_tag_tree",
                            &mut widget_state.selected_tag_ids,
                            self.app_state.clone(),
                        )
                        .updated(self.updated),
                    );
                });

                if widget_state.selected_tag_ids.first() != self.definition.as_ref().map(|d| &d.id)
                    || self.updated
                {
                    if let Some(id) = widget_state.selected_tag_ids.first() {
                        let vault = self
                            .app_state
                            .blocking_current_vault(|| "edit tag set definition".into())?;
                        let def = vault.get_definition(id).map(|r| r.clone());
                        if let Some(def) = def {
                            self.set_existing_definition(def);
                        }
                    }
                }

                self.widget_state = widget_state;

                Ok(())
            })
            .inner
        });
    }

    fn bottom_button_ui(&mut self, ui: &mut egui::Ui) {
        egui::TopBottomPanel::bottom("edit_tag_window_bottom").show_inside(ui, |ui| {
            if self.definition.is_some() {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .button(if self.is_new { "Create" } else { "Save" })
                        .clicked()
                    {
                        self.operation = Operation::Save;
                    }

                    if ui.button("Cancel").clicked() {
                        self.operation = Operation::Close;
                    }

                    if ui.button("Reset").clicked() {
                        self.operation = Operation::Reset;
                    }

                    if !self.is_new && self.definition.is_some() {
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            if ui.button("Delete").clicked() {
                                let r = self.app_state.blocking_read();
                                r.add_dialog(super::DeleteDefinition::new(
                                    std::mem::take(&mut self.definition).unwrap(),
                                ));
                            }
                        });
                    }
                });
            }
        });
    }
}

impl AppModal for EditTag {
    fn id(&self) -> egui::Id {
        "edit_tag_window".into()
    }

    fn update(&mut self, ctx: &egui::Context, app_state: AppStateRef) -> &mut dyn AppModal {
        self.app_state = app_state;
        self.widget_state = State::load(ctx, self.id()).unwrap_or_default();
        let prev_updated = self.updated;
        let mut opened = self.widget_state.opened;

        egui::Window::new("Edit tags")
            .id("edit_tag_window".into())
            .open(&mut opened)
            .show(ctx, |ui| {
                self.operation = if shortcut!(ui, Escape) {
                    Operation::Close
                } else if shortcut!(ui, CTRL + N) {
                    Operation::Add
                } else if shortcut!(ui, CTRL + Z) {
                    Operation::Reset
                } else if shortcut!(ui, CTRL + S) {
                    Operation::Save
                } else {
                    self.operation
                };

                self.side_tree_ui(ui);

                self.bottom_button_ui(ui);

                egui::CentralPanel::default().show_inside(ui, |ui| -> Result<(), ()> {
                    let vault = self
                        .app_state
                        .blocking_current_vault(|| "edit tag".into())?;

                    if let Some(def) = self.definition.as_ref() {
                        if self.is_new || vault.get_definition(&def.id).is_some() {
                            self.edit_ui(ui);
                        }
                    }

                    if let Some(msg) = &self.error_message {
                        ui.colored_label(Color32::RED, msg);
                    }

                    Ok(())
                });
            });

        match std::mem::take(&mut self.operation) {
            Operation::None => {}
            Operation::Add => {
                self.new_definition();
                self.widget_state.selected_tag_ids = vec![];
            }
            Operation::Save => {
                self.save();
                self.updated = true;
            }
            Operation::Close => {
                opened = false;
            }
            Operation::Reset => {
                if self.is_new {
                    self.new_definition();
                } else {
                    let r = self.app_state.blocking_read();
                    let vault = r.current_vault().expect("vault exists");
                    let def = vault
                        .get_definition(&self.definition.as_ref().unwrap().id)
                        .map(|r| r.clone());
                    if let Some(def) = def {
                        drop(vault);
                        drop(r);
                        self.set_existing_definition(def);
                    }
                }
            }
        }

        if prev_updated && self.updated {
            self.updated = false;
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
