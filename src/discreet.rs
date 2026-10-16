//! Agent-style presentation for local reading. Legacy diagnostic exports remain compatible.
use crate::book::{Section, TextPosition};
use crate::pagination::{wrap_text_with_layout, AnchoredLine};
use ratatui::layout::Rect;

pub const DISCREET_ERROR_LINE: &str =
    "TypeError: Cannot read properties of undefined (reading 'profile')";

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum AgentDensity {
    #[default]
    Comfortable,
    Inspect,
}

impl AgentDensity {
    pub fn toggle(self) -> Self {
        match self {
            Self::Comfortable => Self::Inspect,
            Self::Inspect => Self::Comfortable,
        }
    }

    #[allow(dead_code)]
    pub fn label(self) -> &'static str {
        match self {
            Self::Comfortable => "comfortable",
            Self::Inspect => "inspect",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentViewMode {
    Full,
    Compact,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct AgentLayout {
    pub mode: AgentViewMode,
    pub banner: Rect,
    pub header: Rect,
    pub body: Rect,
    pub footer: Rect,
}

impl AgentLayout {
    pub fn new(area: Rect, configured_width: Option<u16>) -> Self {
        if area.height < 6 {
            return Self {
                mode: AgentViewMode::Compact,
                banner: Rect::default(),
                header: Rect::new(area.x, area.y, area.width, area.height.min(1)),
                body: Rect::default(),
                footer: Rect::default(),
            };
        }
        let is_full = area.width >= 72 && area.height >= 22;
        let mode = if is_full {
            AgentViewMode::Full
        } else {
            AgentViewMode::Compact
        };

        let (banner_h, head_h, foot_h) = if is_full {
            let head = if area.height >= 28 { 6 } else { 4 };
            (3, head, 3)
        } else {
            let head = if area.height >= 14 { 2 } else { 1 };
            let foot = if area.height >= 14 { 2 } else { 1 };
            (0, head, foot)
        };

        let avail_mid = area.height.saturating_sub(banner_h + head_h + foot_h);
        let max_text = if area.height >= 26 { 16 } else { 12 };
        let text_rows = avail_mid.saturating_sub(2).min(max_text).max(1);
        let body_height = (text_rows + 2).min(avail_mid);

        let indent = if area.width < 50 { 2 } else { 4 };
        let available = area.width.saturating_sub(indent + 2);
        let body_width = configured_width.unwrap_or(72).max(20).min(available);

        Self {
            mode,
            banner: Rect::new(area.x, area.y, area.width, banner_h),
            header: Rect::new(area.x, area.y + banner_h, area.width, head_h),
            body: Rect::new(
                area.x + indent.min(area.width),
                area.y + banner_h + head_h,
                body_width,
                body_height,
            ),
            footer: Rect::new(
                area.x,
                area.y + banner_h + head_h + body_height,
                area.width,
                foot_h,
            ),
        }
    }

    /// Net width available for text based on density.
    /// Comfortable: full body width with 2 columns margin.
    /// Inspect: retains space for line number (7 cols: `NNN │ `).
    pub fn net_text_width(&self, density: AgentDensity) -> u16 {
        match density {
            AgentDensity::Comfortable => self.body.width.saturating_sub(2).max(20),
            AgentDensity::Inspect => self.body.width.saturating_sub(7).max(20),
        }
    }

    /// Net visible lines available for verbatim prose.
    /// Subtracts the "● Assistant" and "[Output complete]" marker rows.
    pub fn net_text_height(&self) -> u16 {
        self.body.height.saturating_sub(2).max(1)
    }
}

#[derive(Debug)]
pub(crate) struct ContextLine {
    pub section: usize,
    pub line: AnchoredLine,
}

impl ContextLine {
    pub fn position(&self) -> TextPosition {
        TextPosition::new(self.section, self.line.start_char)
    }
}

#[derive(Debug, Default)]
pub(crate) struct AgentViewport {
    pub lines: Vec<ContextLine>,
    pub top: usize,
    pub context_rows: usize,
    width: u16,
}

impl AgentViewport {
    pub fn reflow(&mut self, sections: &[Section], width: u16, position: TextPosition) {
        if self.width != width || self.lines.is_empty() {
            self.lines = sections
                .iter()
                .enumerate()
                .flat_map(|(section, source)| {
                    wrap_text_with_layout(&source.text, usize::from(width.max(1)), true)
                        .into_iter()
                        .map(move |line| ContextLine { section, line })
                })
                .collect();
            self.width = width;
        }
        let anchor = self
            .lines
            .iter()
            .rposition(|line| line.position() <= position)
            .unwrap_or(0);
        self.context_rows = usize::from(
            self.context_rows > 0
                && anchor > 0
                && self.lines[anchor - 1].section == self.lines[anchor].section,
        );
        self.top = anchor.saturating_sub(self.context_rows);
    }

    pub fn visible(&self, height: u16) -> &[ContextLine] {
        let start = self.top.min(self.lines.len());
        &self.lines[start
            ..start
                .saturating_add(usize::from(height))
                .min(self.lines.len())]
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum ReadDirection {
    #[default]
    Current,
    Forward,
    Backward,
}

/// A bounded summary of real operations, never copies of tool-output text.
#[derive(Debug, Default)]
pub(crate) struct AgentSession {
    pub view: AgentViewport,
    pub reads: usize,
    pub resumed: bool,
    pub direction: ReadDirection,
    pub density: AgentDensity,
    pub command: Option<String>,
    pub help: bool,
    pub help_offset: usize,
    pub receipt: Option<&'static str>,
    pub checkpoints: usize,
}

impl AgentSession {
    pub fn record_read(&mut self, direction: ReadDirection) {
        self.reads = self.reads.saturating_add(1);
        self.direction = direction;
        self.receipt = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_matches_approved_grid_and_never_escapes_viewport() {
        for (w, h, mode, banner, head, body, foot, cols, net_text) in [
            (80, 24, AgentViewMode::Full, 3, 4, 14, 3, 72, 12),
            (120, 30, AgentViewMode::Full, 3, 6, 18, 3, 72, 16),
            (60, 18, AgentViewMode::Compact, 0, 2, 14, 2, 54, 12),
        ] {
            let l = AgentLayout::new(Rect::new(0, 0, w, h), None);
            assert_eq!(
                (
                    l.mode,
                    l.banner.height,
                    l.header.height,
                    l.body.height,
                    l.footer.height,
                    l.body.width,
                    l.net_text_height()
                ),
                (mode, banner, head, body, foot, cols, net_text)
            );
            assert!(l.net_text_height() >= 10 && l.net_text_height() <= 16);
        }
        for w in 0..=140 {
            for h in 0..=40 {
                let l = AgentLayout::new(Rect::new(0, 0, w, h), Some(100));
                for area in [l.banner, l.header, l.body, l.footer] {
                    assert!(area.right() <= w && area.bottom() <= h);
                }
            }
        }
    }
}
