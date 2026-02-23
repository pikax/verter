//! Validates Vue expression identifier extraction against Rust OXC-based extraction.
//!
//! This example reads expressions.json, parses each expression with OXC,
//! extracts identifiers, and compares them with Vue's extraction.

use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_span::SourceType;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::time::Instant;
use verter_core::utils::oxc::{
    extract_bindings_from_expression, extract_slot_bindings, extract_vfor_bindings,
    vue::{parse_vfor, parse_vslot},
    BindingContext,
};

// Input JSON structures (from expressions.json)
#[derive(Debug, Deserialize)]
struct InputFile {
    path: String,
    #[allow(dead_code)]
    lang: String,
    #[allow(dead_code)]
    loc: Loc,
    #[allow(dead_code)]
    attributes: Vec<serde_json::Value>,
    expressions: Vec<InputExpression>,
}

#[derive(Debug, Deserialize, Clone)]
struct Loc {
    #[allow(dead_code)]
    start: u32,
    #[allow(dead_code)]
    end: u32,
}

#[derive(Debug, Deserialize, Clone)]
struct InputExpression {
    id: u32,
    #[serde(rename = "type")]
    expr_type: String,
    #[allow(dead_code)]
    loc: Loc,
    #[allow(dead_code)]
    #[serde(rename = "parentId")]
    parent_id: u32,
    expression: ExpressionContent,
}

#[derive(Debug, Deserialize, Clone)]
struct ExpressionContent {
    content: String,
    #[allow(dead_code)]
    loc: Loc,
    identifiers: Vec<String>,
}

// Output JSON structures
#[derive(Debug, Serialize)]
struct OutputFile {
    path: String,
    expressions: Vec<OutputExpression>,
}

#[derive(Debug, Serialize, Clone)]
struct OutputExpression {
    id: u32,
    content: String,
    #[serde(rename = "vueIdentifiers")]
    vue_identifiers: Vec<String>,
    #[serde(rename = "verterIdentifiers")]
    verter_identifiers: Vec<String>,
}

// Mismatched expression with additional info
#[derive(Debug, Serialize)]
struct MismatchedExpression {
    id: u32,
    path: String,
    content: String,
    #[serde(rename = "type")]
    expr_type: String,
    #[serde(rename = "vueIdentifiers")]
    vue_identifiers: Vec<String>,
    #[serde(rename = "verterIdentifiers")]
    verter_identifiers: Vec<String>,
    #[serde(rename = "missingInVerter")]
    missing_in_verter: Vec<String>,
    #[serde(rename = "extraInVerter")]
    extra_in_verter: Vec<String>,
}

// Slot expression output
#[derive(Debug, Serialize)]
struct SlotExpression {
    id: u32,
    path: String,
    content: String,
    #[serde(rename = "vueIdentifiers")]
    vue_identifiers: Vec<String>,
    /// Local bindings declared by the slot (parameter names)
    locals: Vec<String>,
    /// External references used in the slot (types, default values)
    references: Vec<String>,
}

// Slot error expression
#[derive(Debug, Serialize)]
struct SlotErrorExpression {
    id: u32,
    path: String,
    content: String,
    #[serde(rename = "vueIdentifiers")]
    vue_identifiers: Vec<String>,
    /// Local bindings declared by the slot (parameter names)
    locals: Vec<String>,
    /// External references used in the slot (types, default values)
    references: Vec<String>,
    #[serde(rename = "missingInVerter")]
    missing_in_verter: Vec<String>,
    #[serde(rename = "extraInVerter")]
    extra_in_verter: Vec<String>,
}

// v-for expression output
#[derive(Debug, Serialize)]
struct VForExpression {
    id: u32,
    path: String,
    content: String,
    #[serde(rename = "vueIdentifiers")]
    vue_identifiers: Vec<String>,
    /// Local bindings declared by v-for (iteration variables)
    locals: Vec<String>,
    /// External references used in v-for (the iterable, types)
    references: Vec<String>,
}

// v-for error expression
#[derive(Debug, Serialize)]
struct VForErrorExpression {
    id: u32,
    path: String,
    content: String,
    #[serde(rename = "vueIdentifiers")]
    vue_identifiers: Vec<String>,
    /// Local bindings declared by v-for (iteration variables)
    locals: Vec<String>,
    /// External references used in v-for (the iterable, types)
    references: Vec<String>,
    #[serde(rename = "missingInVerter")]
    missing_in_verter: Vec<String>,
    #[serde(rename = "extraInVerter")]
    extra_in_verter: Vec<String>,
}

