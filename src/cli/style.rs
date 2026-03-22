use std::borrow::Cow;
use std::fmt;
use std::io::IsTerminal;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;

// ── Catppuccin Mocha palette (24-bit ANSI) ──────────────────────────────────

const BLUE: &str = "89;180;250"; // #89B4FA  — pack names
const YELLOW: &str = "249;226;175"; // #F9E2AF  — versions
const GREEN: &str = "166;227;161"; // #A6E3A1  — success / ok
const TEAL: &str = "148;226;213"; // #94E2D5  — CLI targets
const OVERLAY: &str = "108;112;134"; // #6C7086  — dim / skipped
const SUBTEXT_COLOR: &str = "166;173;200"; // #A6ADC8  — descriptions
const MAUVE: &str = "203;166;247"; // #CBA6F7  — section headers

// ── Color mode override (set once from --color flag before any output) ──────

// 0 = auto (default), 1 = always, 2 = never
static COLOR_OVERRIDE: AtomicU8 = AtomicU8::new(0);

/// Color mode for the `--color` CLI flag.
#[derive(Clone, Copy, Debug, Default)]
pub enum ColorMode {
    #[default]
    Auto,
    Always,
    Never,
}

impl std::str::FromStr for ColorMode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "auto" => Ok(Self::Auto),
            "always" => Ok(Self::Always),
            "never" => Ok(Self::Never),
            other => Err(format!(
                "invalid color mode '{other}'; expected auto, always, or never"
            )),
        }
    }
}

/// Set the global color mode. Call once in `main()` before any output.
pub fn set_color_mode(mode: ColorMode) {
    let val = match mode {
        ColorMode::Auto => 0,
        ColorMode::Always => 1,
        ColorMode::Never => 2,
    };
    COLOR_OVERRIDE.store(val, Ordering::Relaxed);
}

// ── Detection (cached via OnceLock — one computation per process) ────────────

/// Returns `true` if color output should be used.
///
/// Result is computed once and cached for the process lifetime. The
/// `--color` flag override must be set (via [`set_color_mode`]) before the
/// first call; in practice this means calling it in `main()` before
/// dispatching to any subcommand.
pub fn colors_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        match COLOR_OVERRIDE.load(Ordering::Relaxed) {
            1 => return true,
            2 => return false,
            _ => {}
        }
        should_colorize()
    })
}

fn should_colorize() -> bool {
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    let term = std::env::var("TERM").unwrap_or_default();
    if term.is_empty() || term == "dumb" {
        return false;
    }
    std::io::stdout().is_terminal()
}

// ── Styled wrapper ──────────────────────────────────────────────────────────

/// A lazily-styled text fragment. Renders ANSI escape codes only when
/// [`colors_enabled`] returns `true`; otherwise renders the text unchanged.
pub struct Styled<'a> {
    text: Cow<'a, str>,
    code: Option<&'static str>,
    bold: bool,
}

impl fmt::Display for Styled<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !colors_enabled() {
            return f.write_str(&self.text);
        }
        match (self.bold, self.code) {
            (false, None) => f.write_str(&self.text),
            (true, None) => write!(f, "\x1b[1m{}\x1b[0m", self.text),
            (false, Some(rgb)) => write!(f, "\x1b[38;2;{rgb}m{}\x1b[0m", self.text),
            (true, Some(rgb)) => write!(f, "\x1b[1;38;2;{rgb}m{}\x1b[0m", self.text),
        }
    }
}

fn styled<'a>(text: impl Into<Cow<'a, str>>, code: Option<&'static str>, bold: bool) -> Styled<'a> {
    Styled {
        text: text.into(),
        code,
        bold,
    }
}

// ── Semantic helpers ────────────────────────────────────────────────────────

pub fn pack_name<'a>(text: impl Into<Cow<'a, str>>) -> Styled<'a> {
    styled(text, Some(BLUE), true)
}

pub fn version<'a>(text: impl Into<Cow<'a, str>>) -> Styled<'a> {
    styled(text, Some(YELLOW), true)
}

pub fn success<'a>(text: impl Into<Cow<'a, str>>) -> Styled<'a> {
    styled(text, Some(GREEN), true)
}

pub fn target<'a>(text: impl Into<Cow<'a, str>>) -> Styled<'a> {
    styled(text, Some(TEAL), false)
}

pub fn dim<'a>(text: impl Into<Cow<'a, str>>) -> Styled<'a> {
    styled(text, Some(OVERLAY), false)
}

pub fn subtext<'a>(text: impl Into<Cow<'a, str>>) -> Styled<'a> {
    styled(text, Some(SUBTEXT_COLOR), false)
}

pub fn header<'a>(text: impl Into<Cow<'a, str>>) -> Styled<'a> {
    styled(text, Some(MAUVE), true)
}

pub fn emphasis<'a>(text: impl Into<Cow<'a, str>>) -> Styled<'a> {
    styled(text, None, true)
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn styled_is_plain_text_when_stdout_not_tty() {
        // We don't exercise should_colorize() directly here; instead, we rely on
        // the test harness' default (stdout is not a TTY) and verify that Styled
        // helpers produce plain text with no ANSI escape codes.
        let s = pack_name("webdev");
        assert_eq!(s.to_string(), "webdev");

        let s = version("1.2.3");
        assert_eq!(s.to_string(), "1.2.3");

        let s = success("ok");
        assert_eq!(s.to_string(), "ok");

        let s = dim("skipped");
        assert_eq!(s.to_string(), "skipped");
    }

    #[test]
    fn styled_borrows_str_slice() {
        let name = String::from("webdev");
        // Should compile with &str (borrows, no allocation).
        let s = pack_name(name.as_str());
        assert_eq!(s.to_string(), "webdev");
    }

    #[test]
    fn styled_accepts_owned_string() {
        // Should compile with String (takes ownership, no clone).
        let s = pack_name(String::from("webdev"));
        assert_eq!(s.to_string(), "webdev");
    }
}
