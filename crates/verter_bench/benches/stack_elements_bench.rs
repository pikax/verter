#![allow(dead_code)]
#![allow(clippy::type_complexity)]
//! Benchmark: Vec<StackElement> vs u32 depth counter for element nesting tracking.
//!
//! Measures the overhead of maintaining a full element stack (push/pop/peek with
//! 16-byte StackElement structs) versus a minimal u32 depth counter.
//!
//! The realistic scenario simulates tokenizer-driven parsing where:
//!   - Open tags push onto the stack / increment the counter
//!   - Close tags pop from the stack / decrement the counter
//!   - Depth checks happen frequently (every open/close + attribute events)
//!   - Name-based validation (close tag matches open tag) uses the stack top
//!
//! This benchmark helps evaluate whether removing stack_elements in favor of
//! a simpler tracking mechanism would yield meaningful performance gains.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use std::hint::black_box;

// ============================================================================
// Simulated StackElement (mirrors Syntax::StackElement exactly)
// ============================================================================

#[derive(Debug, Clone, Copy)]
struct StackElement {
    tag_open_start: u32,
    tag_open_end: u32,
    name_start: u32,
    name_end: u32,
}

// ============================================================================
// Approach A: Full stack (current implementation)
// ============================================================================

struct FullStackParser {
    stack: Vec<StackElement>,
    root_count: u32,
}

impl FullStackParser {
    fn new() -> Self {
        Self {
            stack: Vec::with_capacity(64),
            root_count: 0,
        }
    }

    #[inline]
    fn open_tag(&mut self, start: u32, name_end: u32) {
        let se = StackElement {
            tag_open_start: start,
            tag_open_end: start, // filled later
            name_start: start + 1,
            name_end,
        };
        self.stack.push(se);

        // Simulate SFC root detection (most common check)
        let _is_root = self.stack.len() == 1;
    }

    #[inline]
    fn open_tag_end(&mut self, end: u32) {
        if let Some(last) = self.stack.last_mut() {
            last.tag_open_end = end;
        }

        // Simulate SFC root check
        let _is_root = self.stack.len() == 1;
    }

    #[inline]
    fn close_tag(&mut self, source: &[u8], close_name_start: u32, close_name_end: u32) {
        if let Some(open) = self.stack.last() {
            // Simulate name validation (case-insensitive comparison)
            let open_name = &source[open.name_start as usize..open.name_end as usize];
            let close_name = &source[close_name_start as usize..close_name_end as usize];
            if open_name.eq_ignore_ascii_case(close_name) {
                self.stack.pop();
            }
        }

        // Simulate post-close root check
        if self.stack.is_empty() {
            self.root_count += 1;
        }
    }

    #[inline]
    fn attribute_end(&mut self, source: &[u8]) {
        // Simulate root attribute detection
        let is_root = self.stack.len() == 1;
        if is_root {
            if let Some(last) = self.stack.last() {
                let _name = &source[last.name_start as usize..last.name_end as usize];
            }
        }
    }
}

// ============================================================================
// Approach B: Depth counter only (hypothetical simplification)
// ============================================================================

struct DepthCounterParser {
    depth: u32,
    root_count: u32,
    // For root detection, we'd need to cache the root element info separately
    current_root_name_start: u32,
    current_root_name_end: u32,
}

impl DepthCounterParser {
    fn new() -> Self {
        Self {
            depth: 0,
            root_count: 0,
            current_root_name_start: 0,
            current_root_name_end: 0,
        }
    }

    #[inline]
    fn open_tag(&mut self, start: u32, name_end: u32) {
        self.depth += 1;

        // Simulate SFC root detection
        let is_root = self.depth == 1;
        if is_root {
            self.current_root_name_start = start + 1;
            self.current_root_name_end = name_end;
        }
    }

    #[inline]
    fn open_tag_end(&mut self, _end: u32) {
        // Simulate SFC root check
        let _is_root = self.depth == 1;
    }

    #[inline]
    fn close_tag(&mut self, _source: &[u8], _close_name_start: u32, _close_name_end: u32) {
        // NOTE: Without the stack, we CANNOT validate close-tag name matching!
        // This is a correctness trade-off. The depth counter approach would need
        // to delegate validation to the builder (which has open_stack: Vec<NodeId>).
        if self.depth > 0 {
            self.depth -= 1;
        }

        if self.depth == 0 {
            self.root_count += 1;
        }
    }

