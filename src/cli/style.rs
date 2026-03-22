use std::fmt::{Display, Formatter, Result as FmtResult};
use std::io::IsTerminal;

/// Catppuccin Mocha-inspired 24-bit ANSI colors for CLI output.
const PACK_NAME: &str = "89;180;250"; // #89B4FA (blue)
const VERSION: &str = "249;226;175"; // #F9E2AF (yellow)
const SUCCESS: &str = "166;227;161"; // #A6E3A1 (green)
const TARGET: &str = "148;226;213"; // #94E2D5 (teal)
const DIM: &str = "108;112;134"; // #6C7086 (overlay)
const SUBTEXT: &str = "166;173;200"; // #A6ADC8 (subtext)
const HEADER: &str = "203;166;247"; // #CBA6F7 (mauve)

pub struct Styled {
    text: String,
    code: Option<&'static str>,
    bold: bool,
}

impl Display for Styled {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        if !colors_enabled() {
            return write!(f, "{}", self.text);
        }

        match (self.bold, self.code) {
            (false, None) => write!(f, "{}", self.text),
            (true, None) => write!(f, "\x1b[1m{}\x1b[0m", self.text),
            (false, Some(code)) => write!(f, "\x1b[38;2;{code}m{}\x1b[0m", self.text),
            (true, Some(code)) => write!(f, "\x1b[1;38;2;{code}m{}\x1b[0m", self.text),
        }
    }
}

fn styled(text: impl Into<String>, code: Option<&'static str>, bold: bool) -> Styled {
    Styled {
        text: text.into(),
        code,
        bold,
    }
}

pub fn pack_name(text: impl Into<String>) -> Styled {
    styled(text, Some(PACK_NAME), true)
}

pub fn version(text: impl Into<String>) -> Styled {
    styled(text, Some(VERSION), true)
}

pub fn success(text: impl Into<String>) -> Styled {
    styled(text, Some(SUCCESS), true)
}

pub fn target(text: impl Into<String>) -> Styled {
    styled(text, Some(TARGET), false)
}

pub fn dim(text: impl Into<String>) -> Styled {
    styled(text, Some(DIM), false)
}

pub fn subtext(text: impl Into<String>) -> Styled {
    styled(text, Some(SUBTEXT), false)
}

pub fn header(text: impl Into<String>) -> Styled {
    styled(text, Some(HEADER), true)
}

pub fn emphasis(text: impl Into<String>) -> Styled {
    styled(text, None, true)
}

pub fn colors_enabled() -> bool {
    let no_color = std::env::var_os("NO_COLOR").is_some();
    let term = std::env::var_os("TERM")
        .and_then(|term| term.into_string().ok())
        .unwrap_or_default();
    let is_terminal = std::io::stdout().is_terminal();
    should_enable_colors(no_color, &term, is_terminal)
}

fn should_enable_colors(no_color: bool, term: &str, is_terminal: bool) -> bool {
    if no_color {
        return false;
    }
    is_terminal && !term.is_empty() && term != "dumb"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_decision_respects_no_color_and_term() {
        assert!(!should_enable_colors(true, "xterm-256color", true));
        assert!(!should_enable_colors(false, "", true));
        assert!(!should_enable_colors(false, "dumb", true));
        assert!(!should_enable_colors(false, "xterm-256color", false));
        assert!(should_enable_colors(false, "xterm-256color", true));
    }
}
