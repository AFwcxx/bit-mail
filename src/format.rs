use std::{
    io::{self, IsTerminal},
    sync::OnceLock,
    time::{SystemTime, UNIX_EPOCH},
};

const BRAND_COLOR: &str = "36";

pub fn enabled() -> bool {
    color_enabled(
        is_terminal(),
        std::env::var("TERM").ok().as_deref(),
        std::env::var_os("NO_COLOR").is_some(),
    )
}

pub fn is_terminal() -> bool {
    terminal_supported(
        io::stdout().is_terminal(),
        std::env::var("TERM").ok().as_deref(),
    )
}

pub fn stderr_enabled() -> bool {
    color_enabled(
        io::stderr().is_terminal(),
        std::env::var("TERM").ok().as_deref(),
        std::env::var_os("NO_COLOR").is_some(),
    )
}

fn color_enabled(terminal: bool, term: Option<&str>, no_color: bool) -> bool {
    terminal && !no_color && term != Some("dumb")
}

fn terminal_supported(terminal: bool, term: Option<&str>) -> bool {
    terminal && term != Some("dumb")
}

pub fn cyan(value: impl AsRef<str>, color: bool) -> String {
    paint(value.as_ref(), BRAND_COLOR, color)
}
pub fn yellow(value: impl AsRef<str>, color: bool) -> String {
    paint(value.as_ref(), "33", color)
}
pub fn red(value: impl AsRef<str>, color: bool) -> String {
    paint(value.as_ref(), "31", color)
}
pub fn result(value: impl AsRef<str>) -> String {
    cyan(value, enabled())
}

pub fn error(value: impl AsRef<str>) -> String {
    red(value, stderr_enabled())
}

pub fn number(value: impl std::fmt::Display, color: bool) -> String {
    cyan(value.to_string(), color)
}

fn paint(value: &str, code: &str, color: bool) -> String {
    if color {
        format!("\x1b[{code}m{value}\x1b[0m")
    } else {
        value.into()
    }
}

pub fn panel(title: &str, lines: &[String], color: bool) -> String {
    panel_with_width(title, lines, color, terminal_width())
}

fn panel_with_width(title: &str, lines: &[String], color: bool, width_limit: usize) -> String {
    let mut title_lines = wrap_line(title, width_limit);
    let title = title_lines.remove(0);
    let mut wrapped_lines = title_lines;
    wrapped_lines.extend(lines.iter().flat_map(|line| wrap_line(line, width_limit)));
    let lines = wrapped_lines;
    let width = lines
        .iter()
        .map(|line| visible_len(line))
        .max()
        .unwrap_or(0)
        .max(visible_len(&title))
        .max(1);
    let mut output = format!(
        "╭─{} {}╮\n",
        cyan(&title, color),
        "─".repeat(width.saturating_sub(visible_len(&title)))
    );
    for line in lines {
        output.push_str(&format!(
            "│ {line}{} │\n",
            " ".repeat(width.saturating_sub(visible_len(&line)))
        ));
    }
    output.push_str(&format!("╰{}╯\n", "─".repeat(width + 2)));
    output
}

fn terminal_width() -> usize {
    static WIDTH: OnceLock<usize> = OnceLock::new();
    *WIDTH.get_or_init(|| {
        let columns = std::env::var("COLUMNS")
            .ok()
            .filter(|value| value.parse::<usize>().is_ok())
            .or_else(|| terminal_columns_from_tty().map(|value| value.to_string()));
        terminal_width_from(columns.as_deref())
    })
}

fn terminal_width_from(columns: Option<&str>) -> usize {
    columns
        .and_then(|value| value.parse().ok())
        .unwrap_or(80usize)
        .saturating_sub(4)
        .max(1)
}

#[cfg(unix)]
fn terminal_columns_from_tty() -> Option<usize> {
    use std::{
        fs::File,
        process::{Command, Stdio},
    };

    let tty = File::open("/dev/tty").ok()?;
    let output = Command::new("stty")
        .arg("size")
        .stdin(Stdio::from(tty))
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .nth(1)
        .and_then(|value| value.parse().ok())
}

#[cfg(not(unix))]
fn terminal_columns_from_tty() -> Option<usize> {
    None
}

