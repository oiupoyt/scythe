use global_hotkey::hotkey::{HotKey, Modifiers, Code};

pub fn parse_hotkey(s: &str) -> Option<HotKey> {
    let mut mods = Modifiers::empty();
    let mut code = None;

    for part in s.split('+') {
        let p = part.trim().to_lowercase();
        match p.as_str() {
            "ctrl" | "control" => mods |= Modifiers::CONTROL,
            "shift" => mods |= Modifiers::SHIFT,
            "alt" => mods |= Modifiers::ALT,
            "meta" | "super" | "windows" | "cmd" => mods |= Modifiers::META,
            _ => {
                code = match p.as_str() {
                    "a" => Some(Code::KeyA),
                    "b" => Some(Code::KeyB),
                    "c" => Some(Code::KeyC),
                    "d" => Some(Code::KeyD),
                    "e" => Some(Code::KeyE),
                    "f" => Some(Code::KeyF),
                    "g" => Some(Code::KeyG),
                    "h" => Some(Code::KeyH),
                    "i" => Some(Code::KeyI),
                    "j" => Some(Code::KeyJ),
                    "k" => Some(Code::KeyK),
                    "l" => Some(Code::KeyL),
                    "m" => Some(Code::KeyM),
                    "n" => Some(Code::KeyN),
                    "o" => Some(Code::KeyO),
                    "p" => Some(Code::KeyP),
                    "q" => Some(Code::KeyQ),
                    "r" => Some(Code::KeyR),
                    "s" => Some(Code::KeyS),
                    "t" => Some(Code::KeyT),
                    "u" => Some(Code::KeyU),
                    "v" => Some(Code::KeyV),
                    "w" => Some(Code::KeyW),
                    "x" => Some(Code::KeyX),
                    "y" => Some(Code::KeyY),
                    "z" => Some(Code::KeyZ),
                    "0" => Some(Code::Digit0),
                    "1" => Some(Code::Digit1),
                    "2" => Some(Code::Digit2),
                    "3" => Some(Code::Digit3),
                    "4" => Some(Code::Digit4),
                    "5" => Some(Code::Digit5),
                    "6" => Some(Code::Digit6),
                    "7" => Some(Code::Digit7),
                    "8" => Some(Code::Digit8),
                    "9" => Some(Code::Digit9),
                    "f1" => Some(Code::F1),
                    "f2" => Some(Code::F2),
                    "f3" => Some(Code::F3),
                    "f4" => Some(Code::F4),
                    "f5" => Some(Code::F5),
                    "f6" => Some(Code::F6),
                    "f7" => Some(Code::F7),
                    "f8" => Some(Code::F8),
                    "f9" => Some(Code::F9),
                    "f10" => Some(Code::F10),
                    "f11" => Some(Code::F11),
                    "f12" => Some(Code::F12),
                    "space" => Some(Code::Space),
                    "escape" | "esc" => Some(Code::Escape),
                    "tab" => Some(Code::Tab),
                    "enter" | "return" => Some(Code::Enter),
                    "backspace" => Some(Code::Backspace),
                    "insert" => Some(Code::Insert),
                    "delete" | "del" => Some(Code::Delete),
                    "home" => Some(Code::Home),
                    "end" => Some(Code::End),
                    "pageup" | "pgup" | "page_up" => Some(Code::PageUp),
                    "pagedown" | "pgdn" | "page_down" => Some(Code::PageDown),
                    _ => None,
                };
            }
        }
    }

    code.map(|c| HotKey::new(Some(mods), c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_hotkey_combinations() {
        assert!(parse_hotkey("Ctrl+Shift+R").is_some());
        assert!(parse_hotkey("Alt+Z").is_some());
        assert!(parse_hotkey("F12").is_some());
        assert!(parse_hotkey("Ctrl+Alt+Delete").is_some());
        assert!(parse_hotkey("super+space").is_some());
        assert!(parse_hotkey("").is_none());
        assert!(parse_hotkey("invalid_key_xyz").is_none());
    }
}