// Key mismatch expression (Vue incorrectly includes property keys)
#[derive(Debug, Serialize)]
struct KeyMismatchExpression {
    id: u32,
    path: String,
    content: String,
    #[serde(rename = "type")]
    expr_type: String,
    #[serde(rename = "vueIdentifiers")]
    vue_identifiers: Vec<String>,
    #[serde(rename = "verterIdentifiers")]
    verter_identifiers: Vec<String>,
    /// Property keys that Vue incorrectly included
    #[serde(rename = "incorrectKeys")]
    incorrect_keys: Vec<String>,
}

// Keyword mismatch expression (Vue incorrectly includes JS/TS keywords as identifiers)
#[derive(Debug, Serialize)]
struct KeywordMismatchExpression {
    id: u32,
    path: String,
    content: String,
    #[serde(rename = "type")]
    expr_type: String,
    #[serde(rename = "vueIdentifiers")]
    vue_identifiers: Vec<String>,
    #[serde(rename = "verterIdentifiers")]
    verter_identifiers: Vec<String>,
    /// Keywords that Vue incorrectly included (e.g., "as" from "as const")
    #[serde(rename = "incorrectKeywords")]
    incorrect_keywords: Vec<String>,
}

// Function parameter mismatch expression (Vue incorrectly includes arrow function params as identifiers)
#[derive(Debug, Serialize)]
struct FunctionParamMismatchExpression {
    id: u32,
    path: String,
    content: String,
    #[serde(rename = "type")]
    expr_type: String,
    #[serde(rename = "vueIdentifiers")]
    vue_identifiers: Vec<String>,
    #[serde(rename = "verterIdentifiers")]
    verter_identifiers: Vec<String>,
    /// Function parameters that Vue incorrectly included as identifiers
    #[serde(rename = "incorrectParams")]
    incorrect_params: Vec<String>,
}

// Error expression
#[derive(Debug, Serialize)]
struct ErrorExpression {
    id: u32,
    path: String,
    content: String,
    #[serde(rename = "type")]
    expr_type: String,
    #[serde(rename = "vueIdentifiers")]
    vue_identifiers: Vec<String>,
}

/// Result of extracting identifiers - distinguishes parse errors from empty results
enum ExtractionResult {
    /// Regular expression: extracted identifiers
    Ok(Vec<String>),
    /// Slot expression: locals (declared) and references (external)
    Slot {
        locals: Vec<String>,
        references: Vec<String>,
    },
    /// v-for expression: locals (iteration vars) and references (iterable, types)
    VFor {
        locals: Vec<String>,
        references: Vec<String>,
    },
    /// Parse error
    ParseError,
    /// Empty content - should be skipped
    Empty,
}

/// Check if an identifier looks like it could be a property key in the expression.
/// Returns true if the identifier is followed by a colon (not part of ternary).
fn is_likely_property_key(content: &str, identifier: &str) -> bool {
    // Look for patterns like `{ identifier:` or `, identifier:`
    let patterns = [
        format!("{{ {}: ", identifier), // { key: ...
        format!("{{ {}:", identifier),  // {key:...
        format!(",{}: ", identifier),   // , key: ...
        format!(", {}: ", identifier),  // , key: ...
        format!(",{}:", identifier),    // ,key:...
        format!(", {}:", identifier),   // , key:...
        format!("{{{}:", identifier),   // {key:...
    ];

    for pattern in &patterns {
        if content.contains(pattern.as_str()) {
            return true;
        }
    }

    // Also check for identifier followed by colon (but not ::)
    // This is a bit loose but catches most cases
    let search = format!("{}: ", identifier);
    if content.contains(&search) {
        // Make sure it's not part of a ternary or type annotation
        // Simple heuristic: if there's a '{' before it, it's likely a property key
        if let Some(pos) = content.find(&search) {
            let before = &content[..pos];
            if before.contains('{') && !before.contains("=>") {
                return true;
            }
        }
    }

    false
}

/// Check if ALL missing identifiers are likely property keys.
fn are_all_property_keys(content: &str, missing: &[String]) -> bool {
    if missing.is_empty() {
        return false;
    }
    missing.iter().all(|id| is_likely_property_key(content, id))
}

