// This is meant to provide most of the information for vue, eg, new lines, etc.

use super::types::QuoteType;
use crate::{
    common::{ErrorCode, SourceLocation},
    cursor::cursor::{find_subslice, Cursor, CursorPosition},
    tokenizer::{
        helpers::{
            is_end_of_tag_section, is_tag_start_char, is_whitespace, DEFAULT_DELIMITER_CLOSE,
            DEFAULT_DELIMITER_OPEN,
        },
        types::{
            char_codes::{
                AMP, AT, COLON, DASH, DOT, EQ, EXCLAMATION_MARK, GT, LOWER_V, LT, NUMBER,
                QUESTION_MARK, SLASH,
            },
            EventSourceLocation,
        },
    },
};

/// Tokenize the input and call the callback for each event.
/// Events contain full position.
pub fn tokenize(input: &str, callback: impl FnMut(EventSourceLocation)) {
    tokenize_with_delimiters(
        input,
        callback,
        DEFAULT_DELIMITER_OPEN,
        DEFAULT_DELIMITER_CLOSE,
    )
}

/// Tokenize the input with custom delimiters and call the callback for each event.
/// Events contain full position.
pub fn tokenize_with_delimiters(
    input: &str,
    callback: impl FnMut(EventSourceLocation),
    delimiter_open: &[u8],
    delimiter_close: &[u8],
) {
    let mut tokenizer = Tokenizer::new(input, callback, delimiter_open, delimiter_close);
    tokenizer.run();
}

struct Tokenizer<'a, F: FnMut(EventSourceLocation)> {
    callback: F,
    delimiter_open: &'a [u8],
    delimiter_close: &'a [u8],

    delimiter_open_first_byte: u8,
    #[allow(dead_code)]
    delimiter_close_first_byte: u8,

    cursor: Cursor<'a>,
    section_start: Option<CursorPosition>,

    in_v_pre: bool,
    v_pre_depth: usize,
}

impl<'a, F: FnMut(EventSourceLocation)> Tokenizer<'a, F> {
    fn new(
        input: &'a str,
        callback: F,
        delimiter_open: &'a [u8],
        delimiter_close: &'a [u8],
    ) -> Self {
        Self {
            callback,

            delimiter_open,
            delimiter_open_first_byte: delimiter_open[0],
            delimiter_close,
            delimiter_close_first_byte: delimiter_close[0],

            cursor: Cursor::new(input),
            section_start: None,

            in_v_pre: false,
            v_pre_depth: 0,
        }
    }

    fn run(&mut self) {
        // Tokenization logic goes here
        while !self.cursor.ended() {
            self.handle_text();

            self.cursor.increment();
        }
    }

    fn emit(&mut self, event: EventSourceLocation) {
        (self.callback)(event);
    }

    fn handle_text(&mut self) {
        let next = if self.in_v_pre {
            self.cursor.search2(self.delimiter_open_first_byte, b'<')
        } else {
            self.cursor
                .search3(self.delimiter_open_first_byte, b'<', b'&')
        };

        match next {
            Some(idx) => {
                // Emit text up to idx
                self.cursor.advance(idx);
                let c = self.cursor.current_byte();
                if c == LT {
                    self.flush_text_section();

                    self.handle_tag_start()
                } else if c == AMP {
                    // TODO: Handle entity
                    self.cursor.increment();
                } else if c == self.delimiter_open_first_byte {
                    // self.cursor.advance(idx);
                    self.handle_interpolation_start()
                } else {
                    debug_assert!(false, "Unexpected byte encountered");
                    self.end();
                }
            }
            None => {
                // Emit remaining text
                self.end()
            }
        }
    }

    // aka state_before_tag_name
    fn handle_tag_start(&mut self) {
        // Placeholder for tag handling logic
        // self.cursor.increment(); // Move past '<'
        self.cursor.increment();

        let c = self.cursor.current_byte();
        if c == EXCLAMATION_MARK {
            self.handle_before_declaration();
        } else if c == QUESTION_MARK {
            self.handle_processing_instruction();
        } else if c == SLASH {
            self.handle_closing_tag();
        } else if is_tag_start_char(c) {
            self.handle_tag_name()
        }
    }

