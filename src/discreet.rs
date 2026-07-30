use std::time::{SystemTime, UNIX_EPOCH};

use ratatui::layout::Rect;
use time::OffsetDateTime;

use crate::book::TextPosition;

pub const DISCREET_ERROR_LINE: &str =
    "TypeError: Cannot read properties of undefined (reading 'profile')";

const DISCREET_TEXT_MIN_WIDTH: u16 = 48;
const DISCREET_TEXT_PREFERRED_WIDTH: u16 = 64;
const DISCREET_TEXT_MAX_WIDTH: u16 = 64;
const FULL_PREFIX_WIDTH: u16 = 15;
const COMPACT_PREFIX_WIDTH: u16 = 8;
const MINIMAL_PREFIX_WIDTH: u16 = 2;

#[cfg(windows)]
const CAMOUFLAGE_STACK: &str = r#"PS C:\workspace\dashboard-client> node .\dist\server.js

C:\workspace\dashboard-client\dist\chunks\session.js:184
  const profile = state.session.profile;
                                ^

TypeError: Cannot read properties of undefined (reading 'profile')
    at resolveActiveSession (dist\chunks\session.js:184:17)
    at renderWorkspace (dist\chunks\workspace.js:221:9)
    at commitHookEffectListMount (node_modules\react-dom\cjs\react-dom.production.min.js:13:421)
    at commitPassiveMountOnFiber (node_modules\react-dom\cjs\react-dom.production.min.js:14:318)
    at recursivelyTraversePassiveMountEffects (node_modules\react-dom\cjs\react-dom.production.min.js:14:189)
    at commitPassiveMountEffects (node_modules\react-dom\cjs\react-dom.production.min.js:15:732)
    at flushPassiveEffectsImpl (node_modules\react-dom\cjs\react-dom.production.min.js:16:214)
    at flushPassiveEffects (node_modules\react-dom\cjs\react-dom.production.min.js:16:91)
    at performSyncWorkOnRoot (node_modules\react-dom\cjs\react-dom.production.min.js:19:311)
    at flushSyncCallbacks (node_modules\react-dom\cjs\react-dom.production.min.js:19:142)
    at MessagePort.performWorkUntilDeadline (node_modules\scheduler\cjs\scheduler.production.min.js:7:412)

PS C:\workspace\dashboard-client>"#;

#[cfg(not(windows))]
const CAMOUFLAGE_STACK: &str = r#"~/workspace/dashboard-client $ node ./dist/server.js

/workspace/dashboard-client/dist/chunks/session.js:184
  const profile = state.session.profile;
                                ^

TypeError: Cannot read properties of undefined (reading 'profile')
    at resolveActiveSession (dist/chunks/session.js:184:17)
    at renderWorkspace (dist/chunks/workspace.js:221:9)
    at commitHookEffectListMount (node_modules/react-dom/cjs/react-dom.production.min.js:13:421)
    at commitPassiveMountOnFiber (node_modules/react-dom/cjs/react-dom.production.min.js:14:318)
    at recursivelyTraversePassiveMountEffects (node_modules/react-dom/cjs/react-dom.production.min.js:14:189)
    at commitPassiveMountEffects (node_modules/react-dom/cjs/react-dom.production.min.js:15:732)
    at flushPassiveEffectsImpl (node_modules/react-dom/cjs/react-dom.production.min.js:16:214)
    at flushPassiveEffects (node_modules/react-dom/cjs/react-dom.production.min.js:16:91)
    at performSyncWorkOnRoot (node_modules/react-dom/cjs/react-dom.production.min.js:19:311)
    at flushSyncCallbacks (node_modules/react-dom/cjs/react-dom.production.min.js:19:142)
    at MessagePort.performWorkUntilDeadline (node_modules/scheduler/cjs/scheduler.production.min.js:7:412)

~/workspace/dashboard-client $"#;

pub fn discreet_stack() -> &'static str {
    CAMOUFLAGE_STACK
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DiscreetPrefixMode {
    Full,
    Compact,
    Minimal,
    None,
}

