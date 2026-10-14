//! Shortcut strings (`Cmd+Shift+Z`, `Delete`, `.`) as egui shortcuts. `Cmd` is ⌘ on macOS and
//! Ctrl elsewhere, as in the command registry.

use egui::{Key, KeyboardShortcut, Modifiers};

/// Parses a shortcut. Unknown keys give `None` (the shortcut is shown but not bound).
pub fn parse(text: &str) -> Option<KeyboardShortcut> {
    let mut mods = Modifiers::NONE;
    let mut key = None;
    // `.` and `+` are keys too, so split on '+' only between parts.
    let parts: Vec<&str> = if text == "+" { vec!["+"] } else { text.split('+').collect() };
    for part in parts {
        match part.trim() {
            "Cmd" | "Command" => mods |= Modifiers::COMMAND,
            "Ctrl" => mods |= Modifiers::CTRL,
            "Shift" => mods |= Modifiers::SHIFT,
            "Alt" | "Option" => mods |= Modifiers::ALT,
            k => key = Key::from_name(k),
        }
    }
    key.map(|k| KeyboardShortcut::new(mods, k))
}

/// How many modifiers a shortcut uses (longer shortcuts are matched first, so `Cmd+Shift+Z`
/// wins over `Cmd+Z`).
pub fn modifier_count(s: &KeyboardShortcut) -> usize {
    let m = s.modifiers;
    [m.command || m.ctrl || m.mac_cmd, m.shift, m.alt].iter().filter(|b| **b).count()
}

/// Whether a shortcut is a bare key (no Cmd/Ctrl/Alt), which must not fire while typing.
pub fn is_bare(s: &KeyboardShortcut) -> bool {
    let m = s.modifiers;
    !(m.command || m.ctrl || m.mac_cmd || m.alt)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_registry_shortcuts() {
        let s = parse("Cmd+Shift+Z").unwrap();
        assert!(s.modifiers.command && s.modifiers.shift);
        assert_eq!(s.logical_key, Key::Z);
        assert_eq!(parse(".").unwrap().logical_key, Key::Period);
        assert_eq!(parse("Delete").unwrap().logical_key, Key::Delete);
        assert_eq!(parse("F7").unwrap().logical_key, Key::F7);
        assert!(parse("Cmd+Nonsense").is_none());
        assert_eq!(modifier_count(&parse("Cmd+Shift+S").unwrap()), 2);
        assert!(is_bare(&parse("D").unwrap()));
        assert!(!is_bare(&parse("Cmd+S").unwrap()));
    }
}