fn wrap_line(line: &str, width: usize) -> Vec<String> {
    if width == 0 || visible_len(line) <= width {
        return vec![line.into()];
    }
    let mut wrapped = Vec::new();
    let mut current = String::new();
    let mut current_width = 0;
    let mut active = String::new();
    let mut characters = line.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '\x1b' {
            let mut sequence = String::from('\x1b');
            for sequence_character in characters.by_ref() {
                sequence.push(sequence_character);
                if sequence_character.is_ascii_alphabetic() {
                    break;
                }
            }
            if sequence.ends_with('m') {
                if sequence.ends_with("[0m") {
                    active.clear();
                } else {
                    active = sequence.clone();
                }
            }
            current.push_str(&sequence);
            continue;
        }
        let character_width = char_width(character);
        if current_width > 0 && current_width + character_width > width {
            if !active.is_empty() {
                current.push_str("\x1b[0m");
            }
            wrapped.push(std::mem::take(&mut current));
            current.push_str(&active);
            current_width = 0;
        }
        current.push(character);
        current_width += character_width;
    }
    if !current.is_empty() {
        wrapped.push(current);
    }
    wrapped
}

fn visible_len(value: &str) -> usize {
    let mut count = 0;
    let mut escape = false;
    for character in value.chars() {
        if escape {
            if character.is_ascii_alphabetic() {
                escape = false;
            }
        } else if character == '\x1b' {
            escape = true;
        } else {
            count += char_width(character);
        }
    }
    count
}

fn char_width(character: char) -> usize {
    let code = character as u32;
    if character.is_control()
        || matches!(code, 0x0300..=0x036f | 0x1ab0..=0x1aff | 0x1dc0..=0x1dff | 0x20d0..=0x20ff | 0xfe00..=0xfe0f | 0xfe20..=0xfe2f | 0x200d)
    {
        0
    } else if matches!(code, 0x1100..=0x115f | 0x2329..=0x232a | 0x2e80..=0xa4cf | 0xac00..=0xd7a3 | 0xf900..=0xfaff | 0xfe10..=0xfe19 | 0xfe30..=0xfe6f | 0xff00..=0xff60 | 0xffe0..=0xffe6 | 0x1f300..=0x1faff)
    {
        2
    } else {
        1
    }
}

pub fn elapsed(ms: Option<u64>) -> String {
    let Some(ms) = ms else {
        return "Unknown".into();
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i128;
    let age = (now - ms as i128).max(0) as u128;
    let seconds = age / 1_000;
    let relative = match seconds {
        0..=59 => format!("{seconds}s ago"),
        60..=3599 => format!("{}m ago", seconds / 60),
        3600..=86_399 => format!("{}h ago", seconds / 3600),
        _ => format!("{}d ago", seconds / 86_400),
    };
    format!("{relative} ({})", utc(ms))
}

fn utc(ms: u64) -> String {
    let seconds = ms / 1_000;
    let days = seconds / 86_400;
    let (year, month, day) = civil_from_days(days as i64);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02} UTC",
        (seconds % 86_400) / 3600,
        (seconds % 3600) / 60,
        seconds % 60
    )
}

// Howard Hinnant's Gregorian calendar conversion, kept local to avoid a date dependency.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    (y + if m <= 2 { 1 } else { 0 }, m, d)
}

#[cfg(test)]
mod tests {
    use super::{
        char_width, color_enabled, panel_with_width, terminal_supported, terminal_width_from, utc,
        visible_len, wrap_line,
    };

    #[test]
    fn utc_timestamp_is_stable_and_readable() {
        assert_eq!(utc(0), "1970-01-01 00:00:00 UTC");
        assert_eq!(utc(1_735_689_600_000), "2025-01-01 00:00:00 UTC");
    }

    #[test]
    fn terminal_rules_and_wrapping_are_deterministic() {
        assert!(!color_enabled(true, Some("dumb"), false));
        assert!(!color_enabled(true, Some("xterm"), true));
        assert!(color_enabled(true, Some("xterm"), false));
        assert!(!color_enabled(false, Some("xterm"), false));
        assert!(!terminal_supported(true, Some("dumb")));
        assert!(terminal_supported(true, Some("xterm")));
        assert_eq!(terminal_width_from(Some("20")), 16);
        assert_eq!(terminal_width_from(Some("2")), 1);
        assert_eq!(terminal_width_from(None), 76);
        assert_eq!(visible_len("界"), 2);
        assert_eq!(char_width('a'), 1);
        assert_eq!(wrap_line("abcdef", 3), ["abc", "def"]);
        assert_eq!(wrap_line("界界", 2), ["界", "界"]);
        assert_eq!(
            wrap_line("\x1b[36mabcdef\x1b[0m", 3),
            ["\x1b[36mabc\x1b[0m", "\x1b[36mdef\x1b[0m"]
        );
    }

    #[test]
    fn narrow_panels_wrap_without_overflow() {
        let rendered = panel_with_width(
            "界 title",
            &["a very long line with wide 界 text".into()],
            true,
            8,
        );
        let widths = rendered.lines().map(visible_len).collect::<Vec<_>>();
        assert!(!widths.is_empty());
        assert!(widths.iter().all(|width| *width == widths[0]));
        assert_eq!(widths[0], 12);
        assert!(rendered.contains("\x1b[36m"));
    }
}
