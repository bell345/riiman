use egui_modal::Modal;

#[derive(Default)]
pub struct ErrorDialog {
    modal: Option<Modal>,
    message: String,
}

impl ErrorDialog {
    pub fn update(&mut self, ctx: &eframe::egui::Context) -> &mut Self {
        let modal = Modal::new(ctx, "error_modal");

        modal.show(|ui| {
            modal.title(ui, "Error");
            modal.body(ui, format!("An error has occurred:\n{}", self.message));
            modal.buttons(ui, |ui| {
                modal.button(ui, "OK");
            });
        });

        self.modal = Some(modal);
        self
    }

    pub fn open(&mut self, message: String) {
        self.message = message;
        self.modal.take().unwrap().open();
    }
}
