// Heavily informed by Jake Hansen's `egui_autocomplete`:
// https://github.com/JakeHandsome/egui_autocomplete/blob/master/src/lib.rs

use std::sync::Arc;

use eframe::egui;
use eframe::egui::{Rect, Response, Ui, Vec2, Widget};
use indexmap::IndexMap;
use uuid::Uuid;

use crate::data::{FieldDefinition, FieldType, TextSearchQuery, Vault};
use crate::take_shortcut;
use crate::tasks::filter::{evaluate_field_search, FieldMatchResult, MergedFieldMatchResult};
use crate::ui::cloneable_state::CloneableTempState;
use crate::ui::input::update_index;
use crate::ui::widgets;

const MAX_SUGGESTIONS: usize = 10;

pub struct FindTag<'a> {
    widget_id: egui::Id,
    tag_id: &'a mut Option<Uuid>,
    vault: Arc<Vault>,
    state: State,

    create_req: Option<&'a mut Option<String>>,

    exclude_ids: Option<&'a [Uuid]>,
    filter_types: Option<Vec<FieldType>>,
    max_suggestions: usize,
    show_tag: bool,
    desired_width: f32,
}

#[derive(Clone)]
enum AutocompleteResult {
    MatchResult(MergedFieldMatchResult),
    CreateResult,
}

#[derive(Default, Clone)]
struct State {
    search_text: String,
    search_query: TextSearchQuery,
    search_results: Option<Vec<AutocompleteResult>>,
    selected_index: Option<usize>,
    focused: bool,
}

impl CloneableTempState for State {}

impl<'a> FindTag<'a> {
    pub fn new(
        widget_id: impl std::hash::Hash,
        tag_id: &'a mut Option<Uuid>,
        vault: Arc<Vault>,
    ) -> Self {
        Self {
            widget_id: egui::Id::new(widget_id),
            tag_id,
            vault,
            state: State::default(),
            max_suggestions: 10,
            exclude_ids: None,
            filter_types: None,
            show_tag: false,
            desired_width: 120.0,
            create_req: None,
        }
    }

    pub fn exclude_ids(mut self, exclude_ids: &'a [Uuid]) -> Self {
        self.exclude_ids = Some(exclude_ids);
        self
    }

    pub fn filter_types(mut self, filter_types: &[FieldType]) -> Self {
        self.filter_types = Some(filter_types.to_vec());
        self
    }

    pub fn exclude_types(mut self, exclude_types: &[FieldType]) -> Self {
        self.filter_types = Some(
            FieldType::all()
                .iter()
                .filter(|t| !exclude_types.contains(t))
                .copied()
                .collect(),
        );
        self
    }

    pub fn show_tag(mut self, show_tag: bool) -> Self {
        self.show_tag = show_tag;
        self
    }

    pub fn desired_width(mut self, desired_width: f32) -> Self {
        self.desired_width = desired_width;
        self
    }

    pub fn create_request(mut self, create_req: &'a mut Option<String>) -> Self {
        self.create_req = Some(create_req);
        self
    }

    pub fn definition(&self) -> Option<FieldDefinition> {
        let tag_id = self.tag_id.as_ref()?;
        let def = self.vault.get_definition(tag_id)?;
        Some(def.clone())
    }

    pub fn merge_indices(
        &self,
        matches: &[FieldMatchResult],
    ) -> (
        Vec<u32>,
        IndexMap<String, Vec<u32>>,
        IndexMap<Uuid, Vec<u32>>,
        IndexMap<Uuid, IndexMap<String, Vec<u32>>>,
    ) {
        let mut name_indices = vec![];
        let mut aliases_and_indices = IndexMap::new();
        let mut parents_name_and_indices = IndexMap::new();
        let mut parents_aliases_and_indices = IndexMap::new();

        for m in matches {
            match m {
                FieldMatchResult::Name { indices, .. } => name_indices.append(&mut indices.clone()),
                FieldMatchResult::Alias { alias, indices, .. } => aliases_and_indices
                    .entry(alias.to_string())
                    .or_insert_with(Vec::new)
                    .append(&mut indices.clone()),
                FieldMatchResult::ParentName {
                    parent_id, indices, ..
                } => parents_name_and_indices
                    .entry(*parent_id)
                    .or_insert_with(Vec::new)
                    .append(&mut indices.clone()),
                FieldMatchResult::ParentAlias {
                    parent_id,
                    alias,
                    indices,
                    ..
                } => parents_aliases_and_indices
                    .entry(*parent_id)
                    .or_insert_with(IndexMap::new)
                    .entry(alias.to_string())
                    .or_insert_with(Vec::new)
                    .append(&mut indices.clone()),
            }
        }

        (
            name_indices,
            aliases_and_indices,
            parents_name_and_indices,
            parents_aliases_and_indices,
        )
    }

    fn new_search_results(&mut self) {
        self.state.search_query = TextSearchQuery::new(self.state.search_text.clone());
        let Ok(search_results) = evaluate_field_search(
            &self.vault,
            &self.state.search_query,
            self.exclude_ids,
            self.filter_types.as_deref(),
        ) else {
            return;
        };

        let mut vec: Vec<_> = search_results
            .into_iter()
            .map(AutocompleteResult::MatchResult)
            .collect();

        if self.create_req.is_some() && !self.state.search_text.is_empty() {
            if vec.len() >= MAX_SUGGESTIONS {
                vec.insert(9, AutocompleteResult::CreateResult);
            } else {
                vec.push(AutocompleteResult::CreateResult);
            }
        }

        self.state.search_results = Some(vec);
        self.state.selected_index = Some(0);
    }
    