    #[inline]
    fn attribute_end(&mut self, source: &[u8]) {
        // Simulate root attribute detection
        let is_root = self.depth == 1;
        if is_root {
            let _name =
                &source[self.current_root_name_start as usize..self.current_root_name_end as usize];
        }
    }
}

// ============================================================================
// Approach C: Full stack with reduced capacity (capacity 16 instead of 64)
// ============================================================================

struct SmallStackParser {
    stack: Vec<StackElement>,
    root_count: u32,
}

impl SmallStackParser {
    fn new() -> Self {
        Self {
            stack: Vec::with_capacity(16),
            root_count: 0,
        }
    }

    #[inline]
    fn open_tag(&mut self, start: u32, name_end: u32) {
        let se = StackElement {
            tag_open_start: start,
            tag_open_end: start,
            name_start: start + 1,
            name_end,
        };
        self.stack.push(se);
        let _is_root = self.stack.len() == 1;
    }

    #[inline]
    fn open_tag_end(&mut self, end: u32) {
        if let Some(last) = self.stack.last_mut() {
            last.tag_open_end = end;
        }
        let _is_root = self.stack.len() == 1;
    }

    #[inline]
    fn close_tag(&mut self, source: &[u8], close_name_start: u32, close_name_end: u32) {
        if let Some(open) = self.stack.last() {
            let open_name = &source[open.name_start as usize..open.name_end as usize];
            let close_name = &source[close_name_start as usize..close_name_end as usize];
            if open_name.eq_ignore_ascii_case(close_name) {
                self.stack.pop();
            }
        }
        if self.stack.is_empty() {
            self.root_count += 1;
        }
    }

    #[inline]
    fn attribute_end(&mut self, source: &[u8]) {
        let is_root = self.stack.len() == 1;
        if is_root {
            if let Some(last) = self.stack.last() {
                let _name = &source[last.name_start as usize..last.name_end as usize];
            }
        }
    }
}

// ============================================================================
// Simulated event sequences
// ============================================================================

/// Generates a sequence of (open_start, name_end, close_name_start, close_name_end)
/// tuples representing a realistic Vue SFC template with nested elements.
///
/// Pattern: <template> + N elements at varying depths (max depth ~8)
fn generate_sfc_events(element_count: usize) -> (Vec<u8>, Vec<(u32, u32, u32, u32, u8)>) {
    // Build a synthetic source buffer with tag names
    let tag_names: Vec<&[u8]> = vec![
        b"template",
        b"div",
        b"span",
        b"p",
        b"ul",
        b"li",
        b"button",
        b"input",
        b"a",
        b"img",
        b"h1",
        b"h2",
        b"section",
        b"header",
        b"footer",
        b"nav",
        b"main",
        b"form",
        b"label",
        b"select",
    ];

    let mut source = Vec::with_capacity(element_count * 20);
    // Events: (tag_start_in_source, name_end_in_source, close_start, close_end, num_attrs)
    let mut events = Vec::with_capacity(element_count * 2);

    let mut pos: u32 = 0;
    let max_depth: usize = 8;
    let mut depth_stack: Vec<usize> = Vec::new(); // index into tag_names

    for i in 0..element_count {
        let tag_idx = i % tag_names.len();
        let tag = tag_names[tag_idx];

        // Decide whether to open or close (maintain reasonable depth)
        let should_close = depth_stack.len() >= max_depth || (depth_stack.len() > 2 && i % 3 == 0);

        if should_close && !depth_stack.is_empty() {
            // Close the most recent element
            let close_tag_idx = depth_stack.pop().unwrap();
            let close_tag = tag_names[close_tag_idx];

            // Write "</tagname>" into source
            let start = pos;
            source.extend_from_slice(b"</");
            source.extend_from_slice(close_tag);
            source.push(b'>');
            let close_name_start = start + 2;
            let close_name_end = close_name_start + close_tag.len() as u32;
            pos = start + 2 + close_tag.len() as u32 + 1;

            // Event type 1 = close
            events.push((start, close_name_end, close_name_start, close_name_end, 0));
        }

        // Open a new element
        // Write "<tagname ...>" into source
        let start = pos;
        source.push(b'<');
        source.extend_from_slice(tag);
        source.push(b'>');
        let name_end = start + 1 + tag.len() as u32;
        pos = name_end + 1;

        let num_attrs = match i % 5 {
            0 => 0,
            1 => 1,
            2 => 2,
            3 => 3,
            _ => 0,
        };

        // Event type 2 = open (num_attrs > 0 or 0)
        events.push((start, name_end, 0, 0, num_attrs));

        depth_stack.push(tag_idx);
    }

    // Close all remaining open elements
    while let Some(close_tag_idx) = depth_stack.pop() {
        let close_tag = tag_names[close_tag_idx];
        let start = pos;
        source.extend_from_slice(b"</");
        source.extend_from_slice(close_tag);
        source.push(b'>');
        let close_name_start = start + 2;
        let close_name_end = close_name_start + close_tag.len() as u32;
        pos = start + 2 + close_tag.len() as u32 + 1;
        events.push((start, close_name_end, close_name_start, close_name_end, 0));
    }

    (source, events)
}

