use std::ops::Deref;
use std::path::Path;
use std::sync::{Arc, Mutex};

use dashmap::DashMap;
use eframe::egui;
use eframe::egui::{Response, Ui, Widget};
use poll_promise::Promise;
use uuid::Uuid;

use crate::data::{
    FieldDefinition, FieldStore, FieldType, FieldValue, Item, ShortcutAction, SimpleFieldStore,
    Vault,
};
use crate::state::AppStateRef;
use crate::take_shortcut;
use crate::tasks::transform::load_image_preview;
use crate::ui::cloneable_state::CloneableTempState;
use crate::ui::modals::EditTag;
use crate::ui::widgets;
use crate::ui::widgets::ListEditResult;

pub struct ItemPanel<'a, Ref: Deref<Target = Item> + 'a> {
    id: egui::Id,
    items: &'a Vec<Ref>,
    vault: Arc<Vault>,
    state: State,
    app_state: AppStateRef,
}

#[allow(clippy::struct_field_names)]
#[derive(Debug, Default, Clone)]
struct State {
    is_editing: bool,
    is_adding: bool,
    widest_tag_width: f32,
    row_heights: Vec<f32>,
    field_store: SimpleFieldStore,
    quick_create_state: CreateState,
}

impl CloneableTempState for State {}

#[derive(Debug, Clone)]
struct CreateState {
    tag_id: Option<Uuid>,
    tag_value: Option<FieldValue>,
    focused: bool,
    cancelled: bool,
}

impl Default for CreateState {
    fn default() -> Self {
        Self {
            tag_id: None,
            tag_value: None,
            focused: true,
            cancelled: false,
        }
    }
}

