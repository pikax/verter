use crate::tokenizer::types::char_codes::{
    CARRIAGE_RETURN, FORM_FEED, GT, LEFT_BRACE, NEWLINE, RIGHT_BRACE, SLASH, SPACE, TAB,
};

#[inline(always)]
pub fn is_tag_start_char(c: u8) -> bool {
    c.is_ascii_alphabetic()
}

#[inline(always)]
pub fn is_whitespace(c: u8) -> bool {
    c == SPACE || c == NEWLINE || c == TAB || c == FORM_FEED || c == CARRIAGE_RETURN
}

#[inline(always)]
pub fn is_end_of_tag_section(c: u8) -> bool {
    c == SLASH || c == GT || is_whitespace(c)
}

pub const DEFAULT_DELIMITER_OPEN: &[u8] = &[LEFT_BRACE, LEFT_BRACE] /* {{ */;
pub const DEFAULT_DELIMITER_CLOSE: &[u8] = &[RIGHT_BRACE, RIGHT_BRACE] /* }} */;
