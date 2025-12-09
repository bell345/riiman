use crate::data::{FieldStore, FieldType, FieldValue, ShortcutAction};
use crate::state::AppStateRef;
use crate::tasks::AsyncTaskResult;
use crate::ui::thumb_grid::TAB_REQUEST_ID;
use eframe::egui;

pub fn handle_shortcuts(state: &AppStateRef, ctx: &egui::Context) {
    let Ok((current_vault, current_item)) = state.current_vault_and_item() else {
        return;
    };

    for (shortcut, behaviour) in current_vault.vm().shortcuts.iter() {
        if ctx.input_mut(|i| i.consume_key(shortcut.modifiers, shortcut.logical_key)) {
            match behaviour.action {
                ShortcutAction::None => {}
                ShortcutAction::ToggleTag(tag_id) => {
                    if current_item.has_field(&tag_id) {
                        current_item.remove_field(&tag_id);
                    } else {
                        match current_vault.get_definition(&tag_id) {
                            Some(def) if def.field_type == FieldType::Tag => {
                                current_item.set_field_value(tag_id, FieldValue::Tag);
                            }
                            _ => {}
                        }
                    }

                    if state.commit_item_catch(None, &current_item, false).is_err() {
                        return;
                    }
                }
            }

            if behaviour.move_next {
                state.add_completed_task(
                    egui::Id::new("main_thumbnail_grid").with(TAB_REQUEST_ID),
                    Ok(AsyncTaskResult::NextItem),
                );
            }
        }
    }
}
