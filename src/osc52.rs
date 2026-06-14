//! Write text to the system clipboard via the OSC 52 terminal escape.
//!
//! Works in modern terminals that allow the escape (kitty, wezterm,
//! alacritty >= 0.13, foot, iTerm2, Windows Terminal, recent
//! gnome-terminal, etc). The terminal is responsible for placing the
//! bytes on the user's clipboard; the application just emits them.
//!
//! OSC 52 form: `ESC ] 52 ; c ; <base64> ESC \`
//!   - `c` selects the clipboard (c = system clipboard)
//!   - the base64 body is the payload to copy

use std::io::Write;

const OSC52_START: &[u8] = b"\x1b]52;c;";
const OSC52_END: &[u8] = b"\x1b\\";

fn base64_encode(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((input.len() + 2) / 3 * 4);
    let mut i = 0;
    while i + 3 <= input.len() {
        let n = ((input[i] as u32) << 16)
            | ((input[i + 1] as u32) << 8)
            | (input[i + 2] as u32);
        out.push(ALPHABET[((n >> 18) & 0x3f) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 0x3f) as usize] as char);
        out.push(ALPHABET[((n >> 6) & 0x3f) as usize] as char);
        out.push(ALPHABET[(n & 0x3f) as usize] as char);
        i += 3;
    }
    let rem = input.len() - i;
    if rem == 1 {
        let n = (input[i] as u32) << 16;
        out.push(ALPHABET[((n >> 18) & 0x3f) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 0x3f) as usize] as char);
        out.push('=');
        out.push('=');
    } else if rem == 2 {
        let n = ((input[i] as u32) << 16) | ((input[i + 1] as u32) << 8);
        out.push(ALPHABET[((n >> 18) & 0x3f) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 0x3f) as usize] as char);
        out.push(ALPHABET[((n >> 6) & 0x3f) as usize] as char);
        out.push('=');
    }
    out
}

pub fn copy_to_clipboard(text: &str) -> bool {
    let payload = base64_encode(text.as_bytes());
    let mut out = std::io::stdout().lock();
    out.write_all(OSC52_START).is_ok()
        && out.write_all(payload.as_bytes()).is_ok()
        && out.write_all(OSC52_END).is_ok()
        && out.flush().is_ok()
}
