#[macro_export]
macro_rules! take_shortcut {
    ($ui:ident, $modifier:ident + $key:ident) => {
        $ui.input_mut(|i| i.consume_key(egui::Modifiers::$modifier, egui::Key::$key))
    };
    ($ui:ident, $key:ident) => {
        $ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::$key))
    };
}

#[macro_export]
macro_rules! peek_shortcut {
    ($ui:ident, $modifier:ident + $key:ident) => {
        $ui.input(|i| i.events.iter().any(|e| matches!(e, egui::data::input::Event::Key {
            key: ev_key,
            modifiers: ev_mods,
            pressed: true,
            ..
        } if *ev_key == egui::Key::$key && ev_mods.matches_logically(egui::Modifiers::$modifier))))
    };
    ($ui:ident, $key:ident) => {
        $ui.input(|i| i.events.iter().any(|e| matches!(e, egui::data::input::Event::Key {
            key: ev_key,
            modifiers: ev_mods,
            pressed: true,
            ..
        } if *ev_key == egui::Key::$key && ev_mods.matches_logically(egui::Modifiers::NONE))))
    };
}

pub fn update_index(
    index: Option<usize>,
    down_pressed: bool,
    up_pressed: bool,
    match_results_count: usize,
    max_suggestions: usize,
) -> Option<usize> {
    match index {
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
