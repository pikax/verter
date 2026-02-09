// Fast byte-based tokenizer about 3X throughput of string-based tokenizer
//! Based on vue tokenizer

use super::types::{Event, QuoteType};
use crate::{
    common::ErrorCode,
    tokenizer::{
        helpers::{
            is_end_of_tag_section, is_tag_start_char, is_whitespace, DEFAULT_DELIMITER_CLOSE,
            DEFAULT_DELIMITER_OPEN,
        },
        types::{
            char_codes::*,
            sequences::{COMMENT_END, SCRIPT_END, STYLE_END},
        },
    },
};
use memchr::{memchr, memchr2, memchr3};

/// All tokenizer states
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
#[allow(dead_code)]
enum State {
    Text,
    InterpolationOpen,
    Interpolation,
    InterpolationClose,
    BeforeTagName,
    InTagName,
    InSelfClosingTag,
    BeforeClosingTagName,
    InClosingTagName,
    AfterClosingTagName,
    BeforeAttrName,
    InAttrName,
    InDirName,
    InDirArg,
    InDirDynamicArg,
    InDirModifier,
    AfterAttrName,
    BeforeAttrValue,
    InAttrValueDq,
    InAttrValueSq,
    InAttrValueNq,
    BeforeDeclaration,
    InDeclaration,
    InProcessingInstruction,
    BeforeComment,
    InCommentLike,

    // Special tags
    BeforeSpecialS, // Decide if we deal with `<script` or `<style`
    BeforeSpecialT, // Decide if we deal with `<title` or `<textarea`
    SpecialStartSequence,
    InRCDATA,

    InEntity,

    InSFCRootTagName,
}

