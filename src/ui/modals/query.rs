use crate::state::AppStateRef;
use crate::tasks::AsyncTaskResult;
use crate::ui::AppModal;
use eframe::egui;
use egui_modal::Modal;

pub struct Query {
    id: egui::Id,
    modal: Option<Modal>,
    title: String,
    message: String,
    options: QueryOptions,
    opened: bool,
}

#[derive(Default)]
pub struct QueryOptions {
    pub kind: QueryKind,
    pub icon: egui_modal::Icon,
    pub default_button: DefaultButton,
}

#[derive(Debug, Default, Copy, Clone, Eq, PartialEq)]
pub enum QueryKind {
    #[default]
    Ok,
    YesNo,
    OkCancel,
    YesNoCancel,
    RetryCancel,
    CancelRetryContinue,
}

impl QueryKind {
    fn buttons(&self) -> &[QueryResult] {
        match self {
            QueryKind::Ok => &[QueryResult::Ok],
            QueryKind::YesNo => &[QueryResult::Yes, QueryResult::No],
            QueryKind::OkCancel => &[QueryResult::Ok, QueryResult::Cancel],
            QueryKind::YesNoCancel => &[QueryResult::Yes, QueryResult::No, QueryResult::Cancel],
            QueryKind::RetryCancel => &[QueryResult::Retry, QueryResult::Cancel],
            QueryKind::CancelRetryContinue => &[
                QueryResult::Cancel,
                QueryResult::Retry,
                QueryResult::Continue,
            ],
        }
    }
}

#[derive(Debug, Default, Copy, Clone, Eq, PartialEq)]
pub enum DefaultButton {
    #[default]
    Button1,
    Button2,
    Button3,
}

impl DefaultButton {
    fn matches_index(&self, i: usize) -> bool {
        match self {
            DefaultButton::Button1 => i == 0,
            DefaultButton::Button2 => i == 1,
            DefaultButton::Button3 => i == 2,
        }
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, derive_more::Display)]
pub enum QueryResult {
    Ok,
    Yes,
    No,
    Cancel,
    Retry,
    Continue,
}

impl Query {
    pub fn new(
        id: egui::Id,
        title: impl Into<String>,
        message: impl Into<String>,
        options: QueryOptions,
    ) -> Self {
        Self {
            id,
            title: title.into(),
            message: message.into(),
            options,
            modal: None,
            opened: false,
        }
    }
}

impl AppModal for Query {
    fn id(&self) -> egui::Id {
        self.id
    }

    fn update(&mut self, ctx: &egui::Context, state: AppStateRef) {
        let modal = Modal::new(ctx, format!("query_{}", self.id.value()));

        modal.show(|ui| {
            modal.title(ui, &self.title);
            modal.frame(ui, |ui| {
                modal.icon(ui, self.options.icon.clone());
                modal.body(ui, &self.message);
            });
            modal.buttons(ui, |ui| {
                for (i, result) in self.options.kind.buttons().iter().enumerate().rev() {
                    let res = if self.options.default_button.matches_index(i) {
                        modal.suggested_button(ui, result.to_string())
                    } else {
                        modal.button(ui, result.to_string())
                    };

                    if res.clicked() {
                        state
                            .add_completed_task(self.id, Ok(AsyncTaskResult::QueryResult(*result)));
                    }
                }
            });
        });

        if !self.opened {
            modal.open();
            self.opened = true;
        }

        self.modal = Some(modal);
    }

    fn is_open(&self) -> bool {
        self.modal.as_ref().is_some_and(|m| m.is_open())
    }
}
