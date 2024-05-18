#[macro_export]
macro_rules! shortcut {
    ($ui:ident, $modifier:ident + $key:ident) => {
        $ui.input_mut(|i| i.consume_key(egui::Modifiers::$modifier, egui::Key::$key))
    };
    ($ui:ident, $key:ident) => {
        $ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::$key))
    };
}