    fn get_selected_option(&self) -> Option<&AutocompleteResult> {
        let index = self.state.selected_index.as_ref()?;
        let results = self.state.search_results.as_ref()?;
        results.get(*index)
    }

    fn popup_ui(&mut self, ui: &mut Ui, text_res: &Response) -> bool {
        let mut tag_selected = false;

        if let Some(result) = self.get_selected_option() {
            if take_shortcut!(ui, Tab) || take_shortcut!(ui, Enter) {
                tag_selected = true;
                match result {
                    AutocompleteResult::MatchResult(r) => *self.tag_id = Some(r.id),
                    AutocompleteResult::CreateResult => {
                        **self.create_req.as_mut().unwrap() =
                            Some(self.state.search_text.clone());
                    }
                }
            }
        }

        egui::popup::popup_below_widget(ui, self.widget_id, text_res, |ui| {
            ui.set_min_width(200.0);
            for (i, result) in self
                .state
                .search_results
                .as_ref()
                .unwrap()
                .iter()
                .take(self.max_suggestions)
                .enumerate()
            {
                let selected = self.state.selected_index == Some(i);

                match result {
                    AutocompleteResult::MatchResult(MergedFieldMatchResult { id, .. }) => {
                        let Some(def) = self.vault.get_definition(id) else {
                            return;
                        };

                        /*let text = if self.highlight {
                            highlight_matches(
                                def.name.as_ref(),
                                &name_indices,
                                ui.style().visuals.widgets.active.text_color(),
                            )
                        } else {
                            let mut job = egui::text::LayoutJob::default();
                            job.append(def.name.as_ref(), 0.0, egui::TextFormat::default());
                            job
                        };*/

                        let res = ui.add(widgets::Tag::new(&def).selected(selected));
                        if res.clicked() {
                            tag_selected = true;
                            *self.tag_id = Some(*id);
                        }
                        if res.has_focus() {
                            self.state.focused = true;
                        }
                    }
                    AutocompleteResult::CreateResult => {
                        let res = ui.selectable_label(
                            selected,
                            format!("New tag: {}", self.state.search_text),
                        );
                        if res.clicked() {
                            tag_selected = true;
                            **self.create_req.as_mut().unwrap() =
                                Some(self.state.search_text.clone());
                        }
                        if res.has_focus() {
                            self.state.focused = true;
                        }
                    }
                };
            }
        });

        tag_selected
    }
}

impl<'a> Widget for FindTag<'a> {
    fn ui(mut self, ui: &mut Ui) -> Response {
        ui.ctx().check_for_id_clash(
            self.widget_id,
            Rect::from_min_size(ui.available_rect_before_wrap().min, Vec2::ZERO),
            "FindTag",
        );

        self.state = State::load(ui.ctx(), self.widget_id).unwrap_or_default();

        let mut text_res = {
            let tags = if self.show_tag {
                self.definition().map(|def| vec![def]).unwrap_or_default()
            } else {
                Default::default()
            };
            ui.add(
                widgets::SearchBox::new(
                    self.widget_id.with("search_box"),
                    &mut self.state.search_text,
                    Arc::clone(&self.vault),
                )
                .tags(&tags)
                .desired_width(self.desired_width),
            )
        };

        self.state.focused = text_res.has_focus();

        if self.state.search_results.is_none() || text_res.changed() {
            self.new_search_results();
        }

        self.state.selected_index = update_index(
            self.state.selected_index,
            self.state.focused && take_shortcut!(ui, ArrowDown),
            self.state.focused && take_shortcut!(ui, ArrowUp),
            self.state.search_results.as_ref().unwrap().len(),
            MAX_SUGGESTIONS,
        );

        let tag_selected = self.popup_ui(ui, &text_res);

        if self.state.focused && !self.state.search_results.as_ref().unwrap().is_empty() {
            ui.memory_mut(|mem| mem.open_popup(self.widget_id));
        }

        if tag_selected {
            ui.memory_mut(|mem| {
                if mem.is_popup_open(self.widget_id) {
                    mem.close_popup();
                }
            });
            text_res.changed = true;
            self.state.focused = false;
            self.state.search_results = None;
            self.state.search_text.clear();
        }

        std::mem::take(&mut self.state).store(ui.ctx(), self.widget_id);

        text_res
    }
}

#[allow(clippy::cast_possible_truncation)]
fn highlight_matches(
    text: &str,
    match_indices: &[u32],
    color: egui::Color32,
) -> egui::text::LayoutJob {
    let mut formatted = egui::text::LayoutJob::default();
    let mut it = (0..text.len()).peekable();
    // Iterate through all indices in the string
    while let Some(j) = it.next() {
        let start = j;
        let mut end = j;
        let match_state = match_indices.contains(&(start as u32));
        // Find all consecutive characters that have the same state
        while let Some(k) = it.peek() {
            if match_state == match_indices.contains(&(*k as u32)) {
                end += 1;
                // Advance the iterator, we already peeked the value so it is fine to ignore
                _ = it.next();
            } else {
                break;
            }
        }
        // Format current slice based on the state
        let format = if match_state {
            egui::TextFormat::simple(egui::FontId::default(), color)
        } else {
            egui::TextFormat::default()
        };
        let slice = &text[start..=end];
        formatted.append(slice, 0.0, format);
    }
    formatted
}
