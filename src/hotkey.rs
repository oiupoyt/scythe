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
                // simple mapping
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
                    _ => None,
                };
            }
        }
    }
    
    if let Some(c) = code {
        Some(HotKey::new(Some(mods), c))
    } else {
        None
    }
}
