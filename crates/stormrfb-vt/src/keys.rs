//! Linux evdev key codes (`<linux/input-event-codes.h>`) to what RFB sends:
//! the XT scancode qemu's Extended Key Event takes, and an X11 keysym for
//! servers without it (stormvm's own display, `stormvm display`).
//!
//! Scancodes are layout-free: the guest applies its own keymap. Keysyms are
//! not, so they come from a US layout here, which is what a keysym-only
//! server needs to type anything at all.

use stormrfb::qemu_keycode;

/// evdev codes the viewer looks at by name.
pub const KEY_LEFTCTRL: u16 = 29;
pub const KEY_LEFTSHIFT: u16 = 42;
pub const KEY_RIGHTSHIFT: u16 = 54;
pub const KEY_LEFTALT: u16 = 56;
pub const KEY_CAPSLOCK: u16 = 58;
pub const KEY_RIGHTCTRL: u16 = 97;
pub const KEY_RIGHTALT: u16 = 100;

pub const BTN_LEFT: u16 = 0x110;
pub const BTN_RIGHT: u16 = 0x111;
pub const BTN_MIDDLE: u16 = 0x112;
pub const BTN_TOUCH: u16 = 0x14a;

/// The RFB button mask bit of a pointer button code, if it is one.
/// A tablet's touch is a left click.
pub fn button(code: u16) -> Option<u8> {
    match code {
        BTN_LEFT | BTN_TOUCH => Some(1),
        BTN_MIDDLE => Some(2),
        BTN_RIGHT => Some(4),
        _ => None,
    }
}

/// The VT an F-key switches to (F1 → 1 … F12 → 12).
pub fn function_key(code: u16) -> Option<u16> {
    match code {
        59..=68 => Some(code - 58),
        87 => Some(11),
        88 => Some(12),
        _ => None,
    }
}

/// qemu's keycode for an evdev key: the XT make code, bit 7 for an 0xE0
/// prefix ([`qemu_keycode`]). evdev codes 1–88 are the XT codes themselves.
pub fn scancode(code: u16) -> Option<u32> {
    let (sc, ext) = match code {
        1..=88 => (code as u8, false),
        117 => (0x59, false), // KP =
        96 => (0x1c, true),   // KP Enter
        97 => (0x1d, true),   // Right Ctrl
        98 => (0x35, true),   // KP /
        99 => (0x37, true),   // SysRq / Print
        100 => (0x38, true),  // Right Alt
        102 => (0x47, true),  // Home
        103 => (0x48, true),  // Up
        104 => (0x49, true),  // Page Up
        105 => (0x4b, true),  // Left
        106 => (0x4d, true),  // Right
        107 => (0x4f, true),  // End
        108 => (0x50, true),  // Down
        109 => (0x51, true),  // Page Down
        110 => (0x52, true),  // Insert
        111 => (0x53, true),  // Delete
        113 => (0x20, true),  // Mute
        114 => (0x2e, true),  // Volume Down
        115 => (0x30, true),  // Volume Up
        116 => (0x5e, true),  // Power
        119 => (0x46, true),  // Pause (qemu's number for it)
        125 => (0x5b, true),  // Left Meta
        126 => (0x5c, true),  // Right Meta
        127 => (0x5d, true),  // Compose / Menu
        _ => return None,
    };
    qemu_keycode(sc, ext)
}

