use crate::data::FieldDefinition;
use crate::shortcut;
use crate::state::AppStateRef;
use crate::tasks::filter::{evaluate_field_search, MergedFieldMatchResult, TextSearchQuery};
use crate::ui::cloneable_state::CloneableTempState;
use crate::ui::widgets;
use eframe::egui;
use eframe::egui::{Response, Ui, Vec2, Widget};
use eframe::emath::Rect;
use indexmap::IndexMap;
use itertools::Itertools;
use std::collections::HashSet;
use uuid::Uuid;

pub struct TagTree<'a> {
    widget_id: egui::Id,
    selected_tag_ids: &'a mut Vec<Uuid>,
    app_state: AppStateRef,
    updated: bool,
}

#[derive(Debug, Clone)]
struct TagTreeEntry {
    pub item: MergedFieldMatchResult,
    pub collapsed: bool,
    pub children: IndexMap<Uuid, TagTreeEntry>,
}

impl TagTreeEntry {
    pub fn new(item: MergedFieldMatchResult) -> Self {
        Self {
            item,
            collapsed: false,
            children: Default::default(),
        }
    }

    pub fn count(&self) -> usize {
        self.children.iter().map(|(_, v)| v.count()).sum::<usize>() + 1
    }

    /*pub fn up_index(&self, index: usize) -> Option<usize> {
        if index == 0 {
            return None;
        }

        let mut pos = 1;
        let mut prev_visible_idx = 0;
        for curr in &self.children {
            if pos == index {
                return match curr.collapsed {
                    true => None,
                    false => Some(prev_visible_idx)
                };
            }
        }

        None
    }

    pub fn down_index(&self, index: usize) -> Option<usize> {
        if index == 0 {
            return Some(match self.collapsed {
                false => 1,
                true => self.count()
            })
        }

        let mut pos = 1;
        for child in &self.children {
            if let Some(i) = child.down_index(index - pos) {
                return Some(i + pos);
            }
            pos += child.count();
        }

        None
    }*/

    fn tag_ui(
        &mut self,
        ui: &mut Ui,
        def: &FieldDefinition,
        selected_ids: &mut HashSet<Uuid>,
    ) -> Response {
        let res = ui.add(widgets::Tag::new(def).selected(selected_ids.contains(&self.item.id)));
        if res.clicked() {
            selected_ids.clear();
            selected_ids.insert(self.item.id);
        }
        res
    }

    pub fn ui(
        &mut self,
        ui: &mut Ui,
        depth: usize,
        state: AppStateRef,
        selected_ids: &mut HashSet<Uuid>,
    ) -> Option<Response> {
        let r = state.blocking_read();
        let vault = r.current_vault_opt()?;
        let def = vault.get_definition(&self.item.id)?;
        if self.children.is_empty() {
            Some(self.tag_ui(ui, &def, selected_ids))
        } else {
            Some(
                egui::collapsing_header::CollapsingState::load_with_default_open(
                    ui.ctx(),
                    egui::Id::new(self.item.id),
                    !self.collapsed,
                )
                .show_header(ui, |ui| Some(self.tag_ui(ui, &def, selected_ids)))
                .body(|ui| self.children_ui(ui, depth, state.clone(), selected_ids))
                .1
                .response,
            )
        }
    }

    pub fn children_ui(
        &mut self,
        ui: &mut Ui,
        depth: usize,
        state: AppStateRef,
        selected_ids: &mut HashSet<Uuid>,
    ) {
        for (_, v) in &mut self.children {
            v.ui(ui, depth + 1, state.clone(), selected_ids);
        }
    }
}

fn resolve_index(
    entries: &IndexMap<Uuid, TagTreeEntry>,
    mut index: usize,
) -> Option<&MergedFieldMatchResult> {
    for (_, entry) in entries {
        if index == 0 {
            return Some(&entry.item);
        }

        if let Some(child_entry) = resolve_index(&entry.children, index - 1) {
            return Some(child_entry);
        }

        index -= entry.count();
    }

    None
}

#[derive(Default, Clone)]
struct State {
    search_text: String,
    search_query: TextSearchQuery,
    tree: Option<IndexMap<Uuid, TagTreeEntry>>,

    selected_indices: Vec<usize>,
    focused: bool,
}

