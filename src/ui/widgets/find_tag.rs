/// Heavily informed by Jake Hansen's 'egui_autocomplete': https://github.com/JakeHandsome/egui_autocomplete/blob/master/src/lib.rs
use std::sync::Arc;

use eframe::egui;
use eframe::egui::{Rect, Response, Ui, Vec2, Widget};
use indexmap::IndexMap;
use tracing::info;
use uuid::Uuid;

use crate::state::AppStateRef;
use crate::tasks::filter::{
    evaluate_field_search, FieldMatchResult, MergedFieldMatchResult, TextSearchQuery,
};

pub struct FindTag<'a> {
    widget_id: egui::Id,
    tag_id: &'a mut Option<Uuid>,
    app_state: AppStateRef,

    max_suggestions: usize,
    highlight: bool,
}

#[derive(Default, Clone)]
struct State {
    search_text: String,
    search_query: TextSearchQuery,
    search_results: Option<Vec<MergedFieldMatchResult>>,
    selected_index: Option<usize>,
    focused: bool,
}

impl State {
    fn load(ctx: &egui::Context, id: egui::Id) -> Option<Self> {
        ctx.data(|r| r.get_temp(id))
    }

    fn store(self, ctx: &egui::Context, id: egui::Id) {
        ctx.data_mut(|wr| wr.insert_temp(id, self));
    }

    fn update_index(
        &mut self,
        down_pressed: bool,
        up_pressed: bool,
        match_results_count: usize,
        max_suggestions: usize,
    ) {
        self.selected_index = match self.selected_index {
            // Increment selected index when down is pressed,
            // limit it to the number of matches and max_suggestions
            Some(index) if down_pressed => {
                if index + 1 < match_results_count.min(max_suggestions) {
                    Some(index + 1)
                } else {
                    Some(index)
                }
            }
            // Decrement selected index if up is pressed. Deselect if at first index
            Some(index) if up_pressed => {
                if index == 0 {
                    None
                } else {
                    Some(index - 1)
                }
            }
            // If nothing is selected and down is pressed, select first item
            None if down_pressed => Some(0),
            // Do nothing if no keys are pressed
            Some(index) => Some(index),
            None => None,
        }
    }
}

impl<'a> FindTag<'a> {
    pub fn new(
        widget_id: impl std::hash::Hash,
        tag_id: &'a mut Option<Uuid>,
        app_state: AppStateRef,
    ) -> Self {
        Self {
            widget_id: egui::Id::new(widget_id),
            tag_id,
            app_state,
            max_suggestions: 10,
            highlight: true,
        }
    }

    /// This determines the number of options appear in the dropdown menu
    pub fn max_suggestions(mut self, max_suggestions: usize) -> Self {
        self.max_suggestions = max_suggestions;
        self
    }
    /// If set to true, characters will be highlighted in the dropdown to show the match
    pub fn highlight_matches(mut self, highlight: bool) -> Self {
        self.highlight = highlight;
        self
    }
}

impl<'a> Widget for FindTag<'a> {
    fn ui(mut self, ui: &mut Ui) -> Response {
        ui.ctx().check_for_id_clash(
            self.widget_id,
            Rect::from_min_size(ui.available_rect_before_wrap().min, Vec2::ZERO),
            "FindTag",
        );

        let mut state = State::load(ui.ctx(), self.widget_id).unwrap_or_default();
        let original = self.tag_id.clone();

        let mut layouter = |ui: &egui::Ui, text: &str, _wrap_width: f32| -> Arc<egui::Galley> {
            let mut job = egui::text::LayoutJob::default();
            let style = ui.style();

            job.append(
                text,
                16.0,
                egui::TextFormat::simple(
                    egui::TextStyle::Body.resolve(style),
                    style.visuals.text_color(),
                ),
            );

            ui.fonts(|f| f.layout_job(job))
        };

        let up_pressed = state.focused
            && ui.input_mut(|input| {
                input.consume_key(egui::Modifiers::default(), egui::Key::ArrowUp)
            });
        let down_pressed = state.focused
            && ui.input_mut(|input| {
                input.consume_key(egui::Modifiers::default(), egui::Key::ArrowDown)
            });

        let mut text_res =
            ui.add(egui::TextEdit::singleline(&mut state.search_text).layouter(&mut layouter));
        state.focused = text_res.has_focus();

        if state.search_results.is_none() || text_res.changed() {
            state.search_query = TextSearchQuery::new(state.search_text.clone());
            let r = self.app_state.blocking_read();
            let Ok(vault) = r.catch(|| r.current_vault()) else { return text_res; };
            let Ok(search_results) =
                r.catch(|| evaluate_field_search(&vault, &state.search_query))
            else {
                return text_res;
            };
            state.search_results = Some(search_results);
            state.selected_index = None;
        }

        state.update_index(down_pressed, up_pressed, state.search_results.as_ref().unwrap().len(), 10);

        let sr = state.search_results.as_ref().unwrap();

        let accepted_by_keyboard = ui.input_mut(|input| input.key_pressed(egui::Key::Enter))
            || ui.input_mut(|input| input.key_pressed(egui::Key::Tab));
        if let (Some(index), true) = (
            state.selected_index,
            ui.memory(|mem| mem.is_popup_open(self.widget_id)) && accepted_by_keyboard,
        ) {
            state.search_text.clear();
            *self.tag_id = Some(sr[index].id);
        }

        let r = self.app_state.blocking_read();
        let Ok(vault) = r.catch(|| r.current_vault()) else { return text_res; };
        egui::popup::popup_below_widget(ui, self.widget_id, &text_res, |ui| {
            ui.set_min_width(200.0);
            for (i, MergedFieldMatchResult { id, matches }) in sr
                .iter()
                .take(self.max_suggestions)
                .enumerate()
            {
                let mut selected = if let Some(x) = state.selected_index {
                    x == i
                } else {
                    false
                };
                
                let Some(def) = vault.get_definition(id) else { return; };

                let mut name_indices = vec![];
                let mut aliases_and_indices = IndexMap::new();
                let mut parents_name_and_indices = IndexMap::new();
                let mut parents_aliases_and_indices = IndexMap::new();

                for m in matches {
                    match m {
                        FieldMatchResult::Name { indices, .. } => {
                            name_indices.append(&mut indices.clone())
                        }
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

                let text = if self.highlight {
                    highlight_matches(
                        def.name.as_ref(),
                        &name_indices,
                        ui.style().visuals.widgets.active.text_color(),
                    )
                } else {
                    let mut job = egui::text::LayoutJob::default();
                    job.append(def.name.as_ref(), 0.0, egui::TextFormat::default());
                    job
                };
                
                let res = ui.toggle_value(&mut selected, text);
                if res.clicked() {
                    info!("clicked on {}", def.name);
                    state.search_text.clear();
                    *self.tag_id = Some(*id);
                }
                if res.has_focus() {
                    state.focused = true;
                }
            }
        });
        
        if state.focused && !sr.is_empty()
        {
            ui.memory_mut(|mem| mem.open_popup(self.widget_id));
        } else {
            ui.memory_mut(|mem| {
                if mem.is_popup_open(self.widget_id) {
                    mem.close_popup()
                }
            });
        }
        state.store(ui.ctx(), self.widget_id);

        text_res.changed = *self.tag_id != original;
        text_res
    }
}

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