    fn handle_interpolation_start(&mut self) {
        // Placeholder for interpolation handling logic
        if self.cursor.next_bytes_equal(self.delimiter_open) {
            self.flush_text_section();

            let start = self.cursor.position;
            self.cursor.advance(self.delimiter_open.len());

            match find_subslice(self.delimiter_close, self.cursor.remaining()) {
                Some(idx) => {
                    self.cursor.advance(idx + self.delimiter_close.len());

                    self.emit(EventSourceLocation::Interpolation(
                        SourceLocation::from_source(
                            self.cursor.input,
                            start.to_position(),
                            self.cursor.position.to_position(),
                        ),
                    ));

                    self.section_start = Some(self.cursor.position);
                    // Note we could call self.handle_text() here directly to continue processing
                    // that would lead to deep recursion on pathological inputs
                    // so we just return to the main loop to prevent deep recursion.
                }
                None => {
                    // TODO error
                    self.end();
                }
            }
        }
        /*else if self.in_rcdata {
           // TODO
        } */
        else {
            self.cursor.increment();
        }
    }

    fn handle_before_declaration(&mut self) {
        // Placeholder for declaration handling logic
        self.cursor.increment(); // Move past '!'
    }

    fn handle_processing_instruction(&mut self) {
        // Placeholder for processing instruction handling logic
        self.cursor.increment(); // Move past '?'
        self.section_start = Some(self.cursor.position);
    }

    fn handle_closing_tag(&mut self) {
        // Placeholder for closing tag handling logic
        // self.cursor.increment(); // Move past '/'
        // self.section_start = Some(self.cursor.position);

        // let start = self.cursor.position;
    }

    fn handle_tag_name(&mut self) {
        // Placeholder for tag name handling logic
        // self.cursor.increment(); // Move past tag name start
        // self.section_start = Some(self.cursor.position);

        let start = self.cursor.position;

        while !self.cursor.ended() && !is_end_of_tag_section(self.cursor.current_byte()) {
            self.cursor.increment();
        }

        if self.cursor.ended() {
            // TODO error
            self.end();
            return;
        }

        let end = self.cursor.position;

        // TODO add check_and_setup_rcdata
        self.emit(EventSourceLocation::OpenTagName(
            SourceLocation::from_source(self.cursor.input, start.to_position(), end.to_position()),
        ));

        let c = self.cursor.current_byte();
        if c == GT {
            self.emit(EventSourceLocation::OpenTagEnd(end.to_position()));
            // TODO if rcdata handle rcdata
            self.cursor.increment();
            self.section_start = Some(self.cursor.position);
        } else if c == SLASH {
            self.cursor.increment();
            if self.cursor.current_byte() == GT {
                self.emit(EventSourceLocation::SelfClosingTag(
                    self.cursor.position.to_position(),
                ));
                self.cursor.increment();
                // TODO reset rcdata
                // self.in_rcdata = false;
                self.section_start = Some(self.cursor.position);
            } else if self.cursor.ended() {
                // TODO error
                self.end();
            } else {
                // TODO error
                self.handle_before_attribute_name();
            }
        } else {
            self.cursor.increment();
        }
    }