// ============================================================================
// Benchmarks
// ============================================================================

fn bench_stack_overhead(c: &mut Criterion) {
    let mut group = c.benchmark_group("stack_elements_overhead");

    for &count in &[50, 100, 200, 500, 1000] {
        let (source, events) = generate_sfc_events(count);

        group.bench_with_input(
            BenchmarkId::new("full_stack_cap64", count),
            &count,
            |b, _| {
                b.iter(|| {
                    let mut parser = FullStackParser::new();
                    for &(start, name_end, close_start, close_end, num_attrs) in &events {
                        if close_start > 0 {
                            // Close event
                            parser.close_tag(&source, close_start, close_end);
                        } else {
                            // Open event
                            parser.open_tag(start, name_end);
                            parser.open_tag_end(name_end + 1);
                            for _ in 0..num_attrs {
                                parser.attribute_end(&source);
                            }
                        }
                    }
                    black_box(parser.root_count);
                });
            },
        );

        group.bench_with_input(BenchmarkId::new("depth_counter", count), &count, |b, _| {
            b.iter(|| {
                let mut parser = DepthCounterParser::new();
                for &(start, name_end, close_start, close_end, num_attrs) in &events {
                    if close_start > 0 {
                        parser.close_tag(&source, close_start, close_end);
                    } else {
                        parser.open_tag(start, name_end);
                        parser.open_tag_end(name_end + 1);
                        for _ in 0..num_attrs {
                            parser.attribute_end(&source);
                        }
                    }
                }
                black_box(parser.root_count);
            });
        });

        group.bench_with_input(
            BenchmarkId::new("small_stack_cap16", count),
            &count,
            |b, _| {
                b.iter(|| {
                    let mut parser = SmallStackParser::new();
                    for &(start, name_end, close_start, close_end, num_attrs) in &events {
                        if close_start > 0 {
                            parser.close_tag(&source, close_start, close_end);
                        } else {
                            parser.open_tag(start, name_end);
                            parser.open_tag_end(name_end + 1);
                            for _ in 0..num_attrs {
                                parser.attribute_end(&source);
                            }
                        }
                    }
                    black_box(parser.root_count);
                });
            },
        );
    }

    group.finish();

    // Print sizes for reference
    eprintln!("\n=== Stack Element Sizes ===");
    eprintln!(
        "  StackElement:    {} bytes (Copy: {})",
        std::mem::size_of::<StackElement>(),
        std::mem::size_of::<StackElement>() <= 16
    );
    eprintln!(
        "  Vec<SE> cap 64:  {} bytes (stack) + {} bytes (heap)",
        std::mem::size_of::<Vec<StackElement>>(),
        64 * std::mem::size_of::<StackElement>()
    );
    eprintln!(
        "  Vec<SE> cap 16:  {} bytes (stack) + {} bytes (heap)",
        std::mem::size_of::<Vec<StackElement>>(),
        16 * std::mem::size_of::<StackElement>()
    );
    eprintln!("  u32 counter:     {} bytes", std::mem::size_of::<u32>());
    eprintln!("===========================\n");
}

criterion_group!(benches, bench_stack_overhead);
criterion_main!(benches);