/// Tokenize the input and call the callback for each event.
pub fn tokenize(input: &[u8], callback: impl FnMut(Event<'static>)) {
    tokenize_with_delimiters(
        input,
        callback,
        DEFAULT_DELIMITER_OPEN,
        DEFAULT_DELIMITER_CLOSE,
    )
}

/// Tokenize the input with custom delimiters and call the callback for each event.
pub fn tokenize_with_delimiters(
    input: &[u8],
    callback: impl FnMut(Event<'static>),
    delimiter_open: &[u8],
    delimiter_close: &[u8],
) {
    let mut tokenizer = Tokenizer::new(input, callback, delimiter_open, delimiter_close);
    tokenizer.run();
}

struct Tokenizer<'a, F: FnMut(Event<'static>)> {
    input: &'a [u8],

    /** The current state the tokenizer is in. */
    state: State,

    /** Some behavior, eg. when decoding entities, is done while we are in another state. This keeps track of the other state type. */
    #[allow(dead_code)]
    base_state: State,
    section_start: usize,
    index: usize,
    callback: F,
    delimiter_open: &'a [u8],
    delimiter_close: &'a [u8],
    delimiter_index: usize,
    in_rcdata: bool,
    /** The closing sequence to look for in RCDATA mode (e.g., b"</script") */
    current_sequence: &'static [u8],
    /** Index into current_sequence for matching */
    sequence_index: usize,
    /** For disabling RCDATA tags handling */
    #[allow(dead_code)]
    in_xml: bool,
    /** For disabling interpolation parsing in v-pre */
    in_v_pre: bool,
    /** Depth counter for v-pre element nesting */
    v_pre_depth: usize,
    // Cached first bytes of delimiters
    delim_open_first: u8,
    delim_close_first: u8,
}

impl<'a, F: FnMut(Event<'static>)> Tokenizer<'a, F> {
    fn new(
        input: &'a [u8],
        callback: F,
        delimiter_open: &'a [u8],
        delimiter_close: &'a [u8],
    ) -> Self {
        Self {
            input,
            state: State::Text,
            base_state: State::Text,
            section_start: 0,
            index: 0,
            callback,
            delimiter_open,
            delimiter_close,
            delimiter_index: 0,
            in_rcdata: false,
            current_sequence: SCRIPT_END,
            sequence_index: 0,
            in_xml: false,
            in_v_pre: false,
            v_pre_depth: 0,
            delim_open_first: delimiter_open.first().copied().unwrap_or(LEFT_BRACE),
            delim_close_first: delimiter_close.first().copied().unwrap_or(RIGHT_BRACE),
        }
    }

    fn emit(&mut self, event: Event<'static>) {
        (self.callback)(event);
    }
    fn emit_open_tag_name(&mut self, start: u32, end: u32) {
        if self.v_pre_depth > 0 {
            self.v_pre_depth += 1;
        }
        self.emit(Event::OpenTagName { start, end });
    }

    fn emit_open_tag_end(&mut self, end: u32) {
        self.emit(Event::OpenTagEnd { end });
    }

    fn emit_close_tag(&mut self, start: u32, end: u32, name_end: u32) {
        // if self.in_v_pre {
        if self.v_pre_depth > 0 {
            self.v_pre_depth -= 1;
            if self.v_pre_depth == 0 {
                self.in_v_pre = false;
            }
        } else {
            self.in_v_pre = false;
        }
        // }
        self.emit(Event::CloseTag {
            start,
            end,
            name_end,
        });
    }
    fn emit_self_closing_tag(&mut self, end: u32) {
        if self.v_pre_depth > 0 {
            self.v_pre_depth -= 1;
            if self.v_pre_depth == 0 {
                self.in_v_pre = false;
            }
        } else {
            self.in_v_pre = false;
        }
        self.emit(Event::SelfClosingTag { end });
    }

    fn flush_text(&mut self, end: usize, ignore_if_all_whitespace: bool) {
        if self.section_start < end {
            // check if all the bytes are whitespace
            if ignore_if_all_whitespace {
                if !self.input[self.section_start..end]
                    .iter()
                    .all(|&b| is_whitespace(b))
                {
                    self.emit(Event::Text {
                        start: self.section_start as u32,
                        end: end as u32,
                    });
                }
            } else {
                self.emit(Event::Text {
                    start: self.section_start as u32,
                    end: end as u32,
                });
            }
        }
        self.section_start = end;
    }

    fn run(&mut self) {
        let len = self.input.len();

        while self.index < len {
            match self.state {
                State::Text => self.state_text(),
                State::Interpolation => self.state_interpolation(),
                State::InRCDATA => self.state_in_rcdata(),
                _ => {
                    self.index += 1;
                }
            }
        }

        self.emit(Event::End);
    }

    fn state_text(&mut self) {
        let remaining = &self.input[self.index..];

        let next_pos = if !self.in_v_pre {
            memchr3(LT, AMP, self.delim_open_first, remaining)
        } else {
            memchr2(LT, AMP, remaining)
        };

        match next_pos {
            Some(pos) => {
                let p = self.index + pos;
                let c = self.input[p];

                if c == LT {
                    if p > self.section_start {
                        self.flush_text(p, true);
                    }
                    self.state = State::BeforeTagName;
                    self.section_start = p;
                    self.index = p + 1;
                    self.state_before_tag_name();
                } else if c == AMP {
                    // TODO: entity handling
                    self.index = p + 1;
                } else if c == self.delim_open_first {
                    self.state = State::InterpolationOpen;
                    self.index = p;
                    self.delimiter_index = 0;
                    self.state_interpolation_open();
                }
            }
            None => {
                if self.input.len() > self.section_start {
                    self.flush_text(self.input.len(), true);
                }
                self.index = self.input.len();
            }
        }
    }

    fn state_interpolation_open(&mut self) {
        if next_bytes_equal(self.delimiter_open, &self.input[self.index..]) {
            if self.index > self.section_start {
                self.flush_text(self.index, false);
            }
            self.section_start = self.index;
            self.index += self.delimiter_open.len();
            self.state = State::Interpolation;

            match find_subslice(self.delimiter_close, &self.input[self.index..]) {
                Some(p) => {
                    self.index += p + self.delimiter_close.len();
                    self.emit(Event::Interpolation {
                        start: self.section_start as u32,
                        end: self.index as u32,
                        delimiter_open_len: self.delimiter_open.len() as u8,
                        delimiter_close_len: self.delimiter_close.len() as u8,
                    });
                    self.section_start = self.index;
                    self.state = State::Text;
                }
                None => {
                    self.index = self.input.len();
                }
            }
        } else if self.in_rcdata {
            self.state = State::InRCDATA;
        } else {
            self.index += 1;
            self.state = State::Text;
        }
    }

    fn state_interpolation(&mut self) {
        let remaining = &self.input[self.index..];
        let next_pos = memchr(self.delim_close_first, remaining);

        match next_pos {
            Some(pos) => {
                let p = self.index + pos;
                if next_bytes_equal(self.delimiter_close, &self.input[p..]) {
                    self.emit(Event::Interpolation {
                        start: self.section_start as u32,
                        end: (p + self.delimiter_close.len()) as u32,
                        delimiter_open_len: self.delimiter_open.len() as u8,
                        delimiter_close_len: self.delimiter_close.len() as u8,
                    });
                    self.index = p + self.delimiter_close.len();
                    self.section_start = self.index;
                    if self.in_rcdata {
                        self.state = State::InRCDATA;
                    } else {
                        self.state = State::Text;
                    }
                } else {
                    self.index = p + 1;
                    self.state_interpolation();
                }
            }
            None => {
                self.index = self.input.len();
            }
        }
    }

    fn state_in_rcdata(&mut self) {
        while self.index < self.input.len() {
            if self.sequence_index == self.current_sequence.len() {
                let c = self.input[self.index];
                if c == GT || is_whitespace(c) {
                    let end_of_text = self.index - self.current_sequence.len();
                    if self.section_start < end_of_text {
                        self.flush_text(end_of_text, true);
                    }
                    self.section_start = end_of_text;
                    self.state = State::InClosingTagName;
                    self.in_rcdata = false;
                    self.state_in_closing_tag_name();
                    return;
                }
                self.sequence_index = 0;
            }

            let c = self.input[self.index];
            if (c | 0x20) == self.current_sequence[self.sequence_index] {
                self.sequence_index += 1;
            } else if self.sequence_index == 0 {
                if let Some(pos) = memchr(LT, &self.input[self.index..]) {
                    self.index += pos;
                    self.sequence_index = 1;
                } else {
                    self.index = self.input.len();
                    return;
                }
            } else {
                self.sequence_index = if c == LT { 1 } else { 0 };
            }
            self.index += 1;
        }
    }

    fn state_before_tag_name(&mut self) {
        if self.index >= self.input.len() {
            return;
        }
        let c = self.input[self.index];
        if c == EXCLAMATION_MARK {
            self.state = State::BeforeDeclaration;
            self.index += 1;
            self.state_in_before_declaration();
        } else if c == QUESTION_MARK {
            self.state = State::InProcessingInstruction;
            self.index += 1;
            self.section_start = self.index;
            self.state_in_processing_instruction();
        } else if c == SLASH {
            // set to be in "<" which is before "/"
            self.section_start = self.index - 1;
            self.state = State::BeforeClosingTagName;
            self.index += 1;
            self.state_before_closing_tag_name();
        } else if is_tag_start_char(c) {
            // self.section_start = self.index;
            self.state = State::InTagName;
            self.state_in_tag_name()
        } else {
            self.state = State::Text;
            self.state_text();
        }
    }

    fn state_in_processing_instruction(&mut self) {
        while self.index < self.input.len() {
            if self.input[self.index] == GT {
                self.emit(Event::ProcessingInstruction {
                    start: self.section_start as u32,
                    end: self.index as u32,
                });
                self.index += 1;
                self.state = State::Text;
                self.section_start = self.index;
                return;
            }
            self.index += 1;
        }
    }

    fn state_before_closing_tag_name(&mut self) {
        while self.index < self.input.len() {
            let c = self.input[self.index];
            if is_whitespace(c) {
                self.index += 1;
            } else if c == GT {
                self.emit(Event::Error {
                    code: ErrorCode::MISSING_END_TAG_NAME,
                    index: self.index as u32,
                });
                self.index += 1;
                self.state = State::Text;
                self.section_start = self.index;
                return;
            } else {
                self.state = State::InClosingTagName;
                self.state_in_closing_tag_name();
                return;
            }
        }
    }

    fn state_in_closing_tag_name(&mut self) {
        let tag_start = self.section_start;
        while self.index < self.input.len() {
            let c = self.input[self.index];
            if c == GT {
                self.emit_close_tag(tag_start as u32, self.index as u32 + 1, self.index as u32);
                self.index += 1;
                self.state = State::Text;
                self.section_start = self.index;
                return;
            } else if is_whitespace(c) {
                self.state = State::AfterClosingTagName;
                let name_end = self.index as u32;
                // Find the > to get gt_end
                while self.index < self.input.len() && self.input[self.index] != GT {
                    self.index += 1;
                }
                self.emit_close_tag(tag_start as u32, self.index as u32 + 1, name_end);
                self.index += 1;
                self.state = State::Text;
                self.section_start = self.index;
                return;
            } else {
                self.index += 1;
            }
        }
    }

    fn state_in_before_declaration(&mut self) {
        if self.index >= self.input.len() {
            return;
        }
        let c = self.input[self.index];

        match c {
            LEFT_SQUARE => {
                // TODO CData
            }
            _ => {
                if c == DASH {
                    self.state = State::BeforeComment;

                    if next_bytes_equal(b"->", &self.input[self.index + 1..]) {
                        self.index += 3;
                        self.emit(Event::Comment {
                            start: self.section_start as u32,
                            end: self.index as u32,
                        });
                        self.state = State::Text;
                    } else if self.index + 1 < self.input.len()
                        && self.input[self.index + 1] == DASH
                    {
                        self.state = State::InCommentLike;
                        self.index += 2;
                        self.section_start = self.index - 4; // include "<!--"

                        match find_subslice(COMMENT_END, &self.input[self.index..]) {
                            Some(p) => {
                                self.index = self.index + p + COMMENT_END.len();
                                self.emit(Event::Comment {
                                    start: self.section_start as u32,
                                    end: self.index as u32,
                                });
                                self.section_start = self.index;
                                self.state = State::Text;
                            }
                            None => {
                                self.index = self.input.len();
                            }
                        }
                    }
                } else {
                    self.state = State::InDeclaration;
                    self.index += 1;
                }
            }
        }
    }

    fn check_and_setup_rcdata(&mut self) {
        let tag_name = &self.input[self.section_start + 1..self.index];
        let len = tag_name.len();

        if len == 6 {
            if (tag_name[0] | 0x20) == b's'
                && (tag_name[1] | 0x20) == b'c'
                && (tag_name[2] | 0x20) == b'r'
                && (tag_name[3] | 0x20) == b'i'
                && (tag_name[4] | 0x20) == b'p'
                && (tag_name[5] | 0x20) == b't'
            {
                self.in_rcdata = true;
                self.current_sequence = SCRIPT_END;
                self.sequence_index = 0;
            }
        } else if len == 5
            && (tag_name[0] | 0x20) == b's'
            && (tag_name[1] | 0x20) == b't'
            && (tag_name[2] | 0x20) == b'y'
            && (tag_name[3] | 0x20) == b'l'
            && (tag_name[4] | 0x20) == b'e'
        {
            self.in_rcdata = true;
            self.current_sequence = STYLE_END;
            self.sequence_index = 0;
        }
    }

    fn state_in_tag_name(&mut self) {
        while self.index < self.input.len() {
            let c = self.input[self.index];
            if is_end_of_tag_section(c) {
                self.check_and_setup_rcdata();

                self.emit_open_tag_name(self.section_start as u32, self.index as u32);

                if c == GT {
                    self.emit_open_tag_end(self.index as u32);
                    self.index += 1;
                    if self.in_rcdata {
                        self.state = State::InRCDATA;
                        self.section_start = self.index;
                        self.state_in_rcdata();
                    } else {
                        self.state = State::Text;
                        self.section_start = self.index;
                    }
                    return;
                } else if c == SLASH {
                    self.index += 1;
                    if self.index < self.input.len() && self.input[self.index] == GT {
                        self.index += 1;
                        self.emit_self_closing_tag(self.index as u32);
                        self.in_rcdata = false;
                        self.state = State::Text;
                        self.section_start = self.index;
                    } else {
                        self.state = State::BeforeAttrName;
                        self.state_in_before_attr_name();
                    }
                    return;
                } else {
                    self.index += 1;
                    self.state = State::BeforeAttrName;
                    self.state_in_before_attr_name();
                    return;
                }
            } else {
                self.index += 1;
            }
        }
    }

    fn state_in_before_attr_name(&mut self) {
        while self.index < self.input.len() {
            let c = self.input[self.index];
            if c == GT {
                self.emit_open_tag_end(self.index as u32);
                self.index += 1;
                if self.in_rcdata {
                    self.state = State::InRCDATA;
                    self.section_start = self.index;
                    self.state_in_rcdata();
                } else {
                    self.state = State::Text;
                    self.section_start = self.index;
                }
                return;
            } else if c == SLASH {
                self.state = State::InSelfClosingTag;
                self.index += 1;
                if self.index < self.input.len() && self.input[self.index] == GT {
                    self.index += 1;
                    self.emit_self_closing_tag(self.index as u32);
                    self.in_rcdata = false;
                    self.state = State::Text;
                    self.section_start = self.index;
                    return;
                } else {
                    self.emit(Event::Error {
                        code: ErrorCode::UNEXPECTED_SOLIDUS_IN_TAG,
                        index: self.index as u32,
                    });
                    return;
                }
            } else if c == LT
                && self.index + 1 < self.input.len()
                && self.input[self.index + 1] == SLASH
            {
                self.emit_open_tag_end(self.index as u32);
                self.state = State::BeforeTagName;
                self.section_start = self.index;
                self.state_before_tag_name();
                return;
            } else if !is_whitespace(c) {
                if c == EQ {
                    self.emit(Event::Error {
                        code: ErrorCode::UNEXPECTED_EQUALS_SIGN_BEFORE_ATTRIBUTE_NAME,
                        index: self.index as u32,
                    });
                }

                if c == LOWER_V
                    && !self.in_v_pre
                    && self.index + 1 < self.input.len()
                    && self.input[self.index + 1] == DASH
                {
                    self.section_start = self.index;
                    self.state = State::InDirName;
                    self.state_in_dir_name();
                    return;
                } else if c == DOT || c == COLON || c == AT || c == NUMBER {
                    self.emit(Event::DirName {
                        start: self.index as u32,
                        end: (self.index + 1) as u32,
                    });
                    self.index += 1;
                    self.section_start = self.index;
                    self.state = State::InDirArg;
                    self.state_in_dir_arg();
                    return;
                } else {
                    self.section_start = self.index;
                    self.state = State::InAttrName;
                    self.state_in_attr_name();
                    return;
                }
            } else {
                self.index += 1;
            }
        }
    }

    fn state_in_dir_name(&mut self) {
        while self.index < self.input.len() {
            let c = self.input[self.index];
            if is_end_of_tag_section(c) || c == EQ || c == COLON || c == DOT {
                // check if this is v-pre (5 characters: v-pre)
                if self.index - self.section_start == 5
                    && (self.input[self.section_start] | 0x20) == b'v'
                    && self.input[self.section_start + 1] == b'-'
                    && (self.input[self.section_start + 2] | 0x20) == b'p'
                    && (self.input[self.section_start + 3] | 0x20) == b'r'
                    && (self.input[self.section_start + 4] | 0x20) == b'e'
                {
                    self.emit(Event::DirVPre {
                        start: self.section_start as u32,
                        end: self.index as u32,
                    });

                    self.v_pre_depth += 1; // Will be incremented to 1 when OpenTagEnd is emitted
                    self.in_v_pre = true;

                    while self.index < self.input.len() {
                        let c = self.input[self.index];
                        if c == GT {
                            self.emit_open_tag_end(self.index as u32);

                            self.index += 1;

                            if self.in_rcdata {
                                self.state = State::InRCDATA;
                                self.section_start = self.index;
                                self.state_in_rcdata();
                            } else {
                                self.state = State::Text;
                                self.section_start = self.index;
                            }

                            let r = String::from_utf8(self.input[self.index..].to_vec());

                            let _ = r;

                            return;
                        } else if c == SLASH {
                            self.index += 1;
                            if self.index < self.input.len() && self.input[self.index] == GT {
                                // self.v_pre_depth -= 1;
                                // self.in_v_pre = self.v_pre_depth > 0;

                                self.emit_self_closing_tag(self.index as u32);
                                self.index += 1;
                                self.state = State::Text;
                                self.section_start = self.index;
                                return;
                            }
                        } else if is_whitespace(c) {
                            self.index += 1;
                        } else {
                            return self.state_in_before_attr_name();
                        }
                    }
                }

                self.emit(Event::DirName {
                    start: self.section_start as u32,
                    end: self.index as u32,
                });
                if c == COLON {
                    self.index += 1;
                    self.section_start = self.index;
                    self.state = State::InDirArg;
                    self.state_in_dir_arg();
                } else if c == DOT {
                    self.index += 1;
                    self.section_start = self.index;
                    self.state = State::InDirModifier;
                    self.state_in_dir_modifier();
                } else if c == GT {
                    self.emit(Event::AttribNameEnd {
                        end: self.index as u32,
                    });
                    self.emit(Event::AttribEnd {
                        quote: QuoteType::NoValue,
                        end: self.index as u32,
                    });
                    self.emit_open_tag_end(self.index as u32);
                    self.index += 1;
                    self.state = State::Text;
                    self.section_start = self.index;
                } else if c == SLASH {
                    self.emit(Event::AttribNameEnd {
                        end: self.index as u32,
                    });
                    self.emit(Event::AttribEnd {
                        quote: QuoteType::NoValue,
                        end: self.index as u32,
                    });
                    self.index += 1;
                    if self.index < self.input.len() && self.input[self.index] == GT {
                        self.emit_self_closing_tag(self.index as u32);
                        self.index += 1;
                        self.state = State::Text;
                        self.section_start = self.index;
                    } else {
                        self.state = State::BeforeAttrName;
                        self.state_in_before_attr_name();
                    }
                } else if c == EQ {
                    self.emit(Event::AttribNameEnd {
                        end: self.index as u32,
                    });
                    self.index += 1;
                    self.state = State::BeforeAttrValue;
                    self.state_before_attr_value();
                } else {
                    self.handle_attr_name_end();
                }
                return;
            } else {
                self.index += 1;
            }
        }
    }

    fn state_in_dir_arg(&mut self) {
        while self.index < self.input.len() {
            let c = self.input[self.index];
            if c == LEFT_SQUARE {
                self.state = State::InDirDynamicArg;
                self.index += 1;
                self.state_in_dir_dynamic_arg();
                return;
            } else if is_end_of_tag_section(c) || c == EQ || c == DOT {
                self.emit(Event::DirArg {
                    is_dynamic: self.state == State::InDirDynamicArg,
                    start: self.section_start as u32,
                    end: self.index as u32,
                });

                if c == DOT {
                    self.index += 1;
                    self.section_start = self.index;
                    self.state = State::InDirModifier;
                    self.state_in_dir_modifier();
                    return;
                } else if c == GT {
                    self.emit(Event::AttribNameEnd {
                        end: self.index as u32,
                    });
                    self.emit(Event::AttribEnd {
                        quote: QuoteType::NoValue,
                        end: self.index as u32,
                    });
                    self.emit_open_tag_end(self.index as u32);
                    self.index += 1;
                    self.state = State::Text;
                    self.section_start = self.index;
                    return;
                } else if c == SLASH {
                    self.emit(Event::AttribNameEnd {
                        end: self.index as u32,
                    });
                    self.emit(Event::AttribEnd {
                        quote: QuoteType::NoValue,
                        end: self.index as u32,
                    });
                    self.index += 1;
                    if self.index < self.input.len() && self.input[self.index] == GT {
                        self.emit_self_closing_tag(self.index as u32);
                        self.index += 1;
                        self.state = State::Text;
                        self.section_start = self.index;
                    } else {
                        self.state = State::BeforeAttrName;
                        self.state_in_before_attr_name();
                    }
                    return;
                } else if c == EQ {
                    self.emit(Event::AttribNameEnd {
                        end: self.index as u32,
                    });
                    self.index += 1;
                    self.state = State::BeforeAttrValue;
                    self.state_before_attr_value();
                    return;
                } else {
                    self.handle_attr_name_end();
                    return;
                }
            } else {
                self.index += 1;
            }
        }
    }

    fn state_in_dir_dynamic_arg(&mut self) {
        let mut bracket_count = 1;
        self.state = State::InDirDynamicArg;
        while self.index < self.input.len() {
            let c = self.input[self.index];
            if c == RIGHT_SQUARE {
                bracket_count -= 1;
                if bracket_count == 0 {
                    self.index += 1;
                    self.state_in_dir_arg();
                    return;
                } else {
                    self.index += 1;
                }
            } else if c == LEFT_SQUARE {
                bracket_count += 1;
                self.index += 1;
            } else {
                self.index += 1;
            }
        }
    }

    fn state_in_dir_modifier(&mut self) {
        while self.index < self.input.len() {
            let c = self.input[self.index];
            if is_end_of_tag_section(c) || c == EQ || c == DOT {
                self.emit(Event::DirModifier {
                    start: self.section_start as u32,
                    end: self.index as u32,
                });

                if c == DOT {
                    self.index += 1;
                    self.section_start = self.index;
                } else if c == GT {
                    self.emit(Event::AttribNameEnd {
                        end: self.index as u32,
                    });
                    self.emit(Event::AttribEnd {
                        quote: QuoteType::NoValue,
                        end: self.index as u32,
                    });
                    self.emit_open_tag_end(self.index as u32);
                    self.index += 1;
                    self.state = State::Text;
                    self.section_start = self.index;
                    return;
                } else if c == SLASH {
                    self.emit(Event::AttribNameEnd {
                        end: self.index as u32,
                    });
                    self.emit(Event::AttribEnd {
                        quote: QuoteType::NoValue,
                        end: self.index as u32,
                    });
                    self.index += 1;
                    if self.index < self.input.len() && self.input[self.index] == GT {
                        self.emit_self_closing_tag(self.index as u32);
                        self.index += 1;
                        self.state = State::Text;
                        self.section_start = self.index;
                    } else {
                        self.state = State::BeforeAttrName;
                        self.state_in_before_attr_name();
                    }
                    return;
                } else if c == EQ {
                    self.emit(Event::AttribNameEnd {
                        end: self.index as u32,
                    });
                    self.index += 1;
                    self.state = State::BeforeAttrValue;
                    self.state_before_attr_value();
                    return;
                } else {
                    self.handle_attr_name_end();
                    return;
                }
            } else {
                self.index += 1;
            }
        }
    }

    fn state_in_attr_name(&mut self) {
        while self.index < self.input.len() {
            let c = self.input[self.index];
            if is_end_of_tag_section(c) || c == EQ {
                self.emit(Event::AttribName {
                    start: self.section_start as u32,
                    end: self.index as u32,
                });

                if c == EQ {
                    self.emit(Event::AttribNameEnd {
                        end: self.index as u32,
                    });
                    self.index += 1;
                    self.state = State::BeforeAttrValue;
                    self.state_before_attr_value();
                } else {
                    self.handle_attr_name_end();
                }
                return;
            } else {
                self.index += 1;
            }
        }
    }

    fn handle_attr_name_end(&mut self) {
        self.section_start = self.index;
        self.state = State::AfterAttrName;
        self.emit(Event::AttribNameEnd {
            end: self.index as u32,
        });
        self.state_after_attr_name();
    }

    fn state_after_attr_name(&mut self) {
        while self.index < self.input.len() && is_whitespace(self.input[self.index]) {
            self.index += 1;
        }
        if self.index >= self.input.len() {
            return;
        }
        // convert to string
        let _curr = String::from_utf8(self.input[self.index..].to_vec()).unwrap_or_default();
        // log(&format!("Entering v-pre mode, remaining input: {}", curr));

        let c = self.input[self.index];
        if c == EQ {
            self.state = State::BeforeAttrValue;
            self.index += 1;
            self.state_before_attr_value();
        } else if c == SLASH || c == GT {
            self.emit(Event::AttribEnd {
                quote: QuoteType::NoValue,
                end: self.section_start as u32,
            });
            self.state = State::BeforeAttrName;
            self.state_in_before_attr_name();
        } else {
            self.emit(Event::AttribEnd {
                quote: QuoteType::NoValue,
                end: self.section_start as u32,
            });
            self.state_in_before_attr_name();
        }
    }

    fn state_before_attr_value(&mut self) {
        if self.index >= self.input.len() {
            return;
        }
        let c = self.input[self.index];
        if c == DOUBLE_QUOTE || c == SINGLE_QUOTE {
            self.index += 1;
            self.section_start = self.index;

            match find_unescaped(c, &self.input[self.index..], BACKSLASH) {
                Some(p) => {
                    self.index += p;
                    self.emit(Event::AttribData {
                        start: self.section_start as u32,
                        end: self.index as u32,
                    });
                    let quote_type = if c == DOUBLE_QUOTE {
                        QuoteType::Double
                    } else {
                        QuoteType::Single
                    };
                    self.index += 1;
                    self.emit(Event::AttribEnd {
                        quote: quote_type,
                        end: self.index as u32,
                    });
                    self.state = State::BeforeAttrName;
                    self.section_start = self.index;
                    self.state_in_before_attr_name();
                }
                None => {
                    self.index = self.input.len();
                }
            }
        } else if !is_whitespace(c) {
            self.state = State::InAttrValueNq;
            self.section_start = self.index;

            if c == GT {
                self.emit(Event::AttribData {
                    start: self.section_start as u32,
                    end: self.index as u32,
                });
                self.emit(Event::AttribEnd {
                    quote: QuoteType::Unquoted,
                    end: self.index as u32,
                });
                self.index += 1;
                self.state = State::BeforeAttrName;
                self.section_start = self.index;
                self.state_in_before_attr_name();
                return;
            }

            self.state_in_attr_value_nq();
        } else {
            self.index += 1;
            self.state_before_attr_value();
        }
    }

    fn state_in_attr_value_nq(&mut self) {
        while self.index < self.input.len() {
            let c = self.input[self.index];
            if is_whitespace(c) || c == GT {
                self.emit(Event::AttribData {
                    start: self.section_start as u32,
                    end: self.index as u32,
                });
                self.emit(Event::AttribEnd {
                    quote: QuoteType::Unquoted,
                    end: self.index as u32,
                });
                self.state = State::BeforeAttrName;
                self.section_start = self.index;
                self.state_in_before_attr_name();
                return;
            } else {
                self.index += 1;
            }
        }
    }
}