    fn handle_before_attribute_name(&mut self) {
        while !self.cursor.ended() {
            let c = self.cursor.current_byte();
            if c == GT {
                self.emit(EventSourceLocation::OpenTagEnd(
                    self.cursor.position.to_position(),
                ));
                self.cursor.increment();
                // TODO set rcdata
                // if self.in_rcdata {
                //     self.state = State::InRCDATA;
                //     self.section_start = self.index;
                //     self.state_in_rcdata();
                // }
                self.section_start = Some(self.cursor.position);

                return;
            } else if c == SLASH {
                // self.state = State::InSelfClosingTag;
                // self.index += 1;
                // if self.index < self.input.len() && self.input[self.index] == GT {
                //     self.emit(Event::SelfClosingTag {
                //         end: self.index as u32,
                //     });
                //     self.index += 1;
                //     self.in_rcdata = false;
                //     self.state = State::Text;
                //     self.section_start = self.index;
                //     return;
                // } else {
                //     self.emit(Event::Error {
                //         code: ErrorCode::UNEXPECTED_SOLIDUS_IN_TAG,
                //         index: self.index as u32,
                //     });
                //     return;
                // }

                self.cursor.increment();
                if !self.cursor.ended() && self.cursor.current_byte() == GT {
                    self.emit(EventSourceLocation::SelfClosingTag(
                        self.cursor.position.to_position(),
                    ));
                    self.cursor.increment();
                    // TODO reset rcdata
                    // self.in_rcdata = false;
                    self.section_start = Some(self.cursor.position);
                    return;
                } else {
                    // TODO error
                    self.emit(EventSourceLocation::Error {
                        code: ErrorCode::UNEXPECTED_SOLIDUS_IN_TAG,
                        position: self.cursor.position.to_position(),
                    });
                    return;
                }
            } else if c == LT && self.cursor.next_byte() == SLASH {
                self.emit(EventSourceLocation::OpenTagEnd(
                    self.cursor.position.to_position(),
                ));
                self.section_start = Some(self.cursor.position);
                self.handle_tag_start();
                return;
            } else if !is_whitespace(c) {
                if c == EQ {
                    self.emit(EventSourceLocation::Error {
                        code: ErrorCode::UNEXPECTED_EQUALS_SIGN_BEFORE_ATTRIBUTE_NAME,
                        position: self.cursor.position.to_position(),
                    });
                }

                if c == LOWER_V && self.cursor.next_byte() == DASH {
                    self.section_start = Some(self.cursor.position);
                    self.handle_dir_name();
                    return;
                } else if c == DOT || c == COLON || c == AT || c == NUMBER {
                    let start = self.cursor.position;
                    self.cursor.increment();

                    self.emit(EventSourceLocation::DirName(SourceLocation::from_source(
                        self.cursor.input,
                        start.to_position(),
                        self.cursor.position.to_position(),
                    )));
                    self.section_start = Some(self.cursor.position);
                    self.handle_dir_arg();
                    return;
                } else {
                    self.section_start = Some(self.cursor.position);
                    self.handle_attr_name();
                    return;
                }
            } else {
                self.cursor.increment();
            }
        }
    }

