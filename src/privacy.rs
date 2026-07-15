pub const PRIVACY_STACK: &str = "Error: Uncaught TypeError: Cannot read properties of undefined (reading 'content')\nStack trace:\n    at PrivacyOverlay.render (reader.bundle.js:184:17)\n    at RedactedDocumentView.commit (reader.bundle.js:221:9)\n    at performSyncWorkOnRoot (react-dom.production.min.js:13:421)\n    at flushSyncCallbacks (react-dom.production.min.js:12:318)\n    at dispatchDiscreteEvent (react-dom.production.min.js:12:189)\n    at HTMLDocument.handleKeyDown (reader.bundle.js:306:11)\n    at ReaderShell.togglePrivacyMode (reader.bundle.js:148:5)\n    at useReaderHotkeys (reader.bundle.js:91:23)\n    at invokeGuardedCallback (react-dom.production.min.js:14:98)\n    at beginWork (react-dom.production.min.js:15:732)\n    at updateFunctionComponent (react-dom.production.min.js:16:214)\n    at renderWithHooks (react-dom.production.min.js:16:91)\n    at mountIndeterminateComponent (react-dom.production.min.js:16:641)\n    at workLoopSync (react-dom.production.min.js:19:311)\n    at renderRootSync (react-dom.production.min.js:19:142)\n    at recoverFromConcurrentError (react-dom.production.min.js:20:288)\n    at performConcurrentWorkOnRoot (react-dom.production.min.js:20:73)\n    at MessagePort.performWorkUntilDeadline (scheduler.production.min.js:7:412)\n    at runWithPriority (scheduler.production.min.js:6:224)\n    at flushWork (scheduler.production.min.js:7:95)";

pub const PRIVACY_ERROR_LINE: &str =
    "Error: Uncaught TypeError: Cannot read properties of undefined (reading 'content')";

pub fn privacy_stack() -> &'static str {
    PRIVACY_STACK
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn privacy_stack_is_fixed_and_redacted() {
        let stack = privacy_stack();

        assert!(stack.contains("Uncaught TypeError"));
        assert!(stack.contains("PrivacyOverlay.render"));
        assert!(stack.contains("react-dom.production.min.js"));
        assert!(!stack.contains('\\'));
        assert!(!stack.contains('/'));
    }
}
