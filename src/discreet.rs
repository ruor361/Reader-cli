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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DiscreetLayout {
    pub body_width: u16,
}

// The Agent Terminal renders one contiguous Read tool result, not log records.
pub(crate) fn discreet_layout(viewport: Rect, configured_width: Option<u16>) -> DiscreetLayout {
    let available = viewport.width.saturating_sub(6).max(1);
    let body_width = configured_width.unwrap_or(64).clamp(48, 72).min(available);
    DiscreetLayout { body_width }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_layout_reserves_gutter_and_right_margin() {
        for (width, expected) in [
            (120, 64),
            (81, 64),
            (74, 64),
            (52, 46),
            (51, 45),
            (4, 1),
        ] {
            let layout = discreet_layout(Rect::new(0, 0, width, 24), None);
            assert_eq!(layout.body_width, expected);
            assert!(layout.body_width <= width.saturating_sub(6).max(1));
        }
        assert_eq!(
            discreet_layout(Rect::new(0, 0, 120, 24), Some(100)).body_width,
            72
        );
    }

    #[test]
    fn legacy_stack_remains_public_only_for_compatibility() {
        let stack = discreet_stack();
        assert!(stack.contains("TypeError"));
        assert!(stack.contains("resolveActiveSession"));
        assert!(!stack.contains("secret-title"));
    }
}