/// Known keywords/identifiers that Vue incorrectly includes as identifiers.
/// These are JS/TS reserved words or syntax artifacts that shouldn't be treated as identifiers.
const VUE_INCORRECT_KEYWORDS: &[&str] = &[
    "as",       // from "as const" or type assertions
    "const",    // from "as const"
    "typeof",   // TypeScript typeof operator
    "keyof",    // TypeScript keyof operator
    "readonly", // TypeScript readonly modifier
    "infer",    // TypeScript infer keyword
    "extends",  // TypeScript extends keyword
    "in",       // "for x in y" - but "in" is usually handled differently
    "of",       // "for x of y" - same
];

/// Check if an identifier is a known Vue-incorrect keyword.
fn is_vue_incorrect_keyword(identifier: &str) -> bool {
    VUE_INCORRECT_KEYWORDS.contains(&identifier)
}

/// Check if ALL missing identifiers are Vue-incorrect keywords.
fn are_all_vue_keywords(missing: &[String]) -> bool {
    if missing.is_empty() {
        return false;
    }
    missing.iter().all(|id| is_vue_incorrect_keyword(id))
}

/// Check if a character is a valid identifier character (alphanumeric or underscore)
fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

/// Check if an identifier appears as a standalone word in the text
fn contains_word(text: &str, word: &str) -> bool {
    let mut start = 0;
    while let Some(pos) = text[start..].find(word) {
        let abs_pos = start + pos;
        let before_ok =
            abs_pos == 0 || !is_ident_char(text.chars().nth(abs_pos - 1).unwrap_or(' '));
        let after_ok = abs_pos + word.len() >= text.len()
            || !is_ident_char(text.chars().nth(abs_pos + word.len()).unwrap_or(' '));
        if before_ok && after_ok {
            return true;
        }
        start = abs_pos + 1;
    }
    false
}

/// Check if an identifier is likely a function parameter in the expression.
/// Returns true if the identifier appears before `=>` in arrow function syntax.
fn is_likely_function_param(content: &str, identifier: &str) -> bool {
    // Find all arrow function positions
    let mut search_start = 0;
    while let Some(arrow_pos) = content[search_start..].find("=>") {
        let arrow_pos = search_start + arrow_pos;
        let before_arrow = &content[..arrow_pos];

        // Check for simple arrow function: `identifier =>`
        let trimmed = before_arrow.trim_end();
        if trimmed.ends_with(identifier) {
            // Make sure it's a standalone identifier, not part of a larger word
            let before_ident = &trimmed[..trimmed.len() - identifier.len()];
            if before_ident.is_empty() || !is_ident_char(before_ident.chars().last().unwrap_or(' '))
            {
                return true;
            }
        }

        // Check for parenthesized params: `(identifier)` or `(a, identifier)` or `(identifier, b)`
        if let Some(paren_start) = before_arrow.rfind('(') {
            let params_section = &before_arrow[paren_start + 1..];
            // Split by comma and check each param
            for param in params_section.split(',') {
                let param = param.trim();
                // Handle destructuring: { a, b } or [a, b]
                if param.starts_with('{') || param.starts_with('[') {
                    // Check if identifier is inside destructuring as a standalone word
                    if contains_word(param, identifier) {
                        return true;
                    }
                } else if param == identifier {
                    return true;
                } else if param.starts_with(identifier) {
                    // Could be `identifier: Type` or `identifier = default`
                    let rest = &param[identifier.len()..];
                    if rest.starts_with(':') || rest.starts_with('=') || rest.starts_with(' ') {
                        return true;
                    }
                }
            }
        }

        search_start = arrow_pos + 2;
    }

    false
}

/// Check if ALL missing identifiers are likely function parameters.
fn are_all_function_params(content: &str, missing: &[String]) -> bool {
    if missing.is_empty() {
        return false;
    }
    // Must contain arrow function syntax
    if !content.contains("=>") {
        return false;
    }
    missing
        .iter()
        .all(|id| is_likely_function_param(content, id))
}

