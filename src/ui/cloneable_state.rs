use eframe::egui;

pub trait CloneableState: Default + Clone + Send + Sync + 'static {
    fn load(ctx: &egui::Context, id: egui::Id) -> Option<Self> {
        ctx.data(|r| r.get_temp(id))
    }

    fn store(self, ctx: &egui::Context, id: egui::Id) {
        ctx.data_mut(|wr| wr.insert_temp(id, self));
    }

    fn dispose(ctx: &egui::Context, id: egui::Id) {
        ctx.data_mut(|wr| wr.remove_temp::<Self>(id));
    }
}