impl DiscreetPrefixMode {
    pub(crate) const fn width(self) -> u16 {
        match self {
            Self::Full => FULL_PREFIX_WIDTH,
            Self::Compact => COMPACT_PREFIX_WIDTH,
            Self::Minimal => MINIMAL_PREFIX_WIDTH,
            Self::None => 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DiscreetLayout {
    pub body_width: u16,
    pub prefix_mode: DiscreetPrefixMode,
}

impl DiscreetLayout {
    pub(crate) const fn total_width(self) -> u16 {
        self.body_width.saturating_add(self.prefix_mode.width())
    }
}

pub(crate) fn discreet_layout(viewport: Rect, configured_width: Option<u16>) -> DiscreetLayout {
    let available = viewport.width.saturating_sub(2).max(1);
    let target = configured_width
        .unwrap_or(DISCREET_TEXT_PREFERRED_WIDTH)
        .clamp(DISCREET_TEXT_MIN_WIDTH, DISCREET_TEXT_MAX_WIDTH);

    if available >= target.saturating_add(FULL_PREFIX_WIDTH) {
        return DiscreetLayout {
            body_width: target,
            prefix_mode: DiscreetPrefixMode::Full,
        };
    }
    if available >= target.saturating_add(COMPACT_PREFIX_WIDTH) {
        return DiscreetLayout {
            body_width: target,
            prefix_mode: DiscreetPrefixMode::Compact,
        };
    }
    if available >= target.saturating_add(MINIMAL_PREFIX_WIDTH) {
        return DiscreetLayout {
            body_width: target,
            prefix_mode: DiscreetPrefixMode::Minimal,
        };
    }
    if available > MINIMAL_PREFIX_WIDTH {
        return DiscreetLayout {
            body_width: available.saturating_sub(MINIMAL_PREFIX_WIDTH).max(1),
            prefix_mode: DiscreetPrefixMode::Minimal,
        };
    }

    DiscreetLayout {
        body_width: target.min(available),
        prefix_mode: DiscreetPrefixMode::None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DiscreetLogState {
    page_start: TextPosition,
    base_seconds: u32,
    seed: u64,
}

impl DiscreetLogState {
    pub(crate) fn new(book_id: &str, page_start: TextPosition) -> Self {
        let now = OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc());
        let base_seconds =
            u32::from(now.hour()) * 3600 + u32::from(now.minute()) * 60 + u32::from(now.second());
        let entropy = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        let seed = mix64(
            entropy
                ^ stable_hash(book_id)
                ^ (page_start.section_index as u64).rotate_left(17)
                ^ page_start.char_offset as u64,
        );
        Self {
            page_start,
            base_seconds,
            seed,
        }
    }

    pub(crate) fn page_start(&self) -> TextPosition {
        self.page_start
    }

    pub(crate) fn prefixes(
        &self,
        lines: &[String],
        mode: DiscreetPrefixMode,
    ) -> Vec<Option<DiscreetLogPrefix>> {
        if mode == DiscreetPrefixMode::None {
            return vec![None; lines.len()];
        }

        let mut timestamp = self.base_seconds;
        let mut event_index = 0_u64;
        lines
            .iter()
            .enumerate()
            .map(|(line_index, line)| {
                if line.trim().is_empty() {
                    return None;
                }

                if event_index > 0 {
                    timestamp = (timestamp
                        + (mix64(self.seed ^ event_index.rotate_left(23)) % 3) as u32)
                        % 86_400;
                }
                let level = level_for_roll(
                    (mix64(self.seed ^ line_index as u64 ^ event_index.rotate_left(41)) % 100)
                        as u8,
                );
                event_index += 1;
                Some(format_prefix(timestamp, level, mode))
            })
            .collect()
    }

    #[cfg(test)]
    fn fixed(page_start: TextPosition, base_seconds: u32, seed: u64) -> Self {
        Self {
            page_start,
            base_seconds,
            seed,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LogLevel {
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DiscreetLogPrefix {
    pub leading: String,
    pub keyword: &'static str,
    pub trailing: String,
    pub level: LogLevel,
}

fn level_for_roll(roll: u8) -> LogLevel {
    match roll {
        0..=79 => LogLevel::Info,
        80..=94 => LogLevel::Warn,
        _ => LogLevel::Error,
    }
}

fn format_prefix(seconds: u32, level: LogLevel, mode: DiscreetPrefixMode) -> DiscreetLogPrefix {
    let hour = seconds / 3600;
    let minute = seconds % 3600 / 60;
    let second = seconds % 60;
    let (leading, keyword) = match mode {
        DiscreetPrefixMode::Full => (
            format!("{hour:02}:{minute:02}:{second:02} "),
            match level {
                LogLevel::Info => "INFO",
                LogLevel::Warn => "WARN",
                LogLevel::Error => "ERROR",
            },
        ),
        DiscreetPrefixMode::Compact => (
            format!("{minute:02}:{second:02} "),
            match level {
                LogLevel::Info => "I",
                LogLevel::Warn => "W",
                LogLevel::Error => "E",
            },
        ),
        DiscreetPrefixMode::Minimal => (
            String::new(),
            match level {
                LogLevel::Info => "I",
                LogLevel::Warn => "W",
                LogLevel::Error => "E",
            },
        ),
        DiscreetPrefixMode::None => (String::new(), ""),
    };
    let used_width = leading.len().saturating_add(keyword.len());
    DiscreetLogPrefix {
        leading,
        keyword,
        trailing: " ".repeat(usize::from(mode.width()).saturating_sub(used_width)),
        level,
    }
}

fn stable_hash(value: &str) -> u64 {
    value.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

fn mix64(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9e37_79b9_7f4a_7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_adapts_without_sacrificing_reading_floor() {
        let full = discreet_layout(Rect::new(0, 0, 81, 24), None);
        assert_eq!(full.prefix_mode, DiscreetPrefixMode::Full);
        assert_eq!(full.body_width, 64);

        let compact = discreet_layout(Rect::new(0, 0, 74, 24), None);
        assert_eq!(compact.prefix_mode, DiscreetPrefixMode::Compact);
        assert_eq!(compact.body_width, 64);

        let minimal = discreet_layout(Rect::new(0, 0, 73, 24), None);
        assert_eq!(minimal.prefix_mode, DiscreetPrefixMode::Minimal);
        assert_eq!(minimal.body_width, 64);

        let floor = discreet_layout(Rect::new(0, 0, 52, 24), None);
        assert_eq!(floor.prefix_mode, DiscreetPrefixMode::Minimal);
        assert_eq!(floor.body_width, 48);

        let narrow = discreet_layout(Rect::new(0, 0, 51, 24), None);
        assert_eq!(narrow.prefix_mode, DiscreetPrefixMode::Minimal);
        assert_eq!(narrow.body_width, 47);

        let extreme = discreet_layout(Rect::new(0, 0, 4, 24), None);
        assert_eq!(extreme.prefix_mode, DiscreetPrefixMode::None);
        assert_eq!(extreme.body_width, 2);

        for width in 1..=160 {
            let layout = discreet_layout(Rect::new(0, 0, width, 24), None);
            assert!(layout.total_width() <= width.saturating_sub(2).max(1));
        }
    }

    #[test]
    fn log_prefixes_are_stable_and_mark_every_nonempty_physical_line() {
        let state = DiscreetLogState::fixed(TextPosition::new(0, 0), 13 * 3600 + 21 * 60 + 6, 7);
        let lines = vec![
            "first".to_string(),
            "continuation".to_string(),
            String::new(),
            "next paragraph".to_string(),
        ];

        let first = state.prefixes(&lines, DiscreetPrefixMode::Full);
        let second = state.prefixes(&lines, DiscreetPrefixMode::Full);

        assert_eq!(first, second);
        assert!(first[0]
            .as_ref()
            .is_some_and(|prefix| prefix.leading == "13:21:06 "));
        assert!(first[1].is_some());
        assert!(first[2].is_none());
        assert!(first[3].is_some());
        for prefix in first.into_iter().flatten() {
            assert_eq!(
                prefix.leading.len() + prefix.keyword.len() + prefix.trailing.len(),
                usize::from(FULL_PREFIX_WIDTH)
            );
            assert!(matches!(prefix.keyword, "INFO" | "WARN" | "ERROR"));
        }
    }

    #[test]
    fn log_level_distribution_boundaries_are_exact() {
        assert_eq!(level_for_roll(0), LogLevel::Info);
        assert_eq!(level_for_roll(79), LogLevel::Info);
        assert_eq!(level_for_roll(80), LogLevel::Warn);
        assert_eq!(level_for_roll(94), LogLevel::Warn);
        assert_eq!(level_for_roll(95), LogLevel::Error);
        assert_eq!(level_for_roll(99), LogLevel::Error);
    }

    #[test]
    fn structured_prefixes_fill_each_adaptive_width() {
        for (mode, width) in [
            (DiscreetPrefixMode::Full, FULL_PREFIX_WIDTH),
            (DiscreetPrefixMode::Compact, COMPACT_PREFIX_WIDTH),
            (DiscreetPrefixMode::Minimal, MINIMAL_PREFIX_WIDTH),
        ] {
            for level in [LogLevel::Info, LogLevel::Warn, LogLevel::Error] {
                let prefix = format_prefix(13 * 3600 + 21 * 60 + 6, level, mode);
                assert_eq!(
                    prefix.leading.len() + prefix.keyword.len() + prefix.trailing.len(),
                    usize::from(width)
                );
            }
        }
    }

    #[test]
    fn stack_is_fixed_neutral_and_redacted() {
        let stack = discreet_stack();

        assert!(stack.contains("TypeError"));
        assert!(stack.contains("resolveActiveSession"));
        assert!(stack.contains("react-dom"));
        for forbidden in ["Privacy", "Discreet", "Reader", "Book", "secret-title"] {
            assert!(!stack.contains(forbidden));
        }
    }
}
