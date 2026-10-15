//! Terminal-escape sanitization for server-supplied strings
//! (ADR-0047, "Decided: Output, Exit Codes, Errors").

/// Escapes `input` so it can never inject terminal control sequences
/// when printed (ADR-0047, "Decided: Output, Exit Codes, Errors" —
/// "Server-supplied strings are printed escaped so a hostile endpoint
/// cannot inject terminal control sequences"). Every ASCII control
/// character (`0x00..=0x1F`, `0x7F`) — which includes `ESC` (`0x1B`,
/// the prefix every ANSI escape sequence needs) — becomes a visible
/// `\xNN` hex escape; a literal backslash is escaped too, so the
/// output is unambiguous about which `\x..` sequences are this
/// function's own escaping versus bytes that were already literally
/// `\`, `x`, and two hex digits in the original string. Everything
/// else, including non-ASCII UTF-8 text, passes through unchanged —
/// this is a terminal-safety filter, not an ASCII-only one.
#[must_use]
pub fn sanitize_for_terminal(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for character in input.chars() {
        match character {
            '\\' => out.push_str("\\\\"),
            control if control.is_control() => {
                out.push_str(&format!("\\x{:02x}", control as u32));
            }
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::sanitize_for_terminal;

    #[test]
    fn passes_through_ordinary_text_unchanged() {
        assert_eq!(
            sanitize_for_terminal("tx_decode_failed, height=5"),
            "tx_decode_failed, height=5"
        );
    }

    #[test]
    fn escapes_the_ansi_escape_character() {
        assert_eq!(
            sanitize_for_terminal("\x1b[31mred\x1b[0m"),
            "\\x1b[31mred\\x1b[0m"
        );
    }

    #[test]
    fn escapes_every_c0_control_character_and_del() {
        assert_eq!(sanitize_for_terminal("\x00"), "\\x00");
        assert_eq!(sanitize_for_terminal("\x07"), "\\x07");
        assert_eq!(sanitize_for_terminal("\x7f"), "\\x7f");
    }

    #[test]
    fn escapes_a_literal_backslash_so_output_stays_unambiguous() {
        assert_eq!(sanitize_for_terminal("a\\x1bb"), "a\\\\x1bb");
    }

    #[test]
    fn passes_through_non_ascii_text_unchanged() {
        assert_eq!(sanitize_for_terminal("héllo"), "héllo");
    }
}
