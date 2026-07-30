//! Deprecated compatibility names for the former Privacy Mode API.

#[deprecated(note = "use reader::discreet::DISCREET_ERROR_LINE")]
pub const PRIVACY_ERROR_LINE: &str = crate::discreet::DISCREET_ERROR_LINE;

#[deprecated(note = "use reader::discreet_stack")]
pub fn privacy_stack() -> &'static str {
    crate::discreet::discreet_stack()
}