impl CloneableTempState for State {}

impl State {
    fn search_results_count(&self) -> usize {
        let Some(results) = self.tree.as_ref() else {
            return 0;
        };
        results.iter().map(|(_, v)| v.count()).sum::<usize>()
    }

    fn update_index(&mut self, down_pressed: bool, up_pressed: bool) {
        if !down_pressed && !up_pressed {
            return;
        }

        self.selected_indices = match self.selected_indices[..] {
            // Increment selected index when down is pressed,
            // limit it to the number of matches and max_suggestions
            [.., index] if down_pressed => {
                if index + 1 < self.search_results_count() {
                    vec![index + 1]
                } else {
                    vec![index]
                }
            }
            // Decrement selected index if up is pressed. Deselect if at first index
            [index, ..] if up_pressed => {
                if index == 0 {
                    vec![]
                } else {
                    vec![index - 1]
                }
            }
            // If nothing is selected and down is pressed, select first item
            [] if down_pressed => vec![0],
            // Do nothing if no keys are pressed,
            _ => self.selected_indices.clone(),
        }
    }
}

impl<'a> TagTree<'a> {
    pub fn new(
        widget_id: impl std::hash::Hash,
        selected_tag_ids: &'a mut Vec<Uuid>,
        app_state: AppStateRef,
    ) -> Self {
        Self {
            widget_id: egui::Id::new(widget_id),
            selected_tag_ids,
            app_state,
            updated: false,
        }
    }

    pub fn updated(mut self, updated: bool) -> Self {
        self.updated = updated;
        self
    }
}

impl<'a> Widget for TagTree<'a> {
    fn ui(self, ui: &mut Ui) -> Response {
        ui.ctx().check_for_id_clash(
            self.widget_id,
            Rect::from_min_size(ui.available_rect_before_wrap().min, Vec2::ZERO),
            "TagTree",
        );

        let mut state = State::load(ui.ctx(), self.widget_id).unwrap_or_default();

        let text_res = ui.add(widgets::SearchBox::new(&mut state.search_text));

        ui.separator();

        state.focused = text_res.has_focus();

        if state.tree.is_none() || text_res.changed() || self.updated {
            state.search_query = TextSearchQuery::new(state.search_text.clone());
            let r = self.app_state.blocking_read();
            let Ok(vault) = r.catch(|| r.current_vault()) else {
                return text_res;
            };

            let Ok(search_results) =
                r.catch(|| evaluate_field_search(&vault, &state.search_query, None, None))
            else {
                return text_res;
            };

            let mut tree = IndexMap::new();
            for result in search_results {
                for mut path in vault.iter_field_ancestor_paths(&result.id) {
                    let mut entry: Option<&mut TagTreeEntry> = None;
                    while let Some(parent_id) = path.pop_front() {
                        entry = Some(
                            match entry {
                                None => tree.entry(parent_id),
                                Some(e) => e.children.entry(parent_id),
                            }
                            .or_insert_with(|| {
                                TagTreeEntry::new(MergedFieldMatchResult::no_matches(parent_id))
                            }),
                        );
                    }

                    if let Some(entry) = entry {
                        entry.item = result.clone();
                    }
                }
            }

            state.tree = Some(tree);
            state.selected_indices = vec![0];
        }

        state.update_index(
            state.focused && shortcut!(ui, ArrowDown),
            state.focused && shortcut!(ui, ArrowUp),
        );

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_viewport(ui, |ui, _vp| {
                if let Some(search_results) = state.tree.as_mut() {
                    let mut selected_ids: HashSet<Uuid, _> =
                        HashSet::from_iter(std::mem::take(self.selected_tag_ids));
                    let prev_set = selected_ids.clone();
                    for (_, item) in search_results {
                        item.ui(ui, 0, self.app_state.clone(), &mut selected_ids);
                    }

                    let diff = selected_ids.difference(&prev_set).collect_vec();
                    *self.selected_tag_ids = if let Some(new_id) = diff.first() {
                        vec![**new_id]
                    } else {
                        prev_set.into_iter().take(1).collect()
                    };
                } else {
                    *self.selected_tag_ids = vec![];
                }
            });

        state.store(ui.ctx(), self.widget_id);

        text_res
    }
}