fn extract_identifiers_from_expression(content: &str, expr_type: &str) -> ExtractionResult {
    // Skip empty content
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return ExtractionResult::Empty;
    }

    let allocator = Allocator::default();
    let source_type = SourceType::tsx();

    // For slot expressions, use specialized extraction that returns locals and references
    if expr_type == "slot" {
        let parse_result = parse_vslot(&allocator, content, source_type);
        let result = extract_slot_bindings(&parse_result);
        if result.has_errors {
            return ExtractionResult::ParseError;
        }
        return ExtractionResult::Slot {
            locals: result.locals,
            references: result.references,
        };
    }

    // For v-for expressions, use specialized extraction that handles "of" syntax
    if expr_type == "for" {
        let parse_result = parse_vfor(&allocator, content, source_type);
        let result = extract_vfor_bindings(&parse_result);
        if result.has_errors {
            return ExtractionResult::ParseError;
        }
        return ExtractionResult::VFor {
            locals: result.locals,
            references: result.references,
        };
    }

    // Try parsing as expression
    let parser = Parser::new(&allocator, content, source_type);
    match parser.parse_expression() {
        Ok(expr) => {
            let ctx = BindingContext::new(0);
            let result = extract_bindings_from_expression(&expr, content, ctx);
            // Deduplicate and sort identifiers
            let mut seen = HashSet::new();
            let mut identifiers: Vec<String> = result
                .non_ignored_binding_names()
                .into_iter()
                .filter(|name| seen.insert(*name))
                .map(String::from)
                .collect();
            identifiers.sort();
            ExtractionResult::Ok(identifiers)
        }
        Err(_) => ExtractionResult::ParseError,
    }
}