#[inline(always)]
fn next_bytes_equal(needle: &[u8], haystack: &[u8]) -> bool {
    haystack.get(..needle.len()) == Some(needle)
}

#[inline(always)]
fn find_subslice(needle: &[u8], haystack: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    let first = needle[0];
    let mut i = 0;

    while i + needle.len() <= haystack.len() {
        let pos = memchr(first, &haystack[i..])?;
        let at = i + pos;

        if haystack.get(at..at + needle.len()) == Some(needle) {
            return Some(at);
        }
        i = at + 1;
    }
    None
}

#[inline(always)]
fn find_unescaped(needle: u8, haystack: &[u8], escape: u8) -> Option<usize> {
    let mut i = 0;
    while i < haystack.len() {
        let pos = memchr(needle, &haystack[i..])?;
        let at = i + pos;

        if at == 0 || haystack[at - 1] != escape {
            return Some(at);
        }
        i = at + 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to collect all events from tokenizing input
    fn collect_events(input: &str) -> Vec<Event> {
        let mut events = Vec::new();
        tokenize(input.as_bytes(), |event| events.push(event));
        events
    }

    // ==================== Basic tokenization tests ====================

    #[test]
    fn test_basic_element() {
        let events = collect_events("<div>hello</div>");

        assert!(events
            .iter()
            .any(|e| matches!(e, Event::OpenTagName { start: 0, end: 4 })));
        assert!(events
            .iter()
            .any(|e| matches!(e, Event::OpenTagEnd { end: 4 })));
        assert!(events
            .iter()
            .any(|e| matches!(e, Event::Text { start: 5, end: 10 })));
        assert!(events.iter().any(|e| matches!(
            e,
            Event::CloseTag {
                start: 10,
                end: 16,
                ..
            }
        )));
    }

    #[test]
    fn test_close_tag_name_end() {
        let input = "<div>hello</div>";
        let events = collect_events(input);

        // Find the CloseTag event and verify name_end allows correct name extraction
        let close_tag = events.iter().find_map(|e| {
            if let Event::CloseTag {
                start,
                end,
                name_end,
            } = e
            {
                Some((*start, *end, *name_end))
            } else {
                None
            }
        });

        let (start, end, name_end) = close_tag.expect("Should have CloseTag event");

        // Verify the name can be extracted with slice(start + 2, name_end)
        let name = &input[start as usize + 2..name_end as usize];
        assert_eq!(
            name, "div",
            "slice(start + 2, name_end) should give the tag name"
        );

        // Verify end is after the closing >
        assert_eq!(
            &input[end as usize - 1..end as usize],
            ">",
            "end should be after >"
        );
    }

    #[test]
    fn test_close_tag_name_end_with_whitespace() {
        // Close tag with whitespace before >: </div >
        let input = "<div></div  >";
        let events = collect_events(input);

        let close_tag = events.iter().find_map(|e| {
            if let Event::CloseTag {
                start,
                end,
                name_end,
            } = e
            {
                Some((*start, *end, *name_end))
            } else {
                None
            }
        });

        let (start, _end, name_end) = close_tag.expect("Should have CloseTag event");

        // Verify the name can be extracted correctly even with trailing whitespace
        let name = &input[start as usize + 2..name_end as usize];
        assert_eq!(
            name, "div",
            "slice(start + 2, name_end) should give the tag name without whitespace"
        );
    }

    #[test]
    fn test_close_tag_name_end_with_newline_before_gt() {
        // Close tag split across lines: </CButton\n>
        // The > should be part of the closing tag, NOT text content
        // This pattern is common with Prettier-formatted Vue templates
        let input = "<div><span>text</span\n>more</div>";
        let events = collect_events(input);

        // Collect all text events
        let text_events: Vec<&str> = events
            .iter()
            .filter_map(|e| {
                if let Event::Text { start, end } = e {
                    Some(&input[*start as usize..*end as usize])
                } else {
                    None
                }
            })
            .collect();

        // "text" should be one text node, "more" should be another
        // Neither should contain ">" as stray content
        assert!(
            text_events.contains(&"text"),
            "should have 'text' node, got: {:?}",
            text_events
        );
        assert!(
            text_events.contains(&"more"),
            "should have 'more' node, got: {:?}",
            text_events
        );
        // The > from </span\n> should NOT leak into text
        for text in &text_events {
            assert!(
                !text.starts_with('>'),
                "text '{}' should not start with '>' (leaked from close tag)",
                text
            );
        }
    }

    #[test]
    fn test_self_closing_element() {
        let events = collect_events("<input />");

        assert!(events
            .iter()
            .any(|e| matches!(e, Event::OpenTagName { .. })));
        assert!(events
            .iter()
            .any(|e| matches!(e, Event::SelfClosingTag { .. })));
        // Should NOT have OpenTagEnd for self-closing
        assert!(!events.iter().any(|e| matches!(e, Event::OpenTagEnd { .. })));
    }

    #[test]
    fn test_consecutive_self_closing_elements() {
        let events = collect_events("<a/><b/>");

        let self_closing_count = events
            .iter()
            .filter(|e| matches!(e, Event::SelfClosingTag { .. }))
            .count();
        assert_eq!(self_closing_count, 2, "Expected 2 SelfClosingTag events");

        let open_tag_names: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                Event::OpenTagName { start, end } => Some((*start, *end)),
                _ => None,
            })
            .collect();

        assert_eq!(open_tag_names.len(), 2, "Expected 2 OpenTagName events");
        assert_eq!(open_tag_names[0], (0, 2), "First tag name at 0-2");
        assert_eq!(open_tag_names[1], (4, 6), "Second tag name at 4-6");
    }

    // ==================== Interpolation tests ====================

    #[test]
    fn test_interpolation_basic() {
        let events = collect_events("{{ msg }}");

        assert!(events.iter().any(|e| matches!(
            e,
            Event::Interpolation {
                start: 0,
                end: 9,
                ..
            }
        )));
    }

    #[test]
    fn test_interpolation_in_element() {
        let events = collect_events("<div>{{ msg }}</div>");

        assert!(events.iter().any(|e| matches!(
            e,
            Event::Interpolation {
                start: 5,
                end: 14,
                ..
            }
        )));
    }

    // ==================== v-pre directive tests ====================

    #[test]
    fn test_v_pre_directive_detected() {
        let events = collect_events("<span v-pre></span>");

        assert!(
            events.iter().any(|e| matches!(e, Event::DirVPre { .. })),
            "Should detect v-pre directive"
        );
    }

    #[test]
    fn test_v_pre_converts_interpolation_to_text() {
        let events = collect_events("<span v-pre>{{ msg }}</span>");

        // Inside v-pre, interpolation should NOT be detected (emitted as text)
        let has_interpolation = events
            .iter()
            .any(|e| matches!(e, Event::Interpolation { .. }));
        assert!(
            !has_interpolation,
            "Interpolation inside v-pre should NOT be emitted as Interpolation"
        );

        // Should have text instead
        let has_text = events.iter().any(|e| matches!(e, Event::Text { .. }));
        assert!(has_text, "Content inside v-pre should be emitted as Text");
    }

    #[test]
    fn test_v_pre_scope_exits_on_close_tag() {
        let events = collect_events("<span v-pre>{{ raw }}</span>{{ compiled }}");

        // Count interpolations - only the one OUTSIDE v-pre should be Interpolation
        let interpolations: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                Event::Interpolation { start, end, .. } => Some((*start, *end)),
                _ => None,
            })
            .collect();

        assert_eq!(
            interpolations.len(),
            1,
            "Only {{ compiled }} should be Interpolation, got {:?}",
            interpolations
        );

        // The interpolation should be at position 28 (after </span>)
        assert_eq!(
            interpolations[0].0, 28,
            "Interpolation should start at 28 (after </span>)"
        );
    }

    #[test]
    fn test_v_pre_nested_elements() {
        let events = collect_events("<div v-pre><a>{{ msg }}</a></div>{{ outside }}");

        // Interpolation inside nested element should be text
        // Interpolation outside should be Interpolation
        let interpolations: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, Event::Interpolation { .. }))
            .collect();

        assert_eq!(
            interpolations.len(),
            1,
            "Only {{ outside }} should be Interpolation"
        );
    }

    // ==================== v-pre with self-closing element ====================

    /// Self-closing elements with v-pre correctly exit v-pre scope.
    #[test]
    fn test_v_pre_self_closing_exits_scope() {
        let events = collect_events("<input v-pre />{{ after }}");

        let interpolations: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, Event::Interpolation { .. }))
            .collect();

        assert_eq!(
            interpolations.len(),
            1,
            "{{ after }} should be Interpolation after self-closing v-pre"
        );

        let text = events.iter().find(|e| matches!(e, Event::Text { .. }));
        assert!(
            text.is_none(),
            "Self-closing v-pre should not emit Text content"
        );
    }

    #[test]
    fn test_v_pre_self_closing() {
        let events = collect_events("<input v-pre/>{{ msg }}");

        // Inside v-pre, interpolation should NOT be detected (emitted as text)
        let has_interpolation = events
            .iter()
            .any(|e| matches!(e, Event::Interpolation { .. }));
        assert!(
            has_interpolation,
            "Interpolation after self-closing v-pre should be emitted as Interpolation"
        );

        let text = events.iter().find(|e| matches!(e, Event::Text { .. }));
        assert!(
            text.is_none(),
            "Self-closing v-pre should not emit Text content"
        );
    }
    #[test]
    fn test_v_pre_closing() {
        let events = collect_events("<div v-pre></div>{{ msg }}");

        // Inside v-pre, interpolation should NOT be detected (emitted as text)
        let has_interpolation = events
            .iter()
            .any(|e| matches!(e, Event::Interpolation { .. }));
        assert!(
            has_interpolation,
            "Interpolation after closing v-pre should be emitted as Interpolation"
        );

        let text = events.iter().find(|e| matches!(e, Event::Text { .. }));
        assert!(text.is_none(), "Closing v-pre should not emit Text content");
    }
    #[test]
    fn test_v_pre() {
        let events = collect_events("<div v-pre>{{ msg }}</div>");

        let has_interpolation = events
            .iter()
            .any(|e| matches!(e, Event::Interpolation { .. }));
        assert!(
            !has_interpolation,
            "Interpolation in v-pre should not be emitted"
        );

        let text = events.iter().find(|e| matches!(e, Event::Text { .. }));
        assert!(
            text.is_some(),
            "v-pre should emit Text content for interpolation"
        );
    }
    #[test]
    fn test_v_pre_spaced() {
        let events = collect_events("<div v-pre  >{{ msg }}</div>");

        let has_interpolation = events
            .iter()
            .any(|e| matches!(e, Event::Interpolation { .. }));
        assert!(
            !has_interpolation,
            "Interpolation in v-pre should not be emitted"
        );

        let text = events.iter().find(|e| matches!(e, Event::Text { .. }));
        assert!(
            text.is_some(),
            "v-pre should emit Text content for interpolation"
        );
    }

    /// Self-closing and normal v-pre elements behave the same way.
    #[test]
    fn test_v_pre_self_closing_vs_normal_element() {
        // With normal element
        let events_normal = collect_events("<span v-pre></span>{{ after }}");
        let interp_normal = events_normal
            .iter()
            .filter(|e| matches!(e, Event::Interpolation { .. }))
            .count();

        // With self-closing element
        let events_self_closing = collect_events("<input v-pre />{{ after }}");
        let interp_self_closing = events_self_closing
            .iter()
            .filter(|e| matches!(e, Event::Interpolation { .. }))
            .count();

        // Both should have 1 interpolation (v-pre scope exits correctly)
        assert_eq!(
            interp_normal, 1,
            "Normal v-pre element should exit scope correctly"
        );
        assert_eq!(
            interp_self_closing, 1,
            "Self-closing v-pre element should exit scope correctly"
        );
        assert_eq!(
            interp_normal, interp_self_closing,
            "Both should behave the same"
        );
    }

    /// Multiple self-closing v-pre elements each exit their scope independently.
    #[test]
    fn test_multiple_self_closing_v_pre_each_exit_scope() {
        let events = collect_events("<input v-pre /><input v-pre />{{ after }}");

        let interpolations: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, Event::Interpolation { .. }))
            .collect();

        assert_eq!(
            interpolations.len(),
            1,
            "{{ after }} should be Interpolation after multiple self-closing v-pre"
        );
    }

    // ==================== Edge cases ====================

    #[test]
    fn test_v_pre_sibling_scope_isolation() {
        let events = collect_events("<p v-pre>{{ a }}</p><p>{{ b }}</p>");

        // Only {{ b }} should be Interpolation
        let interpolations: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, Event::Interpolation { .. }))
            .collect();

        assert_eq!(
            interpolations.len(),
            1,
            "Only {{ b }} should be Interpolation"
        );
    }

    #[test]
    fn test_nested_v_pre_ignored() {
        // Inner v-pre should be ignored since already in v-pre scope
        let events = collect_events("<div v-pre><span v-pre>{{ msg }}</span></div>{{ outside }}");

        let interpolations: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, Event::Interpolation { .. }))
            .collect();

        assert_eq!(
            interpolations.len(),
            1,
            "Only {{ outside }} should be Interpolation"
        );
    }

    // ==================== v-pre attribute ordering tests ====================
    // The tokenizer processes attributes sequentially without lookahead.
    // This means v-pre must be detected before OpenTagEnd is emitted.
    // When v-pre comes after other directives, behavior should be the same.

    #[test]
    fn test_v_pre_first_with_other_attributes() {
        // v-pre comes first - this is the "canonical" form
        let events = collect_events(r#"<div v-pre v-if="1 > 2">{{ foo }}</div>"#);

        // Interpolation inside v-pre should be text, not Interpolation
        let interpolations: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, Event::Interpolation { .. }))
            .collect();

        let dirnames: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, Event::DirName { .. }))
            .collect();

        assert_eq!(
            interpolations.len(),
            0,
            "{{ foo }} inside v-pre should NOT be Interpolation (v-pre first)"
        );

        assert_eq!(dirnames.len(), 0, "v-pre directive should no be detected");
    }

    #[test]
    fn test_v_prex_first_with_other_attributes() {
        // v-pre comes first - this is the "canonical" form
        let events = collect_events(r#"<div v-prex v-if="1 > 2">{{ foo }}</div>"#);

        // Interpolation inside v-pre should be text, not Interpolation
        let interpolations: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, Event::Interpolation { .. }))
            .collect();

        let dirnames: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, Event::DirName { .. }))
            .collect();

        assert_eq!(
            interpolations.len(),
            1,
            "{{ foo }} inside v-pre should NOT be Interpolation (v-pre first)"
        );

        assert_eq!(dirnames.len(), 2, "v-pre directive should be detected");
    }

    #[test]
    #[should_panic(expected = "V-pre directive should not be detected")]
    fn test_v_pre_last_with_other_attributes() {
        // v-pre comes after v-if - should behave the same as when v-pre comes first
        let events = collect_events(r#"<div v-if="1 > 2" v-pre>{{ foo }}</div>"#);

        let interpolations: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, Event::Interpolation { .. }))
            .collect();

        let dirnames: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, Event::DirName { .. }))
            .collect();

        assert_eq!(
            interpolations.len(),
            0,
            "{{ foo }} inside v-pre should NOT be Interpolation (v-pre last)"
        );
        assert_eq!(dirnames.len(), 0, "V-pre directive should not be detected");
    }

    #[test]
    fn test_v_pre_attribute_order_equivalence() {
        // Both orderings should produce equivalent behavior for content inside
        let input_v_pre_first = r#"<div v-pre v-if="1 > 2">{{ foo }}</div>"#;
        let input_v_pre_last = r#"<div v-if="1 > 2" v-pre>{{ foo }}</div>"#;

        let events_first = collect_events(input_v_pre_first);
        let events_last = collect_events(input_v_pre_last);

        let interp_first = events_first
            .iter()
            .filter(|e| matches!(e, Event::Interpolation { .. }))
            .count();
        let interp_last = events_last
            .iter()
            .filter(|e| matches!(e, Event::Interpolation { .. }))
            .count();

        assert_eq!(
            interp_first, interp_last,
            "v-pre attribute order should not affect interpolation handling. \
             First: {} interpolations, Last: {} interpolations",
            interp_first, interp_last
        );
    }

    #[test]
    fn test_v_pre_with_gt_in_attribute_value() {
        // Test with > character in attribute value (common in v-if conditions)
        let events = collect_events(r#"<span v-pre :class="x > 0 ? 'a' : 'b'">{{ msg }}</span>"#);

        let interpolations: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, Event::Interpolation { .. }))
            .collect();

        assert_eq!(
            interpolations.len(),
            0,
            "{{ msg }} inside v-pre should be text, not Interpolation"
        );
    }

    #[test]
    fn test_v_pre_after_gt_in_attribute() {
        // The problematic case: v-pre comes AFTER an attribute containing >
        let events = collect_events(r#"<span :class="x > 0 ? 'a' : 'b'" v-pre>{{ msg }}</span>"#);

        let interpolations: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, Event::Interpolation { .. }))
            .collect();

        // This documents the expected behavior - both orderings should be equivalent
        assert_eq!(
            interpolations.len(),
            0,
            "{{ msg }} inside v-pre should be text even when v-pre comes after attribute with >"
        );
    }

    // ==================== Directive dynamic argument tests ====================

    #[test]
    fn test_directive_static_argument() {
        let events = collect_events("<div v-foo:arg />");

        let dir_args: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                Event::DirArg { is_dynamic, .. } => Some(*is_dynamic),
                _ => None,
            })
            .collect();

        assert_eq!(dir_args.len(), 1, "Should have one DirArg event");
        assert_eq!(
            dir_args[0], false,
            "Static argument should have is_dynamic=false"
        );
    }

    #[test]
    fn test_directive_dynamic_argument() {
        let events = collect_events("<div v-foo:[arg] />");

        let dir_args: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                Event::DirArg { is_dynamic, .. } => Some(*is_dynamic),
                _ => None,
            })
            .collect();

        assert_eq!(dir_args.len(), 1, "Should have one DirArg event");
        assert_eq!(
            dir_args[0], true,
            "Dynamic argument should have is_dynamic=true"
        );
    }

    #[test]
    fn test_directive_dynamic_argument_nested_brackets() {
        let events = collect_events("<div v-foo:[arr[0]] />");

        let dir_args: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                Event::DirArg { is_dynamic, .. } => Some(*is_dynamic),
                _ => None,
            })
            .collect();

        assert_eq!(dir_args.len(), 1, "Should have one DirArg event");
        assert_eq!(
            dir_args[0], true,
            "Dynamic argument with nested brackets should have is_dynamic=true"
        );
    }

    #[test]
    fn test_directive_static_vs_dynamic_arguments() {
        let events = collect_events("<div v-foo:static v-bar:[dynamic] />");

        let dir_args: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                Event::DirArg { is_dynamic, .. } => Some(*is_dynamic),
                _ => None,
            })
            .collect();

        assert_eq!(dir_args.len(), 2, "Should have two DirArg events");
        assert_eq!(dir_args[0], false, "First argument should be static");
        assert_eq!(dir_args[1], true, "Second argument should be dynamic");
    }

    #[test]
    fn test_directive_dynamic_argument_with_modifier() {
        let events = collect_events("<div v-foo:[arg].mod />");

        let dir_args: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                Event::DirArg { is_dynamic, .. } => Some(*is_dynamic),
                _ => None,
            })
            .collect();

        assert_eq!(dir_args.len(), 1, "Should have one DirArg event");
        assert_eq!(
            dir_args[0], true,
            "Dynamic argument with modifier should have is_dynamic=true"
        );
    }

    #[test]
    fn test_directive_static_argument_with_modifier() {
        let events = collect_events("<div v-foo:arg.mod />");

        let dir_args: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                Event::DirArg { is_dynamic, .. } => Some(*is_dynamic),
                _ => None,
            })
            .collect();

        assert_eq!(dir_args.len(), 1, "Should have one DirArg event");
        assert_eq!(
            dir_args[0], false,
            "Static argument with modifier should have is_dynamic=false"
        );
    }

    #[test]
    fn test_skip_script() {
        let events = collect_events("<script><div v-foo:arg.mod /> </script>");

        let dir_args: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                Event::DirArg { is_dynamic, .. } => Some(*is_dynamic),
                _ => None,
            })
            .collect();

        assert_eq!(
            dir_args.len(),
            0,
            "Should have no DirArg events inside script"
        );
    }

    #[test]
    fn test_not_send_first_child_text_node_if_empty() {
        let events = collect_events("<div>    <span></span></div>");

        let text_nodes: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, Event::Text { .. }))
            .collect();

        assert_eq!(
            text_nodes.len(),
            0,
            "Should have no Text events for empty div"
        );
    }

    #[test]
    fn test_send_v_pre_directive_not_directive_or_attribute() {
        let events = collect_events("<div v-pre></div>");
        let v_pre_events: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, Event::DirVPre { .. }))
            .collect();
        assert_eq!(v_pre_events.len(), 1, "Should have one DirVPre event");

        let other_dir_events: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, Event::DirName { .. }))
            .collect();
        assert_eq!(
            other_dir_events.len(),
            0,
            "Should have no other DirName events"
        );
    }

    // ==================== Offset validation tests ====================
    // These tests verify that the offsets in events actually correspond to the correct
    // slices of the input source. This ensures the tokenizer produces correct byte ranges.

    #[test]
    fn test_tag_name_offsets() {
        let input = "<div></div>";
        let events = collect_events(input);

        // OpenTagName includes the "<" character
        let open_tag = events.iter().find_map(|e| match e {
            Event::OpenTagName { start, end } => Some((*start, *end)),
            _ => None,
        });
        assert!(open_tag.is_some(), "Should have OpenTagName event");
        let (start, end) = open_tag.unwrap();
        assert_eq!(
            &input[start as usize..end as usize],
            "<div",
            "OpenTagName offsets [{}:{}] should match '<div' (includes <)",
            start,
            end
        );
        // The actual tag name without < is at [start+1..end]
        assert_eq!(
            &input[start as usize + 1..end as usize],
            "div",
            "Tag name without '<' should be 'div'"
        );

        // CloseTag should capture "</div>"
        let close_tag = events.iter().find_map(|e| match e {
            Event::CloseTag {
                start,
                end,
                name_end,
            } => Some((*start, *end, *name_end)),
            _ => None,
        });
        assert!(close_tag.is_some(), "Should have CloseTag event");
        let (start, end, name_end) = close_tag.unwrap();
        assert_eq!(
            &input[start as usize..end as usize],
            "</div>",
            "CloseTag offsets [{}:{}] should match '</div>'",
            start,
            end
        );
        assert_eq!(
            &input[start as usize + 2..name_end as usize],
            "div",
            "CloseTag name offsets [{}:{}] should match 'div'",
            start + 2,
            name_end
        );
    }

    #[test]
    fn test_attribute_name_offsets() {
        let input = r#"<div class="hello"></div>"#;
        let events = collect_events(input);

        let attrib_name = events.iter().find_map(|e| match e {
            Event::AttribName { start, end } => Some((*start, *end)),
            _ => None,
        });
        assert!(attrib_name.is_some(), "Should have AttribName event");
        let (start, end) = attrib_name.unwrap();
        assert_eq!(
            &input[start as usize..end as usize],
            "class",
            "AttribName offsets [{}:{}] should match 'class'",
            start,
            end
        );
    }

    #[test]
    fn test_attribute_data_offsets() {
        let input = r#"<div class="hello"></div>"#;
        let events = collect_events(input);

        let attrib_data = events.iter().find_map(|e| match e {
            Event::AttribData { start, end } => Some((*start, *end)),
            _ => None,
        });
        assert!(attrib_data.is_some(), "Should have AttribData event");
        let (start, end) = attrib_data.unwrap();
        assert_eq!(
            &input[start as usize..end as usize],
            "hello",
            "AttribData offsets [{}:{}] should match 'hello' (without quotes)",
            start,
            end
        );
    }

    #[test]
    fn test_attribute_end_offsets() {
        let input = r#"<div class="hello"></div>"#;
        let events = collect_events(input);

        let name_start = events
            .iter()
            .find_map(|e| match e {
                Event::AttribName { start, .. } => Some(*start),
                _ => None,
            })
            .expect("Should have AttribName");

        let attrib_end = events.iter().find_map(|e| match e {
            Event::AttribEnd { end, .. } => Some(*end),
            _ => None,
        });
        assert!(attrib_end.is_some(), "Should have AttribEnd event");
        let end = attrib_end.unwrap();

        // The full attribute from name_start to end should be class="hello"
        assert_eq!(
            &input[name_start as usize..end as usize],
            r#"class="hello""#,
            "Full attribute [{}:{}] should be 'class=\"hello\"'",
            name_start,
            end
        );
    }

    #[test]
    fn test_attribute_offsets_in_template() {
        let input = "<template>\n<div class=\"hello\" v-if=\"show\">\n  {{ message }}\n</template>";
        let events = collect_events(input);

        // Find the class attribute name
        let class_name = events.iter().find_map(|e| match e {
            Event::AttribName { start, end } => {
                if &input[*start as usize..*end as usize] == "class" {
                    Some((*start, *end))
                } else {
                    None
                }
            }
            _ => None,
        });
        assert!(class_name.is_some(), "Should find class attribute");
        let (name_start, name_end) = class_name.unwrap();

        assert_eq!(
            &input[name_start as usize..name_end as usize],
            "class",
            "Class name offsets [{}:{}] should match 'class'",
            name_start,
            name_end
        );

        // Find AttribEnd for class attribute
        let mut found_class = false;
        let attrib_end = events.iter().find_map(|e| match e {
            Event::AttribName { start, .. } if *start == name_start => {
                found_class = true;
                None
            }
            Event::AttribEnd { end, .. } if found_class => {
                found_class = false;
                Some(*end)
            }
            _ => None,
        });

        assert!(attrib_end.is_some(), "Should find AttribEnd for class");
        let end = attrib_end.unwrap();

        let full_attr = &input[name_start as usize..end as usize];
        assert_eq!(
            full_attr, r#"class="hello""#,
            "Full class attribute [{}:{}] should be 'class=\"hello\"'",
            name_start, end
        );
    }

    #[test]
    fn test_attribute_single_quote_offsets() {
        let input = "<div class='world'></div>";
        let events = collect_events(input);

        let name_start = events
            .iter()
            .find_map(|e| match e {
                Event::AttribName { start, .. } => Some(*start),
                _ => None,
            })
            .expect("Should have AttribName");

        let data = events.iter().find_map(|e| match e {
            Event::AttribData { start, end } => Some((*start, *end)),
            _ => None,
        });
        assert!(data.is_some(), "Should have AttribData");
        let (data_start, data_end) = data.unwrap();
        assert_eq!(
            &input[data_start as usize..data_end as usize],
            "world",
            "AttribData should match 'world' without quotes"
        );

        let end = events
            .iter()
            .find_map(|e| match e {
                Event::AttribEnd { end, .. } => Some(*end),
                _ => None,
            })
            .expect("Should have AttribEnd");

        assert_eq!(
            &input[name_start as usize..end as usize],
            "class='world'",
            "Full attribute should be 'class='world''"
        );
    }

    #[test]
    fn test_attribute_unquoted_offsets() {
        let input = "<div id=test></div>";
        let events = collect_events(input);

        let name_start = events
            .iter()
            .find_map(|e| match e {
                Event::AttribName { start, .. } => Some(*start),
                _ => None,
            })
            .expect("Should have AttribName");

        let data = events.iter().find_map(|e| match e {
            Event::AttribData { start, end } => Some((*start, *end)),
            _ => None,
        });
        assert!(data.is_some(), "Should have AttribData");
        let (data_start, data_end) = data.unwrap();
        assert_eq!(
            &input[data_start as usize..data_end as usize],
            "test",
            "Unquoted AttribData should match 'test'"
        );

        let end = events
            .iter()
            .find_map(|e| match e {
                Event::AttribEnd { end, .. } => Some(*end),
                _ => None,
            })
            .expect("Should have AttribEnd");

        assert_eq!(
            &input[name_start as usize..end as usize],
            "id=test",
            "Full unquoted attribute should be 'id=test'"
        );
    }

    #[test]
    fn test_multiple_attributes_offsets() {
        let input = r#"<div id="foo" class="bar"></div>"#;
        let events = collect_events(input);

        let attrs: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                Event::AttribName { start, end } => Some((*start, *end)),
                _ => None,
            })
            .collect();

        assert_eq!(attrs.len(), 2, "Should have 2 attributes");

        // First attribute: id
        let (start, end) = attrs[0];
        assert_eq!(
            &input[start as usize..end as usize],
            "id",
            "First attribute name should be 'id'"
        );

        // Second attribute: class
        let (start, end) = attrs[1];
        assert_eq!(
            &input[start as usize..end as usize],
            "class",
            "Second attribute name should be 'class'"
        );
    }

    #[test]
    fn test_text_node_offsets() {
        let input = "<div>hello world</div>";
        let events = collect_events(input);

        let text = events.iter().find_map(|e| match e {
            Event::Text { start, end } => Some((*start, *end)),
            _ => None,
        });
        assert!(text.is_some(), "Should have Text event");
        let (start, end) = text.unwrap();
        assert_eq!(
            &input[start as usize..end as usize],
            "hello world",
            "Text offsets [{}:{}] should match 'hello world'",
            start,
            end
        );
    }

    #[test]
    fn test_interpolation_offsets() {
        let input = "<div>{{ message }}</div>";
        let events = collect_events(input);

        let interp = events.iter().find_map(|e| match e {
            Event::Interpolation { start, end, .. } => Some((*start, *end)),
            _ => None,
        });
        assert!(interp.is_some(), "Should have Interpolation event");
        let (start, end) = interp.unwrap();
        assert_eq!(
            &input[start as usize..end as usize],
            "{{ message }}",
            "Interpolation offsets [{}:{}] should match '{{{{ message }}}}'",
            start,
            end
        );
    }

    #[test]
    fn test_directive_name_offsets() {
        let input = r#"<div v-if="show"></div>"#;
        let events = collect_events(input);

        let dir_name = events.iter().find_map(|e| match e {
            Event::DirName { start, end } => Some((*start, *end)),
            _ => None,
        });
        assert!(dir_name.is_some(), "Should have DirName event");
        let (start, end) = dir_name.unwrap();
        assert_eq!(
            &input[start as usize..end as usize],
            "v-if",
            "DirName offsets [{}:{}] should match 'v-if'",
            start,
            end
        );
    }

    #[test]
    fn test_directive_arg_offsets() {
        let input = r#"<div v-bind:class="active"></div>"#;
        let events = collect_events(input);

        let dir_arg = events.iter().find_map(|e| match e {
            Event::DirArg { start, end, .. } => Some((*start, *end)),
            _ => None,
        });
        assert!(dir_arg.is_some(), "Should have DirArg event");
        let (start, end) = dir_arg.unwrap();
        assert_eq!(
            &input[start as usize..end as usize],
            "class",
            "DirArg offsets [{}:{}] should match 'class'",
            start,
            end
        );
    }

    #[test]
    fn test_directive_modifier_offsets() {
        let input = r#"<button @click.prevent="handler"></button>"#;
        let events = collect_events(input);

        let modifier = events.iter().find_map(|e| match e {
            Event::DirModifier { start, end } => Some((*start, *end)),
            _ => None,
        });
        assert!(modifier.is_some(), "Should have DirModifier event");
        let (start, end) = modifier.unwrap();
        assert_eq!(
            &input[start as usize..end as usize],
            "prevent",
            "DirModifier offsets [{}:{}] should match 'prevent'",
            start,
            end
        );
    }

    #[test]
    fn test_directive_dynamic_arg_offsets() {
        let input = r#"<div v-bind:[key]="value"></div>"#;
        let events = collect_events(input);

        let dir_arg = events.iter().find_map(|e| match e {
            Event::DirArg {
                start,
                end,
                is_dynamic,
            } => {
                if *is_dynamic {
                    Some((*start, *end))
                } else {
                    None
                }
            }
            _ => None,
        });
        assert!(dir_arg.is_some(), "Should have dynamic DirArg event");
        let (start, end) = dir_arg.unwrap();
        // Dynamic arguments include the brackets in the tokenizer output
        assert_eq!(
            &input[start as usize..end as usize],
            "[key]",
            "Dynamic DirArg offsets [{}:{}] should match '[key]' (includes brackets)",
            start,
            end
        );
        // The key itself (without brackets) is at [start+1..end-1]
        assert_eq!(
            &input[start as usize + 1..end as usize - 1],
            "key",
            "Key without brackets should be 'key'"
        );
    }

    #[test]
    fn test_comment_offsets() {
        let input = "<!-- This is a comment -->";
        let events = collect_events(input);

        let comment = events.iter().find_map(|e| match e {
            Event::Comment { start, end } => Some((*start, *end)),
            _ => None,
        });
        assert!(comment.is_some(), "Should have Comment event");
        let (start, end) = comment.unwrap();
        assert_eq!(
            &input[start as usize..end as usize],
            "<!-- This is a comment -->",
            "Comment offsets [{}:{}] should match full comment",
            start,
            end
        );
    }

    #[test]
    fn test_self_closing_tag_offsets() {
        let input = "<input type=\"text\" />";
        let events = collect_events(input);

        let tag_name = events.iter().find_map(|e| match e {
            Event::OpenTagName { start, end } => Some((*start, *end)),
            _ => None,
        });
        assert!(tag_name.is_some(), "Should have OpenTagName event");
        let (start, end) = tag_name.unwrap();
        assert_eq!(
            &input[start as usize..end as usize],
            "<input",
            "Self-closing tag should be '<input' (includes <)"
        );
        assert_eq!(
            &input[start as usize + 1..end as usize],
            "input",
            "Tag name without '<' should be 'input'"
        );

        let self_closing = events.iter().find_map(|e| match e {
            Event::SelfClosingTag { end } => Some(*end),
            _ => None,
        });
        assert!(self_closing.is_some(), "Should have SelfClosingTag event");
    }

    #[test]
    fn test_nested_elements_offsets() {
        let input = "<div><span>text</span></div>";
        let events = collect_events(input);

        let tag_names: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                Event::OpenTagName { start, end } => Some((*start, *end)),
                _ => None,
            })
            .collect();

        assert_eq!(tag_names.len(), 2, "Should have 2 open tags");

        let (start, end) = tag_names[0];
        assert_eq!(
            &input[start as usize..end as usize],
            "<div",
            "First tag should be '<div' (includes <)"
        );
        assert_eq!(
            &input[start as usize + 1..end as usize],
            "div",
            "First tag name without '<' should be 'div'"
        );

        let (start, end) = tag_names[1];
        assert_eq!(
            &input[start as usize..end as usize],
            "<span",
            "Second tag should be '<span' (includes <)"
        );
        assert_eq!(
            &input[start as usize + 1..end as usize],
            "span",
            "Second tag name without '<' should be 'span'"
        );
    }

    #[test]
    fn test_complex_template_offsets() {
        let input = r#"<template>
<div class="hello" v-if="show">
  {{ message }}
  <span @click:foo.bar="onClick">text</span>
</div>
</template>"#;
        let events = collect_events(input);

        // Verify class attribute
        let class_attr = events.iter().find_map(|e| match e {
            Event::AttribName { start, end } => {
                if &input[*start as usize..*end as usize] == "class" {
                    Some((*start, *end))
                } else {
                    None
                }
            }
            _ => None,
        });
        assert!(class_attr.is_some());
        let (start, end) = class_attr.unwrap();
        assert_eq!(&input[start as usize..end as usize], "class");

        // Verify v-if directive
        let v_if = events.iter().find_map(|e| match e {
            Event::DirName { start, end } => {
                if &input[*start as usize..*end as usize] == "v-if" {
                    Some((*start, *end))
                } else {
                    None
                }
            }
            _ => None,
        });
        assert!(v_if.is_some());
        let (start, end) = v_if.unwrap();
        assert_eq!(&input[start as usize..end as usize], "v-if");

        // Verify interpolation
        let interp = events.iter().find_map(|e| match e {
            Event::Interpolation { start, end, .. } => Some((*start, *end)),
            _ => None,
        });
        assert!(interp.is_some());
        let (start, end) = interp.unwrap();
        assert_eq!(&input[start as usize..end as usize], "{{ message }}");

        // Verify directive with argument and modifier
        let dir_arg = events.iter().find_map(|e| match e {
            Event::DirArg { start, end, .. } => {
                let slice = &input[*start as usize..*end as usize];
                if slice.contains("click") {
                    Some((*start, *end))
                } else {
                    None
                }
            }
            _ => None,
        });
        assert!(dir_arg.is_some());
        let (start, end) = dir_arg.unwrap();
        assert_eq!(&input[start as usize..end as usize], "click:foo");

        let modifier = events.iter().find_map(|e| match e {
            Event::DirModifier { start, end } => Some((*start, *end)),
            _ => None,
        });
        assert!(modifier.is_some());
        let (start, end) = modifier.unwrap();
        assert_eq!(&input[start as usize..end as usize], "bar");
    }

    // ==================== Interpolation delimiter length tests ====================

    /// @ai-generated - Tests that default delimiters emit correct delimiter_open_len/delimiter_close_len
    #[test]
    fn test_interpolation_delimiter_lengths_default() {
        let events = collect_events("<div>{{ msg }}</div>");

        let interp = events.iter().find_map(|e| match e {
            Event::Interpolation {
                start,
                end,
                delimiter_open_len,
                delimiter_close_len,
            } => Some((*start, *end, *delimiter_open_len, *delimiter_close_len)),
            _ => None,
        });

        assert!(interp.is_some(), "Should have Interpolation event");
        let (start, end, open_len, close_len) = interp.unwrap();

        assert_eq!(open_len, 2, "Default delimiter_open_len should be 2");
        assert_eq!(close_len, 2, "Default delimiter_close_len should be 2");

        let input = "<div>{{ msg }}</div>";
        assert_eq!(
            &input[start as usize..end as usize],
            "{{ msg }}",
            "Interpolation span should include delimiters"
        );
    }

    /// @ai-generated - Tests custom 3-byte delimiters produce correct spans and lengths
    #[test]
    fn test_interpolation_custom_3byte_delimiters() {
        let input = "<div>[[[value]]]</div>";
        let mut events = Vec::new();
        tokenize_with_delimiters(input.as_bytes(), |event| events.push(event), b"[[[", b"]]]");

        let interp = events.iter().find_map(|e| match e {
            Event::Interpolation {
                start,
                end,
                delimiter_open_len,
                delimiter_close_len,
            } => Some((*start, *end, *delimiter_open_len, *delimiter_close_len)),
            _ => None,
        });

        assert!(interp.is_some(), "Should have Interpolation event");
        let (start, end, open_len, close_len) = interp.unwrap();

        assert_eq!(open_len, 3, "Custom delimiter_open_len should be 3");
        assert_eq!(close_len, 3, "Custom delimiter_close_len should be 3");

        assert_eq!(
            &input[start as usize..end as usize],
            "[[[value]]]",
            "Interpolation span should match '[[[value]]]'"
        );
        // Verify content extraction via offsets
        assert_eq!(
            &input[(start as usize + open_len as usize)..(end as usize - close_len as usize)],
            "value",
            "Content between delimiters should be 'value'"
        );
    }

    /// @ai-generated - Tests custom single-byte delimiters
    #[test]
    fn test_interpolation_single_byte_delimiters() {
        let input = "<div>#msg#</div>";
        let mut events = Vec::new();
        tokenize_with_delimiters(input.as_bytes(), |event| events.push(event), b"#", b"#");

        let interp = events.iter().find_map(|e| match e {
            Event::Interpolation {
                start,
                end,
                delimiter_open_len,
                delimiter_close_len,
            } => Some((*start, *end, *delimiter_open_len, *delimiter_close_len)),
            _ => None,
        });

        assert!(interp.is_some(), "Should have Interpolation event");
        let (start, end, open_len, close_len) = interp.unwrap();

        assert_eq!(open_len, 1, "delimiter_open_len should be 1 for '#'");
        assert_eq!(close_len, 1, "delimiter_close_len should be 1 for '#'");

        assert_eq!(
            &input[start as usize..end as usize],
            "#msg#",
            "Interpolation span should match '#msg#'"
        );
        assert_eq!(
            &input[(start as usize + open_len as usize)..(end as usize - close_len as usize)],
            "msg",
            "Content between delimiters should be 'msg'"
        );
    }
}