impl<'a, Ref: Deref<Target = Item> + 'a> ItemPanel<'a, Ref> {
    pub fn new(
        id: impl std::hash::Hash,
        items: &'a Vec<Ref>,
        vault: Arc<Vault>,
        app_state: AppStateRef,
    ) -> Self {
        Self {
            items,
            vault,
            id: egui::Id::new(id),
            state: State::default(),
            app_state,
        }
    }

    fn create_ui(
        &self,
        ui: &mut Ui,
        state: &mut CreateState,
        desired_width: f32,
        exclude_ids: &[Uuid],
    ) -> Option<(Uuid, FieldValue)> {
        let mut accept_value = false;
        let mut new_tag_name = None;
        let mut ret = None;
        ui.with_layout(egui::Layout::left_to_right(egui::Align::TOP), |ui| {
            if ui.add(egui::Button::new("\u{274c}").frame(false)).clicked() {
                state.cancelled = true;
            }

            let result = ui.add(
                widgets::FindTag::new(
                    self.id.with("new_tag"),
                    &mut state.tag_id,
                    Arc::clone(&self.vault),
                )
                .desired_width(desired_width)
                .show_tag(true)
                .create_request(&mut new_tag_name)
                .exclude_ids(exclude_ids)
                .exclude_types(&[FieldType::Container]),
            );

            if state.focused {
                result.request_focus();
                state.focused = false;
            }

            if take_shortcut!(ui, Escape) {
                state.cancelled = true;
            }

            let def = {
                if let Some(new_tag_name) = new_tag_name {
                    let mut def = FieldDefinition::new();
                    let id = def.id;
                    def.name = new_tag_name.into();
                    self.vault.set_definition(def);
                    self.vault.get_definition(&id).unwrap()
                } else {
                    let Some(id) = state.tag_id.as_ref() else {
                        return;
                    };
                    let Some(def) = self.vault.get_definition(id) else {
                        return;
                    };
                    def
                }
            };
            let id = def.id;

            ui.add(widgets::TagValueEdit::new(
                self.id.with("new_tag_value"),
                def.field_type,
                &mut state.tag_value,
            ));

            if ui.add(egui::Button::new("\u{2714}").frame(false)).clicked() {
                accept_value = true;
            }

            if def.field_type == FieldType::Tag {
                ret = Some((id, FieldValue::Tag));
            } else if let Some(v) = state.tag_value.as_ref() {
                if accept_value && v.get_type() == def.field_type {
                    ret = Some((id, v.clone()));
                }
            }
        });

        ret
    }

    pub fn edit_ui(&mut self, ui: &mut Ui, item: &Item) {
        let mut fields: Vec<_> = self
            .state
            .field_store
            .iter_fields_with_defs(&self.vault)
            .collect();
        fields.sort_by_key(|r| r.definition().name.clone());

        let existing_ids: Vec<_> = fields.iter().map(|f| f.definition().id).collect();

        let mut result = ListEditResult::None;
        let widest_tag_width = Mutex::new(self.state.widest_tag_width);
        while self.state.row_heights.len() < fields.len() {
            self.state.row_heights.push(22.0);
        }
        let existing_row_heights = self.state.row_heights.clone();
        let new_row_heights = DashMap::new();
        let app_state = self.app_state.clone();
        ui.add(
            widgets::ListEdit::new(self.id.with("list_edit"), &fields, &mut result)
                .row_height_list(&existing_row_heights, 22.0)
                .item_ui(|ui, item| {
                    let mut res = None;
                    ui.horizontal(|ui| {
                        let tag_res = ui.add(widgets::Tag::new(item.definition()));
                        if tag_res.clicked() {
                            app_state.add_dialog(EditTag::edit(item.definition().clone()));
                        }

                        let tag_space = tag_res.rect.width();
                        let mut l = widest_tag_width.lock().unwrap();
                        if tag_space < *l {
                            ui.add_space(*l - tag_space);
                        } else {
                            *l = tag_space;
                        }

                        let mut value = Some(item.value().clone());
                        let value_res = ui.add(widgets::TagValueEdit::new(
                            ui.id().with("value_edit"),
                            item.definition().field_type,
                            &mut value,
                        ));
                        new_row_heights.insert(item.definition().id, value_res.rect.height());
                        if let Some(v) = value {
                            if v.get_type() == item.definition().field_type && &v != item.value() {
                                res = Some(v);
                            }
                        }
                    });
                    res
                })
                .create_label("Add tag".into())
                .create_ui(|ui, state: &mut CreateState| {
                    let res =
                        self.create_ui(ui, state, *widest_tag_width.lock().unwrap(), &existing_ids);
                    state.focused = false;
                    res
                }),
        );
        drop(fields);

        self.state.row_heights = existing_ids
            .iter()
            .map(|id| new_row_heights.get(id).map_or(22.0, |v| *v) + 6.0)
            .collect();
        self.state.widest_tag_width = widest_tag_width.into_inner().unwrap();

        match result {
            ListEditResult::None => {}
            ListEditResult::Add((id, v)) => self.state.field_store.set_field_value(id, v),
            ListEditResult::Remove(i) => {
                self.state.field_store.remove_field(&existing_ids[i]);
            }
            ListEditResult::Edit(i, v) => {
                self.state.field_store.set_field_value(existing_ids[i], v);
            }
        };

        ui.horizontal(|ui| {
            if ui.button("Cancel").clicked() || take_shortcut!(ui, Escape) {
                self.state.is_editing = false;
            }

            if ui.button("OK").clicked() || take_shortcut!(ui, Enter) {
                item.clear();
                item.update(&self.state.field_store);
                if self.app_state.commit_item_catch(None, item).is_err() {
                    return;
                }
                self.state.is_editing = false;
            }
        });
    }

    pub fn view_ui(&mut self, ui: &mut Ui, item: &Item) {
        let mut fields: Vec<_> = item.iter_fields_with_defs(&self.vault).collect();
        fields.sort_by_key(|r| r.definition().name.clone());

        let existing_ids: Vec<_> = fields.iter().map(|f| f.definition().id).collect();

        for def in fields {
            ui.add(widgets::Tag::new(def.definition()).value(def.value()));
        }

        if self.state.is_adding {
            let mut create_state = self.state.quick_create_state.clone();
            if let Some((k, v)) = self.create_ui(ui, &mut create_state, 200.0, &existing_ids) {
                item.set_field_value(k, v);
                if self.app_state.commit_item_catch(None, item).is_err() {
                    return;
                }
                self.state.is_adding = false;
            }
            if create_state.cancelled {
                self.state.is_adding = false;
            }
            self.state.quick_create_state = create_state;
        }

        let is_viewing = !self.state.is_editing && !self.state.is_adding;

        if ui.button("Edit tags").clicked() || (is_viewing && take_shortcut!(ui, CTRL + E)) {
            self.state.is_editing = true;
            self.state.widest_tag_width = 100.0;
            self.state.field_store.clear();
            self.state.field_store.update(item);
        }

        if !self.state.is_adding && ui.button("Add tag").clicked()
            || (is_viewing && take_shortcut!(ui, Backtick))
        {
            self.state.is_adding = true;
            self.state.quick_create_state = Default::default();
        }

        let shortcuts = self.app_state.shortcuts();
        for (shortcut, action) in shortcuts {
            if matches!(action, ShortcutAction::None) {
                continue;
            }

            if ui.input_mut(|i| i.consume_key(shortcut.modifiers, shortcut.logical_key)) {
                match action {
                    ShortcutAction::None => {}
                    ShortcutAction::ToggleTag(tag_id) => {
                        if item.has_field(&tag_id) {
                            item.remove_field(&tag_id);
                        } else {
                            match self.vault.get_definition(&tag_id) {
                                Some(def) if def.field_type == FieldType::Tag => {
                                    item.set_field_value(tag_id, FieldValue::Tag);
                                }
                                _ => {}
                            }
                        }

                        if self.app_state.commit_item_catch(None, item).is_err() {
                            return;
                        }
                    }
                }
            }
        }
    }

    pub fn single_ui(&mut self, ui: &mut Ui, item: &Item) {
        ui.label(egui::RichText::new(item.path()).text_style(egui::TextStyle::Heading));

        if ui.button("Open Preview").clicked() {
            let path = Path::new(item.path());
            let Ok(abs_path) = self.app_state.catch(
                || format!("resolving abs path for {}", path.display()),
                || self.vault.resolve_abs_path(path),
            ) else {
                return;
            };
            self.app_state.add_task("Load image preview", move |_, _| {
                Promise::spawn_blocking(move || load_image_preview(abs_path))
            });
        }

        if self.state.is_editing {
            self.edit_ui(ui, item);
        } else {
            self.view_ui(ui, item);
        }
    }

    pub fn multiple_ui(&mut self, ui: &mut Ui) {
        ui.label(format!(
            "{} item{}",
            self.items.len(),
            if self.items.len() == 1 { "" } else { "s" }
        ));
    }
}

impl<'a, Ref: Deref<Target = Item> + 'a> Widget for ItemPanel<'a, Ref> {
    fn ui(mut self, ui: &mut Ui) -> Response {
        self.state = State::load(ui.ctx(), self.id).unwrap_or_default();

        let res = ui
            .vertical(|ui| {
                if self.items.len() == 1 {
                    let item = self.items.first().unwrap();
                    self.single_ui(ui, item);
                } else {
                    self.multiple_ui(ui);
                }
            })
            .response;

        std::mem::take(&mut self.state).store(ui.ctx(), self.id);

        res
    }
}
