//! Safe shell-quoting for strings that get injected into a PTY running a
//! POSIX shell (bash/zsh/dash).
//!
//! The primary caller is `atn_pty::writer::canned_action_to_bytes`, which
//! formats `coord read <page>` / `coord ack <request_id>` lines from
//! agent-supplied data. Without quoting, values that contain shell
//! metacharacters (`()`, whitespace, `<`, `>`, `$`, `|`, `&`, `;`, `"`,
//! `'`, `\`, backticks, newlines) can either break the shell's parse
//! (the `(priority: High)` issue) or smuggle in injection.

/// Wrap `s` in single quotes, escaping any interior single quotes with
/// the classic `'\''` dance. The result is always a single POSIX-shell
/// word that the shell parser will interpret verbatim.
///
/// Works for every byte in the input, including control characters and
/// unicode — single-quoted strings preserve everything except `'` itself.
pub fn shell_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for ch in s.chars() {
        if ch == '\'' {
            // End the quoted section, insert an escaped quote, start a new one.
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_string_becomes_empty_quotes() {
        assert_eq!(shell_escape(""), "''");
    }

    #[test]
    fn plain_word_gets_quoted() {
        assert_eq!(shell_escape("hello"), "'hello'");
    }

    #[test]
    fn parentheses_and_colon() {
        // The canonical regression case: `(priority: High)` no longer
        // triggers a bash subshell parse.
        assert_eq!(shell_escape("(priority: High)"), "'(priority: High)'");
    }

    #[test]
    fn interior_single_quote_uses_classic_dance() {
        assert_eq!(shell_escape("can't"), "'can'\\''t'");
    }

    #[test]
    fn only_single_quote() {
        assert_eq!(shell_escape("'"), "''\\'''");
    }

    #[test]
    fn metacharacters_pass_through_literally() {
        let specials = "foo <bar> $PATH | `id` & ; \"baz\" \\escape";
        let escaped = shell_escape(specials);
        assert!(escaped.starts_with('\'') && escaped.ends_with('\''));
        // Nothing was doubled except the one delimiter.
        assert_eq!(escaped, format!("'{}'", specials));
    }

    #[test]
    fn newline_preserved() {
        assert_eq!(shell_escape("line1\nline2"), "'line1\nline2'");
    }

    #[test]
    fn unicode_preserved() {
        assert_eq!(shell_escape("héllo 🚀"), "'héllo 🚀'");
    }

    #[test]
    fn multiple_single_quotes_interleaved() {
        assert_eq!(shell_escape("a'b'c"), "'a'\\''b'\\''c'");
    }

    #[test]
    fn result_is_always_a_single_shell_word() {
        // Anything quoted with this helper should be a single POSIX
        // shell token — i.e. parse as one argument via the `sh -c` path.
        let cases = [
            "",
            "simple",
            "with spaces",
            "(priority: High)",
            "has 'inner' quote",
            "$var `cmd` | pipe",
        ];
        for c in cases {
            let quoted = shell_escape(c);
            // Not strict POSIX verification, but: opens and closes with
            // `'`, and has no bare `'` in the middle.
            assert!(quoted.starts_with('\''));
            assert!(quoted.ends_with('\''));
            // Between the outer quotes, every inner `'` must be part of
            // the `'\''` sequence. Stripping the outer quotes should
            // leave only valid fragments.
            let inner = &quoted[1..quoted.len() - 1];
            // Re-split on the dance sequence; the remaining pieces must
            // contain no unescaped `'`.
            for piece in inner.split("'\\''") {
                assert!(!piece.contains('\''), "unescaped quote in {quoted:?}");
            }
        }
    }
}
