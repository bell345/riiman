use eframe::egui;
use eframe::egui::{Response, Ui, Widget};
use egui_extras::{TableBody, TableRow};
use std::marker::PhantomData;

#[allow(clippy::type_complexity)]
pub struct ListEdit<
    'a,
    'b,
    T,
    ItemF: Fn(&mut Ui, &T) + 'a,
    CreateState: Default + Clone + Send + Sync + 'static,
    CreateF: FnOnce(TableRow, &mut CreateState) -> Option<T> + 'a,
> {
    items: &'a Vec<T>,
    result: &'b mut ListEditResult<'a, T>,
    widget_id: egui::Id,
    header_label: String,
    create_label: String,
    item_ui: Option<Box<ItemF>>,
    create_ui: Option<Box<CreateF>>,
    row_height: f32,
    _phantom: PhantomData<CreateState>,
}

#[derive(Debug, Default, Clone)]
struct ListEditState<CreateState: Default + Clone + Send + Sync + 'static> {
    is_adding: bool,
    create_state: CreateState,
}

impl<CreateState: Default + Clone + Send + Sync + 'static> ListEditState<CreateState> {
    fn load(ctx: &egui::Context, id: egui::Id) -> Option<Self> {
        ctx.data(|r| r.get_temp(id))
    }

    fn store(self, ctx: &egui::Context, id: egui::Id) {
        ctx.data_mut(|wr| wr.insert_temp(id, self));
    }
}

pub enum ListEditResult<'a, T> {
    None,
    Add(T),
    Remove(&'a T),
}

impl<
        'a,
        'b,
        T,
        ItemF: Fn(&mut Ui, &T) + 'a,
        CreateState: Default + Clone + Send + Sync + 'static,
        CreateF: FnOnce(TableRow, &mut CreateState) -> Option<T> + 'a,
    > ListEdit<'a, 'b, T, ItemF, CreateState, CreateF>
{
    pub fn new(
        widget_id: impl std::hash::Hash,
        items: &'a Vec<T>,
        result: &'b mut ListEditResult<'a, T>,
    ) -> Self {
        Self {
            items,
            result,
            widget_id: egui::Id::new(widget_id),
            header_label: "".into(),
            create_label: "".into(),
            item_ui: None,
            create_ui: None,
            row_height: 18.0,
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
        self.row_height = row_height;
        self
    }

    fn body(&mut self, mut body: TableBody, state: &mut ListEditState<CreateState>) {
        for item in self.items {
            if let Some(item_ui) = self.item_ui.as_ref() {
                body.row(self.row_height, |mut row| {
                    row.col(|ui| item_ui(ui, item));
                    row.col(|ui| {
                        if ui.add(egui::Button::new("\u{274c}").frame(false)).clicked() {
                            *self.result = ListEditResult::Remove(item);
                        }
                    });
                });
            }
        }

        if state.is_adding {
            if let Some(create_ui) = self.create_ui.take() {
                body.row(self.row_height, |row| {
                    if let Some(new_item) = create_ui(row, &mut state.create_state) {
                        *self.result = ListEditResult::Add(new_item);
                        state.is_adding = false;
                        state.create_state = Default::default();
                    }
                });
            }
        }
    }
}

impl<
        'a,
        'b,
        T,
        ItemF: Fn(&mut Ui, &T) + 'a,
        CreateState: Default + Clone + Send + Sync + 'static,
        CreateF: FnOnce(TableRow, &mut CreateState) -> Option<T> + 'a,
    > Widget for ListEdit<'a, 'b, T, ItemF, CreateState, CreateF>
{
    fn ui(mut self, ui: &mut Ui) -> Response {
        *self.result = ListEditResult::None;

        let mut state = ListEditState::load(ui.ctx(), self.widget_id).unwrap_or_default();

        let res = ui.vertical_centered(|ui| {
            if !self.header_label.is_empty() {
                ui.label(&self.header_label);
            }

            ui.push_id(self.widget_id, |ui| {
                egui_extras::TableBuilder::new(ui)
                    .striped(true)
                    .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                    .column(egui_extras::Column::remainder())
                    .column(egui_extras::Column::exact(12.0))
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

        res.response
    }
}
