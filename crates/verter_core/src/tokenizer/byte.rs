//! Fast byte-based tokenizer — about 3X throughput of string-based tokenizer.
//! Based on Vue tokenizer.

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
            sequences::{CDATA_END, COMMENT_END, SCRIPT_END, STYLE_END, TEXTAREA_END},
        },
    },
};
use memchr::{memchr, memchr2, memchr3, memmem};

/// All tokenizer states.
///
/// Note: The `run()` loop only dispatches on `Text` and `InRCDATA`. All other states
/// are reached via direct function calls. The state field is written for debug assertions
/// and debugging purposes — a `debug_assert!` in `run()` catches missed transitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum State {
    Text,
    InterpolationOpen,
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
    InAttrValueNq,
    BeforeDeclaration,
    InProcessingInstruction,
    BeforeComment,
    InCommentLike,
    InRCDATA,
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

    /// The current state the tokenizer is in.
    state: State,

    section_start: usize,
    index: usize,
    callback: F,
    delimiter_open: &'a [u8],
    delimiter_close: &'a [u8],
    in_rcdata: bool,
    /// Whether RCDATA mode allows interpolation (true for textarea, false for script/style)
    rcdata_allows_interpolation: bool,
    /// The closing sequence to look for in RCDATA mode (e.g., b"</script")
    current_sequence: &'static [u8],
    /// Index into current_sequence for matching
    sequence_index: usize,
    /// For disabling interpolation parsing in v-pre
    in_v_pre: bool,
    /// Depth counter for v-pre element nesting
    v_pre_depth: usize,
    /// Set by the fast pre-pass when v-pre is found ahead in the attribute area.
    /// This allows v-pre itself to still enter `state_in_dir_name` while other
    /// directives are suppressed.
    v_pre_found_by_prepass: bool,
    /// Cached first byte of open delimiter
    delim_open_first: u8,
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
            section_start: 0,
            index: 0,
            callback,
            delimiter_open,
            delimiter_close,
            in_rcdata: false,
            rcdata_allows_interpolation: false,
            current_sequence: SCRIPT_END,
            sequence_index: 0,
            in_v_pre: false,
            v_pre_depth: 0,
            v_pre_found_by_prepass: false,
            delim_open_first: delimiter_open.first().copied().unwrap_or(LEFT_BRACE),
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

    /// Emit OpenTagEnd with exclusive-end (after the `>`).
    fn emit_open_tag_end(&mut self, gt_index: u32) {
        self.emit(Event::OpenTagEnd { end: gt_index + 1 });
    }

    fn decrement_v_pre_depth(&mut self) {
        if self.v_pre_depth > 0 {
            self.v_pre_depth -= 1;
            if self.v_pre_depth == 0 {
                self.in_v_pre = false;
            }
        } else {
            self.in_v_pre = false;
        }
    }

    fn emit_close_tag(&mut self, start: u32, end: u32, name_end: u32) {
        self.decrement_v_pre_depth();
        self.emit(Event::CloseTag {
            start,
            end,
            name_end,
        });
    }
    fn emit_self_closing_tag(&mut self, end: u32) {
        self.decrement_v_pre_depth();
        self.emit(Event::SelfClosingTag { end });
    }

    /// Emit EOF_IN_TAG error and finalize attribute context.
    /// Used by dir_name, dir_arg, dir_modifier, and attr_name EOF handlers.
    fn emit_eof_in_attr_context(&mut self) {
        self.emit(Event::AttribNameEnd {
            end: self.index as u32,
        });
        self.emit(Event::AttribEnd {
            quote: QuoteType::NoValue,
            end: self.index as u32,
        });
        self.emit(Event::Error {
            code: ErrorCode::EOF_IN_TAG,
            index: self.index as u32,
        });
        self.v_pre_found_by_prepass = false;
        self.state = State::Text;
        self.section_start = self.index;
    }

    /// Consume an interpolation at the current position.
    /// Assumes the caller has already verified the open delimiter matches.
    /// Flushes preceding text, emits Interpolation event, and advances index.
    /// Returns `true` if consumed successfully, `false` on EOF (error emitted).
    fn consume_interpolation(&mut self) -> bool {
        if self.index > self.section_start {
            self.flush_text(self.index, false);
        }
        self.section_start = self.index;
        self.index += self.delimiter_open.len();
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
                true
            }
            None => {
                self.emit(Event::Error {
                    code: ErrorCode::X_MISSING_INTERPOLATION_END,
                    index: self.section_start as u32,
                });
                self.index = self.input.len();
                self.section_start = self.index;
                false
            }
        }
    }

    fn flush_text(&mut self, end: usize, ignore_if_all_whitespace: bool) {
        if self.section_start < end {
            let all_ws = ignore_if_all_whitespace
                && self.input[self.section_start..end]
                    .iter()
                    .all(|&b| is_whitespace(b));
            if !all_ws {
                self.emit(Event::Text {
                    start: self.section_start as u32,
                    end: end as u32,
                });
            }
        }
        self.section_start = end;
    }

    /// Advance index to just past the next `>`, or to EOF if none found.
    fn scan_to_gt(&mut self) {
        if let Some(pos) = memchr(GT, &self.input[self.index..]) {
            self.index += pos + 1;
        } else {
            self.index = self.input.len();
        }
    }

    /// Try to consume a self-closing `/>` sequence starting at the current SLASH position.
    /// Assumes the caller already verified `self.input[self.index] == SLASH` and advanced past it.
    /// Returns `true` if `/>` was consumed (tag closed), `false` if only `/` was found.
    fn try_self_closing(&mut self) -> bool {
        if self.index < self.input.len() && self.input[self.index] == GT {
            self.index += 1;
            self.emit_self_closing_tag(self.index as u32);
            self.in_rcdata = false;
            self.rcdata_allows_interpolation = false;
            self.state = State::Text;
            self.section_start = self.index;
            true
        } else {
            false
        }
    }

    /// Transition to Text or InRCDATA after closing a tag, handling the common pattern.
    fn transition_after_tag_close(&mut self) {
        if self.in_rcdata {
            self.state = State::InRCDATA;
            self.section_start = self.index;
            self.state_in_rcdata();
        } else {
            self.state = State::Text;
            self.section_start = self.index;
        }
    }

    fn run(&mut self) {
        let len = self.input.len();

        while self.index < len {
            match self.state {
                State::Text => self.state_text(),
                State::InRCDATA => self.state_in_rcdata(),
                other => {
                    // All states except Text/InRCDATA are reached via direct calls and
                    // should never appear here. The debug_assert catches missed transitions
                    // during development; in release the +1 ensures forward progress to
                    // avoid an infinite loop should a state somehow leak through.
                    debug_assert!(
                        other == State::Text || other == State::InRCDATA,
                        "Unexpected state {other:?} in run loop at index {}",
                        self.index
                    );
                    self.index += 1;
                }
            }
        }

        // Final text flush for any buffered content
        if self.section_start < self.index {
            self.flush_text(self.index, true);
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
                    // Entity boundary detection: scan for `;` to identify `&...;` patterns.
                    // This only detects boundaries and emits TextEntity events — it does NOT
                    // decode entities (e.g. `&amp;` → `&`). Decoding is left to downstream
                    // consumers. Max scan length of 40 covers all named HTML entities.
                    let entity_start = p;
                    let mut j = p + 1;
                    let mut found_entity = false;
                    while j < self.input.len() && j - entity_start <= 40 {
                        let b = self.input[j];
                        if b == SEMICOLON {
                            // Valid entity boundary found
                            if p > self.section_start {
                                self.flush_text(p, true);
                            }
                            j += 1; // include the semicolon
                            self.emit(Event::TextEntity {
                                start: entity_start as u32,
                                end: j as u32,
                            });
                            self.section_start = j;
                            self.index = j;
                            found_entity = true;
                            break;
                        }
                        if is_whitespace(b) || b == LT || b == AMP {
                            break; // not a valid entity
                        }
                        j += 1;
                    }
                    if !found_entity {
                        self.index = p + 1;
                    }
                } else if c == self.delim_open_first {
                    self.state = State::InterpolationOpen;
                    self.index = p;
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

    /// Called from `state_text` only — RCDATA interpolation uses `consume_interpolation` directly.
    fn state_interpolation_open(&mut self) {
        if next_bytes_equal(self.delimiter_open, &self.input[self.index..]) {
            self.consume_interpolation();
        } else {
            self.index += 1;
        }
        self.state = State::Text;
    }

    /// Flush remaining RCDATA text and emit EOF error.
    fn emit_eof_in_rcdata(&mut self) {
        if self.section_start < self.index {
            self.flush_text(self.index, !self.rcdata_allows_interpolation);
        }
        self.emit(Event::Error {
            code: ErrorCode::EOF_IN_SCRIPT_HTML_COMMENT_LIKE_TEXT,
            index: self.index as u32,
        });
    }

    fn state_in_rcdata(&mut self) {
        while self.index < self.input.len() {
            if self.sequence_index == self.current_sequence.len() {
                let c = self.input[self.index];
                if c == GT || is_whitespace(c) {
                    let end_of_text = self.index - self.current_sequence.len();
                    if self.section_start < end_of_text {
                        // Preserve whitespace in textarea (it's significant content)
                        self.flush_text(end_of_text, !self.rcdata_allows_interpolation);
                    }
                    self.section_start = end_of_text;
                    self.state = State::InClosingTagName;
                    self.in_rcdata = false;
                    self.rcdata_allows_interpolation = false;
                    self.state_in_closing_tag_name();
                    return;
                }
                self.sequence_index = 0;
            }

            let c = self.input[self.index];

            // Check for interpolation in textarea content
            if self.rcdata_allows_interpolation
                && !self.in_v_pre
                && self.sequence_index == 0
                && c == self.delim_open_first
                && next_bytes_equal(self.delimiter_open, &self.input[self.index..])
            {
                if self.consume_interpolation() {
                    continue;
                } else {
                    return;
                }
            }

            if (c | 0x20) == self.current_sequence[self.sequence_index] {
                self.sequence_index += 1;
            } else if self.sequence_index == 0 {
                // Fast-forward: skip bytes until we find something interesting.
                // For textarea (rcdata_allows_interpolation), also look for delimiters.
                let found = if self.rcdata_allows_interpolation {
                    memchr2(LT, self.delim_open_first, &self.input[self.index..])
                } else {
                    memchr(LT, &self.input[self.index..])
                };
                if let Some(pos) = found {
                    self.index += pos;
                    if self.input[self.index] == LT {
                        self.sequence_index = 1;
                    } else if pos > 0 {
                        // Found delimiter open byte ahead — re-check from this
                        // position so the interpolation check can fire.
                        continue;
                    }
                    // pos == 0 means current byte is delimiter but interpolation
                    // check already failed above — just advance past it.
                } else {
                    self.index = self.input.len();
                    self.emit_eof_in_rcdata();
                    return;
                }
            } else {
                self.sequence_index = if c == LT { 1 } else { 0 };
            }
            self.index += 1;
        }

        self.emit_eof_in_rcdata();
    }

    fn state_before_tag_name(&mut self) {
        if self.index >= self.input.len() {
            return;
        }
        let c = self.input[self.index];
        if c == EXCLAMATION_MARK {
            self.state = State::BeforeDeclaration;
            self.index += 1;
            self.state_before_declaration();
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
            self.state = State::InTagName;
            self.state_in_tag_name()
        } else {
            self.state = State::Text;
            self.state_text();
        }
    }

    fn state_in_processing_instruction(&mut self) {
        if let Some(pos) = memchr(GT, &self.input[self.index..]) {
            self.index += pos + 1;
            self.emit(Event::ProcessingInstruction {
                start: self.section_start as u32,
                end: self.index as u32,
            });
            self.state = State::Text;
            self.section_start = self.index;
        } else {
            // EOF: emit PI with what we have
            self.index = self.input.len();
            self.emit(Event::ProcessingInstruction {
                start: self.section_start as u32,
                end: self.index as u32,
            });
            self.emit(Event::Error {
                code: ErrorCode::EOF_IN_TAG,
                index: self.index as u32,
            });
            self.state = State::Text;
            self.section_start = self.index;
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
        // EOF: missing closing tag name
        self.emit(Event::Error {
            code: ErrorCode::MISSING_END_TAG_NAME,
            index: self.index as u32,
        });
        self.state = State::Text;
        self.section_start = self.index;
    }

    fn state_in_closing_tag_name(&mut self) {
        let tag_start = self.section_start;
        let remaining = &self.input[self.index..];

        // Fast path: find > using SIMD, then check for whitespace before it.
        match memchr(GT, remaining) {
            Some(gt_pos) => {
                // Check if whitespace appears in the tag name before >
                let ws_pos = remaining[..gt_pos].iter().position(|&b| is_whitespace(b));

                let name_end = match ws_pos {
                    Some(ws) => {
                        self.state = State::AfterClosingTagName;
                        (self.index + ws) as u32
                    }
                    None => (self.index + gt_pos) as u32,
                };

                let abs_gt = self.index + gt_pos;
                self.emit_close_tag(tag_start as u32, abs_gt as u32 + 1, name_end);
                self.index = abs_gt + 1;
                self.state = State::Text;
                self.section_start = self.index;
            }
            None => {
                // No > found — check for whitespace (EOF edge case)
                let ws_pos = remaining.iter().position(|&b| is_whitespace(b));
                self.index = self.input.len();

                let name_end = match ws_pos {
                    Some(ws) => {
                        self.state = State::AfterClosingTagName;
                        (self.index - remaining.len() + ws) as u32
                    }
                    None => self.index as u32,
                };

                self.emit_close_tag(tag_start as u32, self.index as u32, name_end);
                self.emit(Event::Error {
                    code: ErrorCode::EOF_IN_TAG,
                    index: self.index as u32,
                });
                self.state = State::Text;
                self.section_start = self.index;
            }
        }
    }

    fn state_before_declaration(&mut self) {
        if self.index >= self.input.len() {
            return;
        }
        let c = self.input[self.index];

        match c {
            LEFT_SQUARE => {
                // CDATA: scan for ]]> or fall back to scanning to >
                self.index += 1;
                if next_bytes_equal(b"CDATA[", &self.input[self.index..]) {
                    self.index += 6; // skip "CDATA["
                    match find_subslice(CDATA_END, &self.input[self.index..]) {
                        Some(p) => {
                            self.index += p + CDATA_END.len();
                        }
                        None => {
                            self.index = self.input.len();
                        }
                    }
                } else {
                    // Not valid CDATA, scan to >
                    self.scan_to_gt();
                }
                self.state = State::Text;
                self.section_start = self.index;
            }
            DASH => {
                self.state = State::BeforeComment;

                if next_bytes_equal(b"->", &self.input[self.index + 1..]) {
                    // Short comment: <!-->
                    let comment_start = self.section_start;
                    let content_pos = (self.index + 1) as u32; // after "<!-", before "->"
                    self.index += 3;
                    self.emit(Event::Comment {
                        start: comment_start as u32,
                        end: self.index as u32,
                        content_start: content_pos,
                        content_end: content_pos,
                    });
                    self.state = State::Text;
                    self.section_start = self.index;
                } else if self.index + 1 < self.input.len() && self.input[self.index + 1] == DASH {
                    self.state = State::InCommentLike;
                    self.index += 2;
                    self.section_start = self.index - 4; // include "<!--"
                    let content_start = self.index; // after "<!--"

                    // Check for abrupt-close comment: <!--->
                    if next_bytes_equal(b"->", &self.input[self.index..]) {
                        self.index += 2;
                        self.emit(Event::Comment {
                            start: self.section_start as u32,
                            end: self.index as u32,
                            content_start: content_start as u32,
                            content_end: content_start as u32,
                        });
                        self.section_start = self.index;
                        self.state = State::Text;
                    } else {
                        match find_subslice(COMMENT_END, &self.input[self.index..]) {
                            Some(p) => {
                                let content_end = self.index + p; // before "-->"
                                self.index = content_end + COMMENT_END.len();
                                self.emit(Event::Comment {
                                    start: self.section_start as u32,
                                    end: self.index as u32,
                                    content_start: content_start as u32,
                                    content_end: content_end as u32,
                                });
                                self.section_start = self.index;
                                self.state = State::Text;
                            }
                            None => {
                                // EOF: emit comment with content up to end
                                let content_end = self.input.len();
                                self.index = content_end;
                                self.emit(Event::Comment {
                                    start: self.section_start as u32,
                                    end: self.index as u32,
                                    content_start: content_start as u32,
                                    content_end: content_end as u32,
                                });
                                self.emit(Event::Error {
                                    code: ErrorCode::EOF_IN_COMMENT,
                                    index: self.index as u32,
                                });
                                self.section_start = self.index;
                                self.state = State::Text;
                            }
                        }
                    }
                } else {
                    // Single dash only (e.g., `<!-x>`) — not a valid comment, scan to >
                    self.scan_to_gt();
                    self.state = State::Text;
                    self.section_start = self.index;
                }
            }
            _ => {
                // Declaration (e.g., <!DOCTYPE html>) — scan to closing >
                self.scan_to_gt();
                self.state = State::Text;
                self.section_start = self.index;
            }
        }
    }

    fn check_and_setup_rcdata(&mut self) {
        let tag_name = &self.input[self.section_start + 1..self.index];

        if tag_name.eq_ignore_ascii_case(b"script") {
            self.in_rcdata = true;
            self.rcdata_allows_interpolation = false;
            self.current_sequence = SCRIPT_END;
            self.sequence_index = 0;
        } else if tag_name.eq_ignore_ascii_case(b"style") {
            self.in_rcdata = true;
            self.rcdata_allows_interpolation = false;
            self.current_sequence = STYLE_END;
            self.sequence_index = 0;
        } else if tag_name.eq_ignore_ascii_case(b"textarea") {
            self.in_rcdata = true;
            self.rcdata_allows_interpolation = true;
            self.current_sequence = TEXTAREA_END;
            self.sequence_index = 0;
        }
    }

    fn state_in_tag_name(&mut self) {
        while self.index < self.input.len() {
            let c = self.input[self.index];
            if is_end_of_tag_section(c) {
                self.check_and_setup_rcdata();

                // Fast pre-pass: scan ahead for v-pre in attribute area.
                // Sets in_v_pre to suppress directives, but NOT v_pre_depth
                // (depth is set when v-pre is actually encountered during processing).
                if !self.in_v_pre && c != GT && scan_for_v_pre(self.input, self.index) {
                    self.in_v_pre = true;
                    self.v_pre_found_by_prepass = true;
                }

                self.emit_open_tag_name(self.section_start as u32, self.index as u32);

                if c == GT {
                    self.emit_open_tag_end(self.index as u32);
                    self.index += 1;
                    self.transition_after_tag_close();
                    return;
                } else if c == SLASH {
                    self.index += 1;
                    if !self.try_self_closing() {
                        self.state = State::BeforeAttrName;
                        self.state_before_attr_name();
                    }
                    return;
                } else {
                    self.index += 1;
                    self.state = State::BeforeAttrName;
                    self.state_before_attr_name();
                    return;
                }
            } else {
                self.index += 1;
            }
        }
        // EOF: emit tag name with what we have
        self.check_and_setup_rcdata();
        self.emit_open_tag_name(self.section_start as u32, self.index as u32);
        self.emit(Event::Error {
            code: ErrorCode::EOF_IN_TAG,
            index: self.index as u32,
        });
        self.state = State::Text;
        self.section_start = self.index;
    }

    fn state_before_attr_name(&mut self) {
        while self.index < self.input.len() {
            let c = self.input[self.index];
            if c == GT {
                self.emit_open_tag_end(self.index as u32);
                self.index += 1;
                self.transition_after_tag_close();
                return;
            } else if c == SLASH {
                self.state = State::InSelfClosingTag;
                self.index += 1;
                if !self.try_self_closing() {
                    self.emit(Event::Error {
                        code: ErrorCode::UNEXPECTED_SOLIDUS_IN_TAG,
                        index: self.index as u32,
                    });
                    self.state = State::BeforeAttrName;
                    self.state_before_attr_name();
                }
                return;
            } else if c == LT
                && self.index + 1 < self.input.len()
                && self.input[self.index + 1] == SLASH
            {
                self.emit_open_tag_end(self.index as u32);
                self.state = State::BeforeTagName;
                self.section_start = self.index;
                self.index += 1; // advance past `<` so state_before_tag_name sees `/`
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
                    && self.index + 1 < self.input.len()
                    && self.input[self.index + 1] == DASH
                {
                    if !self.in_v_pre {
                        // Normal case: not in v-pre, process as directive
                        self.section_start = self.index;
                        self.state = State::InDirName;
                        self.state_in_dir_name();
                        return;
                    }
                    if self.v_pre_found_by_prepass {
                        // Pre-pass found v-pre on THIS tag. Check if this
                        // attribute is v-pre itself — let it through to
                        // state_in_dir_name so DirVPre is emitted properly.
                        if is_v_pre_at(self.input, self.index) {
                            self.v_pre_found_by_prepass = false; // consumed
                            self.section_start = self.index;
                            self.state = State::InDirName;
                            self.state_in_dir_name();
                            return;
                        }
                    }
                    // in_v_pre from parent or not v-pre: treat as regular attribute
                    self.section_start = self.index;
                    self.state = State::InAttrName;
                    self.state_in_attr_name();
                    return;
                } else if !self.in_v_pre && (c == DOT || c == COLON || c == AT || c == NUMBER) {
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
        // EOF in tag — no `>` found, emit OpenTagEnd at EOF position directly
        self.emit(Event::OpenTagEnd {
            end: self.index as u32,
        });
        self.emit(Event::Error {
            code: ErrorCode::EOF_IN_TAG,
            index: self.index as u32,
        });
        self.v_pre_found_by_prepass = false;
        self.state = State::Text;
        self.section_start = self.index;
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
                            self.transition_after_tag_close();

                            return;
                        } else if c == SLASH {
                            self.index += 1;
                            if self.try_self_closing() {
                                return;
                            }
                        } else if is_whitespace(c) {
                            self.index += 1;
                        } else {
                            return self.state_before_attr_name();
                        }
                    }
                    // EOF inside v-pre tag
                    self.emit(Event::Error {
                        code: ErrorCode::EOF_IN_TAG,
                        index: self.index as u32,
                    });
                    self.v_pre_found_by_prepass = false;
                    self.state = State::Text;
                    self.section_start = self.index;
                    return;
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
                } else {
                    self.handle_directive_end(c);
                }
                return;
            } else {
                self.index += 1;
            }
        }
        // EOF: emit directive name with what we have
        self.emit(Event::DirName {
            start: self.section_start as u32,
            end: self.index as u32,
        });
        self.emit_eof_in_attr_context();
    }

    fn state_in_dir_arg(&mut self) {
        self.state_in_dir_arg_emit(false);
    }

    fn state_in_dir_arg_emit(&mut self, is_dynamic: bool) {
        while self.index < self.input.len() {
            let c = self.input[self.index];
            if c == LEFT_SQUARE && !is_dynamic {
                self.state = State::InDirDynamicArg;
                self.index += 1;
                self.state_in_dir_dynamic_arg();
                return;
            } else if is_end_of_tag_section(c) || c == EQ || c == DOT {
                self.emit(Event::DirArg {
                    is_dynamic,
                    start: self.section_start as u32,
                    end: self.index as u32,
                });

                if c == DOT {
                    self.index += 1;
                    self.section_start = self.index;
                    self.state = State::InDirModifier;
                    self.state_in_dir_modifier();
                } else {
                    self.handle_directive_end(c);
                }
                return;
            } else {
                self.index += 1;
            }
        }
        // EOF: emit directive arg with what we have
        self.emit(Event::DirArg {
            is_dynamic,
            start: self.section_start as u32,
            end: self.index as u32,
        });
        self.emit_eof_in_attr_context();
    }

    fn state_in_dir_dynamic_arg(&mut self) {
        let mut bracket_count = 1;
        while self.index < self.input.len() {
            let c = self.input[self.index];
            if c == RIGHT_SQUARE {
                bracket_count -= 1;
                if bracket_count == 0 {
                    self.index += 1;
                    self.state_in_dir_arg_emit(true);
                    return;
                }
            } else if c == LEFT_SQUARE {
                bracket_count += 1;
            }
            self.index += 1;
        }
        // EOF: unclosed dynamic argument — emit partial events for consistency
        self.emit(Event::DirArg {
            is_dynamic: true,
            start: self.section_start as u32,
            end: self.index as u32,
        });
        self.emit(Event::AttribNameEnd {
            end: self.index as u32,
        });
        self.emit(Event::AttribEnd {
            quote: QuoteType::NoValue,
            end: self.index as u32,
        });
        self.emit(Event::Error {
            code: ErrorCode::X_MISSING_DYNAMIC_DIRECTIVE_ARGUMENT_END,
            index: self.index as u32,
        });
        self.state = State::Text;
        self.section_start = self.index;
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
                } else {
                    self.handle_directive_end(c);
                    return;
                }
            } else {
                self.index += 1;
            }
        }
        // EOF: emit modifier with what we have
        self.emit(Event::DirModifier {
            start: self.section_start as u32,
            end: self.index as u32,
        });
        self.emit_eof_in_attr_context();
    }

    fn state_in_attr_name(&mut self) {
        let remaining = &self.input[self.index..];

        // Fast path: find common terminators (=, space, >) using SIMD.
        let offset = match memchr3(EQ, SPACE, GT, remaining) {
            Some(pos) => {
                // Check for rare terminators (slash, tab, newline, cr, ff) before pos
                remaining[..pos]
                    .iter()
                    .position(|&b| {
                        b == SLASH
                            || b == TAB
                            || b == NEWLINE
                            || b == FORM_FEED
                            || b == CARRIAGE_RETURN
                    })
                    .unwrap_or(pos)
            }
            None => {
                // Check for rare terminators in all remaining
                match remaining.iter().position(|&b| {
                    b == SLASH || b == TAB || b == NEWLINE || b == FORM_FEED || b == CARRIAGE_RETURN
                }) {
                    Some(pos) => pos,
                    None => {
                        // EOF: emit attribute name with what we have
                        self.index = self.input.len();
                        self.emit(Event::AttribName {
                            start: self.section_start as u32,
                            end: self.index as u32,
                        });
                        self.emit_eof_in_attr_context();
                        return;
                    }
                }
            }
        };

        self.index += offset;
        let c = self.input[self.index];
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
    }

    /// Handle the end of a directive component (name, arg, or modifier) when
    /// the terminating character is GT, SLASH, EQ, or whitespace.
    /// Shared by `state_in_dir_name`, `state_in_dir_arg`, and `state_in_dir_modifier`.
    fn handle_directive_end(&mut self, c: u8) {
        if c == GT {
            self.emit(Event::AttribNameEnd {
                end: self.index as u32,
            });
            self.emit(Event::AttribEnd {
                quote: QuoteType::NoValue,
                end: self.index as u32,
            });
            self.emit_open_tag_end(self.index as u32);
            self.index += 1;
            self.transition_after_tag_close();
        } else if c == SLASH {
            self.emit(Event::AttribNameEnd {
                end: self.index as u32,
            });
            self.emit(Event::AttribEnd {
                quote: QuoteType::NoValue,
                end: self.index as u32,
            });
            self.index += 1;
            if !self.try_self_closing() {
                self.state = State::BeforeAttrName;
                self.state_before_attr_name();
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
            // EOF: emit attrib end
            self.emit(Event::AttribEnd {
                quote: QuoteType::NoValue,
                end: self.section_start as u32,
            });
            self.emit(Event::Error {
                code: ErrorCode::EOF_IN_TAG,
                index: self.index as u32,
            });
            self.state = State::Text;
            self.section_start = self.index;
            return;
        }
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
            self.state_before_attr_name();
        } else {
            self.emit(Event::AttribEnd {
                quote: QuoteType::NoValue,
                end: self.section_start as u32,
            });
            self.state_before_attr_name();
        }
    }

    fn state_before_attr_value(&mut self) {
        // Skip whitespace before attribute value (loop instead of recursion)
        while self.index < self.input.len() && is_whitespace(self.input[self.index]) {
            self.index += 1;
        }
        if self.index >= self.input.len() {
            // EOF: emit error
            self.emit(Event::Error {
                code: ErrorCode::EOF_IN_TAG,
                index: self.index as u32,
            });
            self.state = State::Text;
            self.section_start = self.index;
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
                    self.state_before_attr_name();
                }
                None => {
                    // EOF: emit partial attribute data
                    let end = self.input.len();
                    self.emit(Event::AttribData {
                        start: self.section_start as u32,
                        end: end as u32,
                    });
                    let quote_type = if c == DOUBLE_QUOTE {
                        QuoteType::Double
                    } else {
                        QuoteType::Single
                    };
                    self.emit(Event::AttribEnd {
                        quote: quote_type,
                        end: end as u32,
                    });
                    self.emit(Event::Error {
                        code: ErrorCode::EOF_IN_TAG,
                        index: end as u32,
                    });
                    self.index = end;
                    self.state = State::Text;
                    self.section_start = self.index;
                }
            }
        } else if !is_whitespace(c) {
            self.state = State::InAttrValueNq;
            self.section_start = self.index;

            if c == GT {
                // GT after = ends the tag (empty unquoted value)
                self.emit(Event::AttribData {
                    start: self.section_start as u32,
                    end: self.index as u32,
                });
                self.emit(Event::AttribEnd {
                    quote: QuoteType::Unquoted,
                    end: self.index as u32,
                });
                self.emit_open_tag_end(self.index as u32);
                self.index += 1;
                self.transition_after_tag_close();
                return;
            }

            self.state_in_attr_value_nq();
        }
    }

    fn state_in_attr_value_nq(&mut self) {
        let remaining = &self.input[self.index..];

        // Fast path: find common terminators (space, >, newline) using SIMD.
        let offset = match memchr3(SPACE, GT, NEWLINE, remaining) {
            Some(pos) => {
                // Check for rare terminators (tab, cr, ff) before pos
                remaining[..pos]
                    .iter()
                    .position(|&b| b == TAB || b == FORM_FEED || b == CARRIAGE_RETURN)
                    .unwrap_or(pos)
            }
            None => {
                // Check for rare terminators in all remaining
                match remaining
                    .iter()
                    .position(|&b| b == TAB || b == FORM_FEED || b == CARRIAGE_RETURN)
                {
                    Some(pos) => pos,
                    None => {
                        // EOF: emit attribute data with what we have
                        self.index = self.input.len();
                        self.emit(Event::AttribData {
                            start: self.section_start as u32,
                            end: self.index as u32,
                        });
                        self.emit(Event::AttribEnd {
                            quote: QuoteType::Unquoted,
                            end: self.index as u32,
                        });
                        self.emit(Event::Error {
                            code: ErrorCode::EOF_IN_TAG,
                            index: self.index as u32,
                        });
                        self.state = State::Text;
                        self.section_start = self.index;
                        return;
                    }
                }
            }
        };

        self.index += offset;
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
        self.state_before_attr_name();
    }
}