/// The US-layout keysym of an evdev key, shifted or not. 0 when it has none.
pub fn keysym(code: u16, shift: bool) -> u32 {
    const ROW: [(u16, u8, u8); 47] = [
        (2, b'1', b'!'),
        (3, b'2', b'@'),
        (4, b'3', b'#'),
        (5, b'4', b'$'),
        (6, b'5', b'%'),
        (7, b'6', b'^'),
        (8, b'7', b'&'),
        (9, b'8', b'*'),
        (10, b'9', b'('),
        (11, b'0', b')'),
        (12, b'-', b'_'),
        (13, b'=', b'+'),
        (16, b'q', b'Q'),
        (17, b'w', b'W'),
        (18, b'e', b'E'),
        (19, b'r', b'R'),
        (20, b't', b'T'),
        (21, b'y', b'Y'),
        (22, b'u', b'U'),
        (23, b'i', b'I'),
        (24, b'o', b'O'),
        (25, b'p', b'P'),
        (26, b'[', b'{'),
        (27, b']', b'}'),
        (30, b'a', b'A'),
        (31, b's', b'S'),
        (32, b'd', b'D'),
        (33, b'f', b'F'),
        (34, b'g', b'G'),
        (35, b'h', b'H'),
        (36, b'j', b'J'),
        (37, b'k', b'K'),
        (38, b'l', b'L'),
        (39, b';', b':'),
        (40, b'\'', b'"'),
        (41, b'`', b'~'),
        (43, b'\\', b'|'),
        (44, b'z', b'Z'),
        (45, b'x', b'X'),
        (46, b'c', b'C'),
        (47, b'v', b'V'),
        (48, b'b', b'B'),
        (49, b'n', b'N'),
        (50, b'm', b'M'),
        (51, b',', b'<'),
        (52, b'.', b'>'),
        (53, b'/', b'?'),
    ];
    if let Some(&(_, lo, hi)) = ROW.iter().find(|r| r.0 == code) {
        return u32::from(if shift { hi } else { lo });
    }
    match code {
        1 => 0xff1b,                              // Escape
        14 => 0xff08,                             // BackSpace
        15 if shift => 0xfe20,                    // ISO_Left_Tab
        15 => 0xff09,                             // Tab
        28 => 0xff0d,                             // Return
        29 => 0xffe3,                             // Control_L
        42 => 0xffe1,                             // Shift_L
        54 => 0xffe2,                             // Shift_R
        55 => 0xffaa,                             // KP_Multiply
        56 => 0xffe9,                             // Alt_L
        57 => 0x20,                               // space
        58 => 0xffe5,                             // Caps_Lock
        59..=68 => 0xffbe + u32::from(code - 59), // F1–F10
        69 => 0xff7f,                             // Num_Lock
        70 => 0xff14,                             // Scroll_Lock
        71 => 0xffb7,                             // KP_7
        72 => 0xffb8,
        73 => 0xffb9,
        74 => 0xffad, // KP_Subtract
        75 => 0xffb4,
        76 => 0xffb5,
        77 => 0xffb6,
        78 => 0xffab, // KP_Add
        79 => 0xffb1,
        80 => 0xffb2,
        81 => 0xffb3,
        82 => 0xffb0,
        83 => 0xffae, // KP_Decimal
        86 if shift => u32::from(b'>'),
        86 => u32::from(b'<'), // the ISO key next to left Shift
        87 => 0xffc8,          // F11
        88 => 0xffc9,          // F12
        96 => 0xff8d,          // KP_Enter
        97 => 0xffe4,          // Control_R
        98 => 0xffaf,          // KP_Divide
        99 => 0xff61,          // Print
        100 => 0xffea,         // Alt_R
        102 => 0xff50,         // Home
        103 => 0xff52,         // Up
        104 => 0xff55,         // Prior
        105 => 0xff51,         // Left
        106 => 0xff53,         // Right
        107 => 0xff57,         // End
        108 => 0xff54,         // Down
        109 => 0xff56,         // Next
        110 => 0xff63,         // Insert
        111 => 0xffff,         // Delete
        113 => 0x1008_ff12,    // XF86AudioMute
        114 => 0x1008_ff11,    // XF86AudioLowerVolume
        115 => 0x1008_ff13,    // XF86AudioRaiseVolume
        116 => 0x1008_ff2a,    // XF86PowerOff
        117 => 0xffbd,         // KP_Equal
        119 => 0xff13,         // Pause
        125 => 0xffeb,         // Super_L
        126 => 0xffec,         // Super_R
        127 => 0xff67,         // Menu
        _ => 0,
    }
}

/// Whether the key's keysym is a letter, which Caps Lock shifts.
pub fn is_letter(code: u16) -> bool {
    matches!(keysym(code, false), 0x61..=0x7a)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scancodes_are_xt_with_the_e0_bit() {
        assert_eq!(scancode(30), Some(0x1e)); // A
        assert_eq!(scancode(28), Some(0x1c)); // Enter
        assert_eq!(scancode(96), Some(0x9c)); // KP Enter: E0 1C
        assert_eq!(scancode(103), Some(0xc8)); // Up: E0 48
        assert_eq!(scancode(125), Some(0xdb)); // Left Meta: E0 5B
        assert_eq!(scancode(119), Some(0xc6)); // Pause
        assert_eq!(scancode(0), None);
        assert_eq!(scancode(200), None);
    }

    #[test]
    fn keysyms_follow_a_us_layout() {
        assert_eq!(keysym(30, false), u32::from(b'a'));
        assert_eq!(keysym(30, true), u32::from(b'A'));
        assert_eq!(keysym(3, true), u32::from(b'@'));
        assert_eq!(keysym(28, false), 0xff0d);
        assert_eq!(keysym(59, false), 0xffbe);
        assert_eq!(keysym(68, false), 0xffc7);
        assert_eq!(keysym(88, false), 0xffc9);
        assert!(is_letter(30) && !is_letter(2));
    }

    #[test]
    fn every_scancode_key_but_the_ro_block_has_a_keysym() {
        for code in 1..=127u16 {
            if scancode(code).is_some() && !(84..=85).contains(&code) {
                assert_ne!(keysym(code, false), 0, "evdev {code}");
            }
        }
    }

    #[test]
    fn function_keys_name_their_vt() {
        assert_eq!(function_key(59), Some(1));
        assert_eq!(function_key(63), Some(5));
        assert_eq!(function_key(64), Some(6));
        assert_eq!(function_key(87), Some(11));
        assert_eq!(function_key(30), None);
    }
}
