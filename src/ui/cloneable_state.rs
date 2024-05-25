use eframe::egui;
use serde::{Deserialize, Serialize};

pub trait CloneableTempState: Default + Clone + Send + Sync + 'static {
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

pub trait CloneablePersistedState: Default + Clone + Serialize + Send + Sync + 'static {
    fn load(ctx: &egui::Context, id: egui::Id) -> Option<Self>
    where
        for<'a> Self: Deserialize<'a>,
    {
        ctx.data_mut(|wr| wr.get_persisted(id))
    }

    fn store(self, ctx: &egui::Context, id: egui::Id)
    where
        for<'a> Self: Deserialize<'a>,
    {
        ctx.data_mut(|wr| wr.insert_persisted(id, self));
    }
}
