use crate::state::AppStateRef;
use crate::ui::modals::AppModal;
use egui_modal::{Icon, Modal};
use uuid::Uuid;

#[derive(Default)]
pub struct Message {
    id: Uuid,
    modal: Option<Modal>,
    title: Option<String>,
    message: String,
    msg_type: MessageType,
    opened: bool,
}

#[derive(Debug, Default)]
pub enum MessageType {
    #[default]
    Info,
    Success,
    Error,
}

impl Message {
    pub fn error(message: String) -> Self {
        tracing::error!(message);
        Self {
            id: Uuid::new_v4(),
            message,
            msg_type: MessageType::Error,
            ..Default::default()
        }
    }

    pub fn info(message: String) -> Self {
        tracing::info!(message);
        Self {
            id: Uuid::new_v4(),
            message,
            msg_type: MessageType::Info,
            ..Default::default()
        }
    }

    pub fn success(message: String) -> Self {
        tracing::info!(message);
        Self {
            id: Uuid::new_v4(),
            message,
            msg_type: MessageType::Success,
            ..Default::default()
        }
    }

    pub fn with_title(mut self, title: String) -> Self {
        self.title = Some(title);
        self
    }
}

impl AppModal for Message {
    fn id(&self) -> eframe::egui::Id {
        eframe::egui::Id::new(self.id)
    }

    fn update(&mut self, ctx: &eframe::egui::Context, _state: AppStateRef) {
        let modal = Modal::new(ctx, format!("msg_modal_{}", self.id));

        modal.show(|ui| {
            match self.msg_type {
                MessageType::Info => {
                    modal.title(ui, self.title.as_ref().unwrap_or(&"Info".to_string()));
                    modal.body(ui, &self.message);
                    modal.icon(ui, Icon::Info);
                }
                MessageType::Success => {
                    modal.title(ui, self.title.as_ref().unwrap_or(&"Success".to_string()));
                    modal.body(ui, &self.message);
                    modal.icon(ui, Icon::Success);
                }
                MessageType::Error => {
                    modal.title(ui, self.title.as_ref().unwrap_or(&"Error".to_string()));
                    modal.body(ui, format!("An error has occurred {}", self.message));
                    modal.icon(ui, Icon::Error);
                }
            }
            modal.buttons(ui, |ui| {
                modal.button(ui, "OK");
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
