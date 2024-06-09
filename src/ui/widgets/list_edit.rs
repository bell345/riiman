use std::marker::PhantomData;

use crate::ui::cloneable_state::CloneableTempState;
use eframe::egui;
use eframe::egui::{Response, Ui, Widget};
use egui_extras::TableBody;

#[allow(clippy::type_complexity)]
pub struct ListEdit<
    'a,
    'b,
    T,
    EditT,
    CreateT,
    ItemF: Fn(&mut Ui, &T) -> Option<EditT> + 'a,
    CreateState: Default + Clone + Send + Sync + 'static,
    CreateF: FnOnce(&mut Ui, &mut CreateState) -> Option<CreateT> + 'a,
> {
    items: &'a Vec<T>,
    result: &'b mut ListEditResult<EditT, CreateT>,
    widget_id: egui::Id,
    header_label: String,
    create_label: String,
    item_ui: Option<Box<ItemF>>,
    create_ui: Option<Box<CreateF>>,
    row_height: RowHeight<'a>,
    updated: bool,
    _phantom: PhantomData<CreateState>,
}

enum RowHeight<'a> {
    Same(f32),
    List(&'a [f32], f32),
}

impl<'a> RowHeight<'a> {
    fn get(&self, i: usize) -> f32 {
        match self {
            RowHeight::Same(v) => *v,
            RowHeight::List(l, default) => *l.get(i).unwrap_or(default),
        }
    }
}

#[derive(Default, Clone)]
struct State<CreateState: Default + Clone + Send + Sync + 'static> {
    is_adding: bool,
    create_state: CreateState,
}

impl<CreateState: Default + Clone + Send + Sync + 'static> CloneableTempState
    for State<CreateState>
{
}

#[derive(Default)]
pub enum ListEditResult<EditT, CreateT> {
    #[default]
    None,
    Add(CreateT),
    Remove(usize),
    Edit(usize, EditT),
}

impl<
        'a,
        'b,
        T,
        EditT,
        CreateT,
        ItemF: Fn(&mut Ui, &T) -> Option<EditT> + 'a,
        CreateState: Default + Clone + Send + Sync + 'static,
        CreateF: FnOnce(&mut Ui, &mut CreateState) -> Option<CreateT> + 'a,
    > ListEdit<'a, 'b, T, EditT, CreateT, ItemF, CreateState, CreateF>
{
    pub fn new(
        widget_id: impl std::hash::Hash,
        items: &'a Vec<T>,
        result: &'b mut ListEditResult<EditT, CreateT>,
    ) -> Self {
        Self {
            items,
            result,
            widget_id: egui::Id::new(widget_id),
            header_label: String::new(),
            create_label: String::new(),
            item_ui: None,
            create_ui: None,
            row_height: RowHeight::Same(18.0),
            updated: false,
            _phantom: Default::default(),
        }
    }

    pub fn header_label(mut self, header_label: String) -> Self {
        self.header_label = header_label;
        self
    }

    pub fn create_label(mut self, create_label: String) -> Self {
        self.create_label = create_label;
        self
    }

    pub fn item_ui(mut self, item_ui: ItemF) -> Self {
        self.item_ui = Some(Box::new(item_ui));
        self
    }

    pub fn create_ui(mut self, create_ui: CreateF) -> Self {
        self.create_ui = Some(Box::new(create_ui));
        self
    }

    pub fn row_height(mut self, row_height: f32) -> Self {
        self.row_height = RowHeight::Same(row_height);
        self
    }

    pub fn row_height_list(mut self, row_height_list: &'a [f32], default: f32) -> Self {
        self.row_height = RowHeight::List(row_height_list, default);
        self
    }

    pub fn updated(mut self, updated: bool) -> Self {
        self.updated = updated;
        self
    }

    fn body(&mut self, mut body: TableBody, state: &mut State<CreateState>) {
        for (i, item) in self.items.iter().enumerate() {
            if let Some(item_ui) = self.item_ui.as_ref() {
                body.row(self.row_height.get(i), |mut row| {
                    row.col(|ui| {
                        if ui.add(egui::Button::new("\u{274c}").frame(false)).clicked() {
                            *self.result = ListEditResult::Remove(i);
                        }
                    });
                    row.col(|ui| {
                        ui.push_id(self.widget_id.with(i), |ui| {
                            if let Some(edited_item) = item_ui(ui, item) {
                                *self.result = ListEditResult::Edit(i, edited_item);
                            }
                        });
                    });
                });
            }
        }

        if state.is_adding {
            if let Some(create_ui) = self.create_ui.take() {
                body.row(self.row_height.get(self.items.len()), |mut row| {
                    row.col(|ui| {
                        if ui.add(egui::Button::new("\u{274c}").frame(false)).clicked() {
                            state.is_adding = false;
                            state.create_state = Default::default();
                        }
                    });
                    row.col(|ui| {
                        if let Some(new_item) = create_ui(ui, &mut state.create_state) {
                            *self.result = ListEditResult::Add(new_item);
                            state.is_adding = false;
                            state.create_state = Default::default();
                        }
                    });
                });
            }
        }
    }
}

impl<
        'a,
        'b,
        T,
        EditT,
        CreateT,
        ItemF: Fn(&mut Ui, &T) -> Option<EditT> + 'a,
        CreateState: Default + Clone + Send + Sync + 'static,
        CreateF: FnOnce(&mut Ui, &mut CreateState) -> Option<CreateT> + 'a,
    > Widget for ListEdit<'a, 'b, T, EditT, CreateT, ItemF, CreateState, CreateF>
{
    fn ui(mut self, ui: &mut Ui) -> Response {
        *self.result = ListEditResult::None;

        let mut state = State::load(ui.ctx(), self.widget_id).unwrap_or_default();

        let res = ui.vertical_centered_justified(|ui| {
            if !self.header_label.is_empty() {
                ui.label(&self.header_label);
            }

            ui.push_id(self.widget_id, |ui| {
                egui_extras::TableBuilder::new(ui)
                    .striped(true)
                    .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                    .column(egui_extras::Column::exact(12.0))
                    .column(egui_extras::Column::remainder())
                    .auto_shrink([false, true])
                    .vscroll(false)
                    .body(|body| self.body(body, &mut state));
            });

            if !self.create_label.is_empty() && ui.button(&self.create_label).clicked() {
                state.is_adding = true;
                state.create_state = Default::default();
            }
        });

        state.store(ui.ctx(), self.widget_id);

        if self.updated {
            State::<CreateState>::dispose(ui.ctx(), self.widget_id);
        }

        res.response
    }
}
