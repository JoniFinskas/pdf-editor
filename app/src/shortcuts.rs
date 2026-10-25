use eframe::egui::{Context, Key, KeyboardShortcut, Modifiers};

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Tools,
    GoToPage,
    Open,
    Save,
    SaveAs,
    Undo,
    Redo,
    Find,
    ZoomIn,
    ZoomOut,
    ActualSize,
}

pub fn consume(ctx: &Context, editing: bool) -> Vec<Command> {
    let pressed = |m, k| ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(m, k)));
    let mut out = Vec::new();
    let ctrl = Modifiers::COMMAND;
    let shift = ctrl | Modifiers::SHIFT;
    if pressed(shift, Key::S) {
        out.push(Command::SaveAs);
    }
    for (key, command) in [
        (Key::G, Command::GoToPage),
        (Key::K, Command::Tools),
        (Key::O, Command::Open),
        (Key::S, Command::Save),
        (Key::F, Command::Find),
        (Key::Minus, Command::ZoomOut),
        (Key::Num0, Command::ActualSize),
    ] {
        if pressed(ctrl, key) {
            out.push(command);
        }
    }
    if pressed(ctrl, Key::Plus)
        || pressed(ctrl, Key::Equals)
        || pressed(shift, Key::Plus)
        || pressed(shift, Key::Equals)
    {
        out.push(Command::ZoomIn);
    }
    // Active text fields own their text undo stack.
    if !editing {
        if pressed(shift, Key::Z) || pressed(ctrl, Key::Y) {
            out.push(Command::Redo);
        }
        if pressed(ctrl, Key::Z) {
            out.push(Command::Undo);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    fn dispatch(key: Key, modifiers: Modifiers, editing: bool) -> Vec<Command> {
        let ctx = Context::default();
        let mut commands = vec![];
        let input = eframe::egui::RawInput {
            events: vec![eframe::egui::Event::Key {
                key,
                physical_key: Some(key),
                pressed: true,
                repeat: false,
                modifiers,
            }],
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| commands = consume(ui.ctx(), editing));
        output.textures_delta.clear();
        commands
    }
    #[test]
    fn required_shortcuts_dispatch_and_preserve_text_undo() {
        for (key, command) in [
            (Key::G, Command::GoToPage),
            (Key::K, Command::Tools),
            (Key::O, Command::Open),
            (Key::S, Command::Save),
            (Key::Z, Command::Undo),
            (Key::Y, Command::Redo),
            (Key::F, Command::Find),
            (Key::Plus, Command::ZoomIn),
            (Key::Minus, Command::ZoomOut),
        ] {
            assert_eq!(dispatch(key, Modifiers::COMMAND, false), vec![command]);
            assert!(dispatch(key, Modifiers::NONE, false).is_empty());
        }
        assert!(dispatch(Key::Z, Modifiers::COMMAND, true).is_empty());
        assert_eq!(
            dispatch(Key::S, Modifiers::COMMAND | Modifiers::SHIFT, false),
            vec![Command::SaveAs]
        );
        assert_eq!(
            dispatch(Key::Equals, Modifiers::COMMAND | Modifiers::SHIFT, false),
            vec![Command::ZoomIn]
        );
        assert_eq!(
            dispatch(Key::Z, Modifiers::COMMAND | Modifiers::SHIFT, false),
            vec![Command::Redo]
        );
    }
}
