//! Keybind string parsing and hotkey polling via GetAsyncKeyState.
//!
//! Keybind strings follow the format "Modifier+Key", e.g.:
//! - "F12" (single key)
//! - "Ctrl+F9" (modifier + key)
//! - "Ctrl+Shift+H" (multiple modifiers + key)

use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};

/// A parsed hotkey: modifier flags + a virtual key code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hotkey {
    pub vk: u16,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
}

/// State tracker that detects key-down edges (press, not hold).
pub struct HotkeyPoller {
    hotkey: Hotkey,
    was_pressed: bool,
}

impl HotkeyPoller {
    pub fn new(hotkey: Hotkey) -> Self {
        Self {
            hotkey,
            was_pressed: false,
        }
    }

    /// Update the hotkey being polled (e.g., when config changes).
    pub fn set_hotkey(&mut self, hotkey: Hotkey) {
        self.hotkey = hotkey;
        self.was_pressed = false;
    }

    /// Returns `true` on the rising edge (key just pressed, not held).
    /// Call this each iteration of the host main loop.
    pub fn poll(&mut self) -> bool {
        let pressed = self.is_combo_pressed();
        let triggered = pressed && !self.was_pressed;
        self.was_pressed = pressed;
        triggered
    }

    fn is_combo_pressed(&self) -> bool {
        if self.hotkey.ctrl && !is_key_down(VK_CONTROL.0) {
            return false;
        }
        if self.hotkey.alt && !is_key_down(VK_MENU.0) {
            return false;
        }
        if self.hotkey.shift && !is_key_down(VK_SHIFT.0) {
            return false;
        }
        if self.hotkey.win && !(is_key_down(VK_LWIN.0) || is_key_down(VK_RWIN.0)) {
            return false;
        }
        is_key_down(self.hotkey.vk)
    }
}

fn is_key_down(vk: u16) -> bool {
    (unsafe { GetAsyncKeyState(vk as i32) } as u16) & 0x8000 != 0
}

/// Parse a keybind string like "F12" or "Ctrl+Shift+F9" into a Hotkey.
/// Returns None if the key name is unrecognized.
pub fn parse_keybind(s: &str) -> Option<Hotkey> {
    let mut ctrl = false;
    let mut alt = false;
    let mut shift = false;
    let mut win = false;
    let mut main_key = None;

    for part in s.split('+') {
        let part = part.trim();
        match part.to_lowercase().as_str() {
            "ctrl" | "control" => ctrl = true,
            "alt" => alt = true,
            "shift" => shift = true,
            "meta" | "win" | "super" | "cmd" => win = true,
            _ => {
                main_key = Some(key_name_to_vk(part)?);
            }
        }
    }

    let vk = main_key?;
    Some(Hotkey {
        vk,
        ctrl,
        alt,
        shift,
        win,
    })
}

/// Map a key name string to a Windows virtual key code.
fn key_name_to_vk(name: &str) -> Option<u16> {
    let upper = name.to_uppercase();
    if let Some(n) = upper.strip_prefix('F') {
        if let Ok(num) = n.parse::<u16>() {
            if (1..=24).contains(&num) {
                return Some(0x70 + num - 1);
            }
        }
    }

    match upper.as_str() {
        "ESCAPE" | "ESC" => Some(0x1B),
        "TAB" => Some(0x09),
        "SPACE" => Some(0x20),
        "ENTER" | "RETURN" => Some(0x0D),
        "BACKSPACE" => Some(0x08),
        "DELETE" | "DEL" => Some(0x2E),
        "INSERT" | "INS" => Some(0x2D),
        "HOME" => Some(0x24),
        "END" => Some(0x23),
        "PAGEUP" => Some(0x21),
        "PAGEDOWN" => Some(0x22),
        "UP" | "ARROWUP" => Some(0x26),
        "DOWN" | "ARROWDOWN" => Some(0x28),
        "LEFT" | "ARROWLEFT" => Some(0x25),
        "RIGHT" | "ARROWRIGHT" => Some(0x27),
        "CAPSLOCK" => Some(0x14),
        "NUMLOCK" => Some(0x90),
        "SCROLLLOCK" => Some(0x91),
        "PRINTSCREEN" => Some(0x2C),
        "PAUSE" => Some(0x13),
        "NUMPAD0" => Some(0x60),
        "NUMPAD1" => Some(0x61),
        "NUMPAD2" => Some(0x62),
        "NUMPAD3" => Some(0x63),
        "NUMPAD4" => Some(0x64),
        "NUMPAD5" => Some(0x65),
        "NUMPAD6" => Some(0x66),
        "NUMPAD7" => Some(0x67),
        "NUMPAD8" => Some(0x68),
        "NUMPAD9" => Some(0x69),
        // OEM keys (US keyboard layout)
        ";" | ":" => Some(0xBA),  // VK_OEM_1
        "=" | "+" => Some(0xBB),  // VK_OEM_PLUS
        "," | "<" => Some(0xBC),  // VK_OEM_COMMA
        "-" | "_" => Some(0xBD),  // VK_OEM_MINUS
        "." | ">" => Some(0xBE),  // VK_OEM_PERIOD
        "/" | "?" => Some(0xBF),  // VK_OEM_2
        "`" | "~" => Some(0xC0),  // VK_OEM_3
        "[" | "{" => Some(0xDB),  // VK_OEM_4
        "\\" | "|" => Some(0xDC), // VK_OEM_5
        "]" | "}" => Some(0xDD),  // VK_OEM_6
        "'" | "\"" => Some(0xDE), // VK_OEM_7
        _ => {
            let bytes = upper.as_bytes();
            if bytes.len() == 1 {
                let b = bytes[0];
                if b.is_ascii_alphanumeric() {
                    return Some(b as u16);
                }
            }
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_single_key() {
        let hk = parse_keybind("F12").unwrap();
        assert_eq!(hk.vk, 0x7B);
        assert!(!hk.ctrl);
        assert!(!hk.alt);
        assert!(!hk.shift);
    }

    #[test]
    fn parse_modifier_combo() {
        let hk = parse_keybind("Ctrl+Shift+F9").unwrap();
        assert_eq!(hk.vk, 0x78);
        assert!(hk.ctrl);
        assert!(!hk.alt);
        assert!(hk.shift);
    }

    #[test]
    fn parse_letter_key() {
        let hk = parse_keybind("Ctrl+H").unwrap();
        assert_eq!(hk.vk, 0x48);
        assert!(hk.ctrl);
    }

    #[test]
    fn parse_unknown_returns_none() {
        assert!(parse_keybind("FooBar").is_none());
    }

    #[test]
    fn parse_case_insensitive() {
        let hk = parse_keybind("ctrl+f12").unwrap();
        assert!(hk.ctrl);
        assert_eq!(hk.vk, 0x7B);
    }

    #[test]
    fn parse_oem_keys() {
        let hk = parse_keybind("Ctrl+Shift+?").unwrap();
        assert_eq!(hk.vk, 0xBF);
        assert!(hk.ctrl);
        assert!(hk.shift);

        assert_eq!(parse_keybind("/").unwrap().vk, 0xBF);
        assert_eq!(parse_keybind(";").unwrap().vk, 0xBA);
        assert_eq!(parse_keybind("[").unwrap().vk, 0xDB);
        assert_eq!(parse_keybind("`").unwrap().vk, 0xC0);
    }
}