    // state_in_dir_name
    fn handle_dir_name(&mut self) {
        // Placeholder for directive name handling logic
        // self.cursor.increment(); // Move past directive name start
        // self.section_start = Some(self.cursor.position);

        while !self.cursor.ended() && is_end_of_tag_section(self.cursor.current_byte()) {
            self.cursor.increment();
        }
        if self.cursor.ended() {
            // TODO error
            self.end();
            return;
        }
        let end = self.cursor.position;
        if let Some(start) = self.section_start {
            if self.cursor.position.byte_index - start.byte_index == 5
                && (self.cursor.bytes[start.byte_index] | 0x20) == b'v'
                && self.cursor.bytes[start.byte_index + 1] == b'-'
                && (self.cursor.bytes[start.byte_index + 2] | 0x20) == b'p'
                && (self.cursor.bytes[start.byte_index + 3] | 0x20) == b'r'
                && (self.cursor.bytes[start.byte_index + 4] | 0x20) == b'e'
            {
                self.in_v_pre = true;
                self.v_pre_depth = 0; // Will be incremented to 1 when OpenTagEnd is emitted
                self.emit(EventSourceLocation::DirVPre(SourceLocation::from_source(
                    self.cursor.input,
                    start.to_position(),
                    end.to_position(),
                )));
            }

            self.emit(EventSourceLocation::DirName(SourceLocation::from_source(
                self.cursor.input,
                start.to_position(),
                end.to_position(),
            )));
        }

        let c = self.cursor.current_byte();
        if c == COLON {
            self.handle_dir_arg();
        } else if c == DOT {
            self.handle_dir_modifier();
        } else if c == GT {
            self.emit(EventSourceLocation::OpenTagEnd(
                self.cursor.position.to_position(),
            ));
            self.cursor.increment();
            self.section_start = Some(self.cursor.position);
        } else if c == SLASH {
            self.emit(EventSourceLocation::OpenTagEnd(
                self.cursor.position.to_position(),
            ));
            self.emit(EventSourceLocation::AttribEnd {
                quote: QuoteType::NoValue,
                position: self.cursor.position.to_position(),
            });
            self.cursor.increment();
            if !self.cursor.ended() && self.cursor.current_byte() == GT {
                self.emit(EventSourceLocation::SelfClosingTag(
                    self.cursor.position.to_position(),
                ));
                self.cursor.increment();
                // TODO reset rcdata
                // self.in_rcdata = false;
                self.section_start = Some(self.cursor.position);
            } else if self.cursor.ended() {
                // TODO error
                self.end();
            } else {
                // TODO error
                self.handle_before_attribute_name();
            }
        } else if c == EQ {
            self.emit(EventSourceLocation::AttribNameEnd(
                self.cursor.position.to_position(),
            ));
            self.handle_before_attribute_value()
        } else {
            self.handle_before_attribute_name();
        }
    }

    // state_in_dir_arg
    fn handle_dir_arg(&mut self) {
        // skip past ':'
        self.cursor.increment();
    }

    // state_in_dir_modifier
    fn handle_dir_modifier(&mut self) {
        // skip past '.'
        self.cursor.increment();
    }

    // state_in_attr_name
    fn handle_attr_name(&mut self) {
        // TODO
        self.cursor.increment();
    }
    // state_in_before_attr_name
    fn handle_before_attribute_value(&mut self) {
        // skip past '='
        self.cursor.increment();
    }

    fn end(&mut self) {
        if !self.cursor.ended() {
            let start = self.cursor.position;
            let end = self.cursor.to_end();
            self.emit(EventSourceLocation::Text(SourceLocation::from_source(
                self.cursor.input,
                start.to_position(),
                end.to_position(),
            )));
        }

        self.emit(EventSourceLocation::End);
    }

    fn flush_text_section(&mut self) {
        if let Some(start) = self.section_start {
            if self.cursor.position.byte_index > start.byte_index {
                let end = self.cursor.position;
                self.emit(EventSourceLocation::Text(SourceLocation::from_source(
                    self.cursor.input,
                    start.to_position(),
                    end.to_position(),
                )));
            }
            self.section_start = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokenizer::string::tokenize;

    #[test]
    fn test_simple_text() {
        let mut events = Vec::new();
        tokenize("hello world", |e: EventSourceLocation| {
            events.push(format!("{:?}", e));
        });

        assert!(!events.is_empty());
        println!("Simple text events: {:?}", events);
    }

    #[test]
    fn test_simple_html_tag() {
        let mut events = Vec::new();
        tokenize("<div>content</div>", |e: EventSourceLocation| {
            events.push(format!("{:?}", e));
        });

        assert!(!events.is_empty());
        println!("HTML tag events: {:#?}", events);
    }

    #[test]
    fn test_vue_interpolation() {
        let mut events = Vec::new();
        tokenize("{{ message }}", |e: EventSourceLocation| {
            events.push(format!("{:?}", e));
        });

        assert!(!events.is_empty());
        println!("Vue interpolation events: {:#?}", events);
    }

    #[test]
    fn test_unicode() {
        let mut events = Vec::new();
        tokenize("<div>张张张张张张</div>", |e: EventSourceLocation| {
            events.push(format!("{:?}", e));
        });

        assert!(!events.is_empty());
        println!("Vue directive events: {:#?}", events);
    }
}
