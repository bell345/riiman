use crate::data::FieldDefinition;
use crate::state::AppStateRef;
use crate::ui::cloneable_state::CloneableTempState;
use crate::ui::{widgets, AppModal};
use eframe::egui::{Context, Id};
use egui_modal::Modal;

#[derive(Default)]
pub struct DeleteDefinition {
    modal: Option<Modal>,
    definition: FieldDefinition,
    opened: bool,
}

impl DeleteDefinition {
    pub fn new(definition: FieldDefinition) -> Self {
        Self {
            definition,
            ..Default::default()
        }
    }
}

#[derive(Clone, Default)]
struct State {
    n_items: Option<usize>,
}

impl CloneableTempState for State {}

impl AppModal for DeleteDefinition {
    fn id(&self) -> Id {
        "delete_def_modal".into()
    }

    fn update(&mut self, ctx: &Context, app_state: AppStateRef) -> &mut dyn AppModal {
        let modal = Modal::new(ctx, self.id().value());

        modal.show(|ui| {
            let mut state = State::load(ctx, self.id()).unwrap_or_default();

            modal.title(ui, "Delete tag");
            modal.frame(ui, |ui| {
                ui.vertical_centered_justified(|ui| {
                    ui.label("You are about to delete the following tag:");
                    ui.add(widgets::Tag::new(&self.definition));

                    let app_state2 = app_state.clone();
                    let id = self.definition.id;
                    let n_items = *state.n_items.get_or_insert_with(move || {
                        let r = app_state2.blocking_read();
                        let Ok(vault) = r.current_vault() else {
                            return 0;
                        };

                        let items = vault.find_items_by_tag(&id);
                        items.len()
                    });

                    if n_items > 0 {
                        ui.label(format!(
                            "There {} currently {n_items} item{} with this tag.",
                            if n_items == 1 { "is" } else { "are" },
                            if n_items == 1 { "" } else { "s" }
                        ));
                    }

                    let r = app_state.blocking_read();
                    let Ok(vault) = r.catch(|| r.current_vault()) else {
                        return;
                    };
                    let descendants = vault.iter_descendants(&self.definition.id);
                    if !descendants.is_empty() {
                        ui.label("This will also delete all of the children of this tag:");
                        for descendant in descendants {
                            ui.add(widgets::Tag::new(&descendant));
                        }
                    }
                });
            });
            modal.buttons(ui, |ui| {
                if modal.suggested_button(ui, "Delete").clicked() {
                    let r = app_state.blocking_read();
                    let Ok(vault) = r.catch(|| r.current_vault()) else {
                        return;
                    };
                    vault.remove_definition(&self.definition.id);
                }
                modal.button(ui, "Cancel");
            });

            state.store(ctx, self.id());
        });

        if !self.opened {
            modal.open();
            self.opened = true;
        }

        self.modal = Some(modal);
        self
    }

    fn dispose(&mut self, ctx: &Context, _state: AppStateRef) {
        State::dispose(ctx, self.id());
    }

    fn is_open(&self) -> bool {
        self.modal.as_ref().is_some_and(|m| m.is_open())
    }
}