/// Check whether `v-pre` (case-insensitive) starts at `input[pos]` and is followed
/// by a valid word boundary (whitespace, `>`, `/`, `=`, or EOF).
///
/// Intentionally does NOT treat `:` or `.` as boundaries — `v-pre:arg` and `v-pre.mod`
/// are different directives and should not match.
#[inline]
fn is_v_pre_at(input: &[u8], pos: usize) -> bool {
    pos + 5 <= input.len()
        && (input[pos] | 0x20) == b'v'
        && input[pos + 1] == b'-'
        && (input[pos + 2] | 0x20) == b'p'
        && (input[pos + 3] | 0x20) == b'r'
        && (input[pos + 4] | 0x20) == b'e'
        && (pos + 5 >= input.len() || {
            let next = input[pos + 5];
            is_whitespace(next) || next == GT || next == SLASH || next == EQ
        })
}

/// Fast pre-pass to detect `v-pre` attribute anywhere in a tag's attribute area.
/// Scans forward from `start` (after tag name end) looking for `v-pre` as a standalone
/// attribute, properly skipping over quoted values.
#[inline]
fn scan_for_v_pre(input: &[u8], start: usize) -> bool {
    let mut pos = start;
    while pos < input.len() {
        let c = input[pos];
        if c == GT {
            return false;
        }
        if c == DOUBLE_QUOTE || c == SINGLE_QUOTE {
            // skip quoted value
            pos += 1;
            while pos < input.len() && input[pos] != c {
                pos += 1;
            }
            if pos < input.len() {
                pos += 1;
            }
            continue;
        }
        if is_v_pre_at(input, pos) {
            return true;
        }
        pos += 1;
    }
    false
}

#[inline(always)]
fn next_bytes_equal(needle: &[u8], haystack: &[u8]) -> bool {
    haystack.get(..needle.len()) == Some(needle)
}

#[inline(always)]
fn find_subslice(needle: &[u8], haystack: &[u8]) -> Option<usize> {
    memmem::find(haystack, needle)
}

#[inline(always)]
fn find_unescaped(needle: u8, haystack: &[u8], escape: u8) -> Option<usize> {
    let mut i = 0;
    while i < haystack.len() {
        let pos = memchr(needle, &haystack[i..])?;
        let at = i + pos;

        // Count consecutive escape characters before this position
        let mut esc_count = 0;
        let mut j = at;
        while j > 0 && haystack[j - 1] == escape {
            esc_count += 1;
            j -= 1;
        }
        // Even number of escapes = not escaped
        if esc_count % 2 == 0 {
            return Some(at);
        }
        i = at + 1;
    }
    None
}
