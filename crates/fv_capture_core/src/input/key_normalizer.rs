use device_query::Keycode;

use super::KeyCode;

pub fn keycode_to_key_code(key: Keycode) -> Option<KeyCode> {
    let normalized = match key {
        Keycode::A => KeyCode::Character('A'),
        Keycode::B => KeyCode::Character('B'),
        Keycode::C => KeyCode::Character('C'),
        Keycode::D => KeyCode::Character('D'),
        Keycode::E => KeyCode::Character('E'),
        Keycode::F => KeyCode::Character('F'),
        Keycode::G => KeyCode::Character('G'),
        Keycode::H => KeyCode::Character('H'),
        Keycode::I => KeyCode::Character('I'),
        Keycode::J => KeyCode::Character('J'),
        Keycode::K => KeyCode::Character('K'),
        Keycode::L => KeyCode::Character('L'),
        Keycode::M => KeyCode::Character('M'),
        Keycode::N => KeyCode::Character('N'),
        Keycode::O => KeyCode::Character('O'),
        Keycode::P => KeyCode::Character('P'),
        Keycode::Q => KeyCode::Character('Q'),
        Keycode::R => KeyCode::Character('R'),
        Keycode::S => KeyCode::Character('S'),
        Keycode::T => KeyCode::Character('T'),
        Keycode::U => KeyCode::Character('U'),
        Keycode::V => KeyCode::Character('V'),
        Keycode::W => KeyCode::Character('W'),
        Keycode::X => KeyCode::Character('X'),
        Keycode::Y => KeyCode::Character('Y'),
        Keycode::Z => KeyCode::Character('Z'),
        Keycode::Key0 => KeyCode::Number(0),
        Keycode::Key1 => KeyCode::Number(1),
        Keycode::Key2 => KeyCode::Number(2),
        Keycode::Key3 => KeyCode::Number(3),
        Keycode::Key4 => KeyCode::Number(4),
        Keycode::Key5 => KeyCode::Number(5),
        Keycode::Key6 => KeyCode::Number(6),
        Keycode::Key7 => KeyCode::Number(7),
        Keycode::Key8 => KeyCode::Number(8),
        Keycode::Key9 => KeyCode::Number(9),
        Keycode::F1 => KeyCode::Function(1),
        Keycode::F2 => KeyCode::Function(2),
        Keycode::F3 => KeyCode::Function(3),
        Keycode::F4 => KeyCode::Function(4),
        Keycode::F5 => KeyCode::Function(5),
        Keycode::F6 => KeyCode::Function(6),
        Keycode::F7 => KeyCode::Function(7),
        Keycode::F8 => KeyCode::Function(8),
        Keycode::F9 => KeyCode::Function(9),
        Keycode::F10 => KeyCode::Function(10),
        Keycode::F11 => KeyCode::Function(11),
        Keycode::F12 => KeyCode::Function(12),
        Keycode::F13 => KeyCode::Function(13),
        Keycode::F14 => KeyCode::Function(14),
        Keycode::F15 => KeyCode::Function(15),
        Keycode::F16 => KeyCode::Function(16),
        Keycode::F17 => KeyCode::Function(17),
        Keycode::F18 => KeyCode::Function(18),
        Keycode::F19 => KeyCode::Function(19),
        Keycode::F20 => KeyCode::Function(20),
        Keycode::Escape => KeyCode::Escape,
        Keycode::Space => KeyCode::Space,
        Keycode::LControl | Keycode::RControl => KeyCode::Control,
        Keycode::LShift | Keycode::RShift => KeyCode::Shift,
        Keycode::LAlt | Keycode::RAlt | Keycode::LOption | Keycode::ROption => KeyCode::Alt,
        Keycode::Command | Keycode::RCommand | Keycode::LMeta | Keycode::RMeta => KeyCode::Meta,
        Keycode::Enter | Keycode::NumpadEnter => KeyCode::Enter,
        Keycode::Up => KeyCode::ArrowUp,
        Keycode::Down => KeyCode::ArrowDown,
        Keycode::Left => KeyCode::ArrowLeft,
        Keycode::Right => KeyCode::ArrowRight,
        Keycode::Backspace => KeyCode::Backspace,
        Keycode::Tab => KeyCode::Tab,
        Keycode::Home => KeyCode::Home,
        Keycode::End => KeyCode::End,
        Keycode::PageUp => KeyCode::PageUp,
        Keycode::PageDown => KeyCode::PageDown,
        Keycode::Insert => KeyCode::Insert,
        Keycode::Delete => KeyCode::Delete,
        Keycode::Numpad0 => KeyCode::Number(0),
        Keycode::Numpad1 => KeyCode::Number(1),
        Keycode::Numpad2 => KeyCode::Number(2),
        Keycode::Numpad3 => KeyCode::Number(3),
        Keycode::Numpad4 => KeyCode::Number(4),
        Keycode::Numpad5 => KeyCode::Number(5),
        Keycode::Numpad6 => KeyCode::Number(6),
        Keycode::Numpad7 => KeyCode::Number(7),
        Keycode::Numpad8 => KeyCode::Number(8),
        Keycode::Numpad9 => KeyCode::Number(9),
        other => KeyCode::Unknown(other.to_string()),
    };

    Some(normalized)
}

pub fn normalized_combo(keys: &[KeyCode]) -> Vec<KeyCode> {
    let mut modifiers = Vec::new();
    let mut regular = Vec::new();

    for key in keys {
        match key {
            KeyCode::Control if !modifiers.contains(&KeyCode::Control) => {
                modifiers.push(KeyCode::Control)
            }
            KeyCode::Shift if !modifiers.contains(&KeyCode::Shift) => {
                modifiers.push(KeyCode::Shift)
            }
            KeyCode::Alt if !modifiers.contains(&KeyCode::Alt) => modifiers.push(KeyCode::Alt),
            KeyCode::Meta if !modifiers.contains(&KeyCode::Meta) => modifiers.push(KeyCode::Meta),
            other if !other.is_modifier() => regular.push(other.clone()),
            _ => {}
        }
    }

    modifiers.extend(regular);
    modifiers
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_common_keys_to_language_neutral_codes() {
        assert_eq!(
            keycode_to_key_code(Keycode::LControl),
            Some(KeyCode::Control)
        );
        assert_eq!(
            keycode_to_key_code(Keycode::S),
            Some(KeyCode::Character('S'))
        );
        assert_eq!(keycode_to_key_code(Keycode::F4), Some(KeyCode::Function(4)));
    }

    #[test]
    fn normalizes_combo_modifiers_first_and_deduplicates() {
        let combo = normalized_combo(&[
            KeyCode::Character('S'),
            KeyCode::Shift,
            KeyCode::Control,
            KeyCode::Control,
        ]);

        assert_eq!(
            combo,
            vec![KeyCode::Shift, KeyCode::Control, KeyCode::Character('S')]
        );
    }
}