fn main() {
    let start_time = Instant::now();

    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let input_path = Path::new(manifest_dir)
        .join("examples")
        .join("expressions")
        .join("source")
        .join("expressions.json");

    let output_dir = Path::new(manifest_dir)
        .join("examples")
        .join("expressions")
        .join("generated");

    // Create output directory if it doesn't exist
    fs::create_dir_all(&output_dir).expect("Failed to create generated directory");

    println!("Reading expressions from: {}", input_path.display());

    let input_content = fs::read_to_string(&input_path).expect("Failed to read expressions.json");

    let input_files: Vec<InputFile> =
        serde_json::from_str(&input_content).expect("Failed to parse expressions.json");

    println!("Processing {} files...", input_files.len());

    let mut total_expressions = 0;
    let mut skipped_empty = 0;
    let total_files = input_files.len();
    let mut matching_expressions = 0;
    let mut mismatched_incorrect: Vec<MismatchedExpression> = Vec::new();
    let mut mismatched_correct: Vec<MismatchedExpression> = Vec::new();
    let mut mismatched_keys: Vec<KeyMismatchExpression> = Vec::new();
    let mut mismatched_keywords: Vec<KeywordMismatchExpression> = Vec::new();
    let mut mismatched_functions: Vec<FunctionParamMismatchExpression> = Vec::new();
    let mut parse_errors: Vec<ErrorExpression> = Vec::new();
    let mut slots_correct: Vec<SlotExpression> = Vec::new();
    let mut slots_error: Vec<SlotErrorExpression> = Vec::new();
    let mut vfor_correct: Vec<VForExpression> = Vec::new();
    let mut vfor_error: Vec<VForErrorExpression> = Vec::new();

    let output_files: Vec<OutputFile> = input_files
        .into_iter()
        .map(|file| {
            let file_path = file.path.clone();
            let expressions: Vec<OutputExpression> = file
                .expressions
                .into_iter()
                .filter_map(|expr| {
                    total_expressions += 1;

                    let is_slot = expr.expr_type == "slot";
                    let extraction_result = extract_identifiers_from_expression(
                        &expr.expression.content,
                        &expr.expr_type,
                    );

                    // Handle extraction results
                    match extraction_result {
                        ExtractionResult::Empty => {
                            skipped_empty += 1;
                            return None;
                        }
                        ExtractionResult::ParseError => {
                            parse_errors.push(ErrorExpression {
                                id: expr.id,
                                path: file_path.clone(),
                                content: expr.expression.content.clone(),
                                expr_type: expr.expr_type.clone(),
                                vue_identifiers: expr.expression.identifiers.clone(),
                            });
                            return None;
                        }
                        ExtractionResult::Slot { locals, references } => {
                            // For slots, compare Vue identifiers with our locals
                            let mut vue_identifiers_sorted = expr.expression.identifiers.clone();
                            vue_identifiers_sorted.sort();

                            let vue_set: HashSet<_> = expr.expression.identifiers.iter().collect();
                            let locals_set: HashSet<_> = locals.iter().collect();

                            if vue_set == locals_set {
                                matching_expressions += 1;
                                slots_correct.push(SlotExpression {
                                    id: expr.id,
                                    path: file_path.clone(),
                                    content: expr.expression.content.clone(),
                                    vue_identifiers: vue_identifiers_sorted.clone(),
                                    locals: locals.clone(),
                                    references: references.clone(),
                                });
                            } else {
                                let mut missing_in_verter: Vec<String> = vue_set
                                    .difference(&locals_set)
                                    .map(|s| (*s).clone())
                                    .collect();
                                missing_in_verter.sort();

                                let mut extra_in_verter: Vec<String> = locals_set
                                    .difference(&vue_set)
                                    .map(|s| (*s).clone())
                                    .collect();
                                extra_in_verter.sort();

                                // Check if mismatch is due to Vue keywords
                                if !missing_in_verter.is_empty()
                                    && extra_in_verter.is_empty()
                                    && are_all_vue_keywords(&missing_in_verter)
                                {
                                    // Vue incorrectly included keywords, treat as correct
                                    mismatched_keywords.push(KeywordMismatchExpression {
                                        id: expr.id,
                                        path: file_path.clone(),
                                        content: expr.expression.content.clone(),
                                        expr_type: expr.expr_type.clone(),
                                        vue_identifiers: vue_identifiers_sorted.clone(),
                                        verter_identifiers: locals.clone(),
                                        incorrect_keywords: missing_in_verter,
                                    });
                                } else {
                                    slots_error.push(SlotErrorExpression {
                                        id: expr.id,
                                        path: file_path.clone(),
                                        content: expr.expression.content.clone(),
                                        vue_identifiers: vue_identifiers_sorted.clone(),
                                        locals: locals.clone(),
                                        references: references.clone(),
                                        missing_in_verter,
                                        extra_in_verter,
                                    });
                                }
                            }

                            // For output, combine locals and references
                            let mut verter_identifiers = locals;
                            verter_identifiers.extend(references);
                            verter_identifiers.sort();
                            verter_identifiers.dedup();

                            return Some(OutputExpression {
                                id: expr.id,
                                content: expr.expression.content,
                                vue_identifiers: vue_identifiers_sorted,
                                verter_identifiers,
                            });
                        }
                        ExtractionResult::VFor { locals, references } => {
                            // For v-for, combine locals and references for comparison
                            let mut vue_identifiers_sorted = expr.expression.identifiers.clone();
                            vue_identifiers_sorted.sort();

                            let vue_set: HashSet<_> = expr.expression.identifiers.iter().collect();

                            // Combine locals and references for comparison
                            let mut all_verter: Vec<String> = locals.clone();
                            all_verter.extend(references.clone());
                            all_verter.sort();
                            all_verter.dedup();
                            let verter_set: HashSet<_> = all_verter.iter().collect();

                            if vue_set == verter_set {
                                matching_expressions += 1;
                                vfor_correct.push(VForExpression {
                                    id: expr.id,
                                    path: file_path.clone(),
                                    content: expr.expression.content.clone(),
                                    vue_identifiers: vue_identifiers_sorted.clone(),
                                    locals: locals.clone(),
                                    references: references.clone(),
                                });
                            } else {
                                let mut missing_in_verter: Vec<String> = vue_set
                                    .difference(&verter_set)
                                    .map(|s| (*s).clone())
                                    .collect();
                                missing_in_verter.sort();

                                let mut extra_in_verter: Vec<String> = verter_set
                                    .difference(&vue_set)
                                    .map(|s| (*s).clone())
                                    .collect();
                                extra_in_verter.sort();

                                // Check if mismatch is due to Vue keywords
                                if !missing_in_verter.is_empty()
                                    && extra_in_verter.is_empty()
                                    && are_all_vue_keywords(&missing_in_verter)
                                {
                                    // Vue incorrectly included keywords, treat as correct
                                    mismatched_keywords.push(KeywordMismatchExpression {
                                        id: expr.id,
                                        path: file_path.clone(),
                                        content: expr.expression.content.clone(),
                                        expr_type: expr.expr_type.clone(),
                                        vue_identifiers: vue_identifiers_sorted.clone(),
                                        verter_identifiers: all_verter.clone(),
                                        incorrect_keywords: missing_in_verter,
                                    });
                                } else if !missing_in_verter.is_empty()
                                    && extra_in_verter.is_empty()
                                    && are_all_property_keys(
                                        &expr.expression.content,
                                        &missing_in_verter,
                                    )
                                {
                                    // Vue incorrectly included property keys
                                    mismatched_keys.push(KeyMismatchExpression {
                                        id: expr.id,
                                        path: file_path.clone(),
                                        content: expr.expression.content.clone(),
                                        expr_type: expr.expr_type.clone(),
                                        vue_identifiers: vue_identifiers_sorted.clone(),
                                        verter_identifiers: all_verter.clone(),
                                        incorrect_keys: missing_in_verter,
                                    });
                                } else {
                                    vfor_error.push(VForErrorExpression {
                                        id: expr.id,
                                        path: file_path.clone(),
                                        content: expr.expression.content.clone(),
                                        vue_identifiers: vue_identifiers_sorted.clone(),
                                        locals: locals.clone(),
                                        references: references.clone(),
                                        missing_in_verter,
                                        extra_in_verter,
                                    });
                                }
                            }

                            return Some(OutputExpression {
                                id: expr.id,
                                content: expr.expression.content,
                                vue_identifiers: vue_identifiers_sorted,
                                verter_identifiers: all_verter,
                            });
                        }
                        ExtractionResult::Ok(verter_identifiers) => {
                            // Regular expression handling
                            let mut vue_identifiers_sorted = expr.expression.identifiers.clone();
                            vue_identifiers_sorted.sort();

                            let vue_set: HashSet<_> = expr.expression.identifiers.iter().collect();
                            let verter_set: HashSet<_> = verter_identifiers.iter().collect();

                            if vue_set == verter_set {
                                matching_expressions += 1;
                            } else {
                                let mut missing_in_verter: Vec<String> = vue_set
                                    .difference(&verter_set)
                                    .map(|s| (*s).clone())
                                    .collect();
                                missing_in_verter.sort();

                                let mut extra_in_verter: Vec<String> = verter_set
                                    .difference(&vue_set)
                                    .map(|s| (*s).clone())
                                    .collect();
                                extra_in_verter.sort();

                                let mismatch = MismatchedExpression {
                                    id: expr.id,
                                    path: file_path.clone(),
                                    content: expr.expression.content.clone(),
                                    expr_type: expr.expr_type.clone(),
                                    vue_identifiers: vue_identifiers_sorted.clone(),
                                    verter_identifiers: verter_identifiers.clone(),
                                    missing_in_verter: missing_in_verter.clone(),
                                    extra_in_verter: extra_in_verter.clone(),
                                };

                                // Categorize mismatches
                                if !missing_in_verter.is_empty()
                                    && extra_in_verter.is_empty()
                                    && are_all_vue_keywords(&missing_in_verter)
                                {
                                    // Vue incorrectly included JS/TS keywords (e.g., "as" from "as const")
                                    mismatched_keywords.push(KeywordMismatchExpression {
                                        id: expr.id,
                                        path: file_path.clone(),
                                        content: expr.expression.content.clone(),
                                        expr_type: expr.expr_type.clone(),
                                        vue_identifiers: vue_identifiers_sorted.clone(),
                                        verter_identifiers: verter_identifiers.clone(),
                                        incorrect_keywords: missing_in_verter.clone(),
                                    });
                                } else if !missing_in_verter.is_empty()
                                    && extra_in_verter.is_empty()
                                    && are_all_function_params(
                                        &expr.expression.content,
                                        &missing_in_verter,
                                    )
                                {
                                    // Vue incorrectly included function parameters as external references
                                    mismatched_functions.push(FunctionParamMismatchExpression {
                                        id: expr.id,
                                        path: file_path.clone(),
                                        content: expr.expression.content.clone(),
                                        expr_type: expr.expr_type.clone(),
                                        vue_identifiers: vue_identifiers_sorted.clone(),
                                        verter_identifiers: verter_identifiers.clone(),
                                        incorrect_params: missing_in_verter.clone(),
                                    });
                                } else if !missing_in_verter.is_empty()
                                    && extra_in_verter.is_empty()
                                    && are_all_property_keys(
                                        &expr.expression.content,
                                        &missing_in_verter,
                                    )
                                {
                                    // Vue incorrectly included property keys
                                    mismatched_keys.push(KeyMismatchExpression {
                                        id: expr.id,
                                        path: file_path.clone(),
                                        content: expr.expression.content.clone(),
                                        expr_type: expr.expr_type.clone(),
                                        vue_identifiers: vue_identifiers_sorted.clone(),
                                        verter_identifiers: verter_identifiers.clone(),
                                        incorrect_keys: missing_in_verter.clone(),
                                    });
                                } else if !missing_in_verter.is_empty() {
                                    // Verter is missing identifiers that Vue found
                                    mismatched_incorrect.push(mismatch);
                                } else {
                                    // Verter has extra identifiers (Vue is wrong)
                                    mismatched_correct.push(mismatch);
                                }
                            }

                            Some(OutputExpression {
                                id: expr.id,
                                content: expr.expression.content,
                                vue_identifiers: vue_identifiers_sorted,
                                verter_identifiers,
                            })
                        }
                    }
                })
                .collect();

            OutputFile {
                path: file_path,
                expressions,
            }
        })
        .collect();

    let elapsed = start_time.elapsed();

    // Write verter.json
    let verter_path = output_dir.join("verter.json");
    let verter_content =
        serde_json::to_string_pretty(&output_files).expect("Failed to serialize verter.json");
    fs::write(&verter_path, verter_content).expect("Failed to write verter.json");

    // Write mismatched.json (truly incorrect - excluding v-for, slots, and key mismatches)
    let mismatched_path = output_dir.join("mismatched.json");
    let mismatched_content = serde_json::to_string_pretty(&mismatched_incorrect)
        .expect("Failed to serialize mismatched.json");
    fs::write(&mismatched_path, mismatched_content).expect("Failed to write mismatched.json");

    // Write mismatched.keys.json (Vue incorrectly included property keys)
    let keys_path = output_dir.join("mismatched.keys.json");
    let keys_content = serde_json::to_string_pretty(&mismatched_keys)
        .expect("Failed to serialize mismatched.keys.json");
    fs::write(&keys_path, keys_content).expect("Failed to write mismatched.keys.json");

    // Write mismatched.correct.json (verter is correct, Vue is wrong - other cases)
    let correct_path = output_dir.join("mismatched.correct.json");
    let correct_content = serde_json::to_string_pretty(&mismatched_correct)
        .expect("Failed to serialize mismatched.correct.json");
    fs::write(&correct_path, correct_content).expect("Failed to write mismatched.correct.json");

    // Write mismatched.keyworded.json (Vue incorrectly included JS/TS keywords)
    let keyworded_path = output_dir.join("mismatched.keyworded.json");
    let keyworded_content = serde_json::to_string_pretty(&mismatched_keywords)
        .expect("Failed to serialize mismatched.keyworded.json");
    fs::write(&keyworded_path, keyworded_content)
        .expect("Failed to write mismatched.keyworded.json");

    // Write mismatched.functions.json (Vue incorrectly included function parameters)
    let functions_path = output_dir.join("mismatched.functions.json");
    let functions_content = serde_json::to_string_pretty(&mismatched_functions)
        .expect("Failed to serialize mismatched.functions.json");
    fs::write(&functions_path, functions_content)
        .expect("Failed to write mismatched.functions.json");

    // Write slots.json (correct slot expressions)
    let slots_path = output_dir.join("slots.json");
    let slots_content =
        serde_json::to_string_pretty(&slots_correct).expect("Failed to serialize slots.json");
    fs::write(&slots_path, slots_content).expect("Failed to write slots.json");

    // Write slots.error.json (mismatched slot expressions)
    let slots_error_path = output_dir.join("slots.error.json");
    let slots_error_content =
        serde_json::to_string_pretty(&slots_error).expect("Failed to serialize slots.error.json");
    fs::write(&slots_error_path, slots_error_content).expect("Failed to write slots.error.json");

    // Write vfor.json (correct v-for expressions)
    let vfor_path = output_dir.join("vfor.json");
    let vfor_content =
        serde_json::to_string_pretty(&vfor_correct).expect("Failed to serialize vfor.json");
    fs::write(&vfor_path, vfor_content).expect("Failed to write vfor.json");

    // Write vfor.error.json (mismatched v-for expressions)
    let vfor_error_path = output_dir.join("vfor.error.json");
    let vfor_error_content =
        serde_json::to_string_pretty(&vfor_error).expect("Failed to serialize vfor.error.json");
    fs::write(&vfor_error_path, vfor_error_content).expect("Failed to write vfor.error.json");

    // Write errors.json
    let errors_path = output_dir.join("errors.json");
    let errors_content =
        serde_json::to_string_pretty(&parse_errors).expect("Failed to serialize errors.json");
    fs::write(&errors_path, errors_content).expect("Failed to write errors.json");

    // Calculate statistics (excluding skipped empty)
    let counted_expressions = total_expressions - skipped_empty;

    // Exact match rate: only when Vue and Verter 100% agree
    let match_rate = (matching_expressions as f64 / counted_expressions as f64) * 100.0;

    // Accuracy rate: includes cases where Verter is correct but Vue has known bugs
    // This counts: exact matches + property key bugs + keyword bugs + function param bugs + other Verter correct cases
    let verter_correct_count = matching_expressions
        + mismatched_keys.len()
        + mismatched_keywords.len()
        + mismatched_functions.len()
        + mismatched_correct.len();
    let accuracy_rate = (verter_correct_count as f64 / counted_expressions as f64) * 100.0;

    // Write summary.md
    let summary_path = output_dir.join("summary.md");
    let summary_content = format!(
        r#"# Expression Validator Results

## Summary

| Metric | Value |
|--------|-------|
| Total Files | {} |
| Total Expressions | {} |
| Skipped (Empty) | {} |
| Matching | {} |
| Mismatched (Verter Incorrect) | {} |
| Mismatched (Property Keys) | {} |
| Mismatched (Keywords) | {} |
| Mismatched (Function Params) | {} |
| Mismatched (Verter Correct) | {} |
| Slots (Correct) | {} |
| Slots (Error) | {} |
| v-for (Correct) | {} |
| v-for (Error) | {} |
| Parse Errors | {} |
| Match Rate | {:.2}% |
| Accuracy Rate | {:.2}% |
| Processing Time | {:.2}s |

## Details

- **Match Rate**: Percentage where Vue and Verter produce identical results
- **Accuracy Rate**: Percentage where Verter is correct (includes Vue's known bugs)
- **Matching**: Vue and Verter extracted the same identifiers
- **Mismatched (Verter Incorrect)**: Verter is missing identifiers that Vue found
- **Mismatched (Property Keys)**: Vue incorrectly includes object property keys as identifiers (e.g., `rowData` in `{{ rowData: role }}`)
- **Mismatched (Keywords)**: Vue incorrectly includes JS/TS keywords as identifiers (e.g., `as` from `as const`)
- **Mismatched (Function Params)**: Vue incorrectly includes arrow function parameters as external references (e.g., `os` in `os => foo[os]`)
- **Mismatched (Verter Correct)**: Verter has different identifiers, but is likely correct (other cases)
- **Slots (Correct)**: Slot expressions parsed as function arguments - matching
- **Slots (Error)**: Slot expressions with mismatched identifiers
- **v-for (Correct)**: v-for expressions with locals and references extracted correctly
- **v-for (Error)**: v-for expressions with mismatched identifiers
- **Parse Errors**: Verter couldn't parse the expression (e.g., reserved words like `class`)

## Files

- `verter.json` - Full comparison output for all expressions
- `mismatched.json` - Expressions where Verter is truly incorrect (missing identifiers)
- `mismatched.keys.json` - Expressions where Vue incorrectly included property keys
- `mismatched.keyworded.json` - Expressions where Vue incorrectly included JS/TS keywords
- `mismatched.functions.json` - Expressions where Vue incorrectly included function parameters
- `mismatched.correct.json` - Expressions where Verter is correct but differs from Vue
- `slots.json` - Slot expressions (correct)
- `slots.error.json` - Slot expressions with errors
- `vfor.json` - v-for expressions (correct)
- `vfor.error.json` - v-for expressions with errors
- `errors.json` - Expressions that failed to parse
"#,
        total_files,
        total_expressions,
        skipped_empty,
        matching_expressions,
        mismatched_incorrect.len(),
        mismatched_keys.len(),
        mismatched_keywords.len(),
        mismatched_functions.len(),
        mismatched_correct.len(),
        slots_correct.len(),
        slots_error.len(),
        vfor_correct.len(),
        vfor_error.len(),
        parse_errors.len(),
        match_rate,
        accuracy_rate,
        elapsed.as_secs_f64()
    );
    fs::write(&summary_path, summary_content).expect("Failed to write summary.md");

    println!("\nResults:");
    println!("  Total files: {}", total_files);
    println!("  Total expressions: {}", total_expressions);
    println!("  Skipped (empty): {}", skipped_empty);
    println!("  Matching: {}", matching_expressions);
    println!(
        "  Mismatched (Verter Incorrect): {}",
        mismatched_incorrect.len()
    );
    println!("  Mismatched (Property Keys): {}", mismatched_keys.len());
    println!("  Mismatched (Keywords): {}", mismatched_keywords.len());
    println!(
        "  Mismatched (Function Params): {}",
        mismatched_functions.len()
    );
    println!(
        "  Mismatched (Verter Correct): {}",
        mismatched_correct.len()
    );
    println!("  Slots (Correct): {}", slots_correct.len());
    println!("  Slots (Error): {}", slots_error.len());
    println!("  v-for (Correct): {}", vfor_correct.len());
    println!("  v-for (Error): {}", vfor_error.len());
    println!("  Parse errors: {}", parse_errors.len());
    println!("  Match rate: {:.2}%", match_rate);
    println!("  Accuracy rate: {:.2}%", accuracy_rate);
    println!("  Processing time: {:.2}s", elapsed.as_secs_f64());
    println!("\nOutput written to: {}", output_dir.display());
}
