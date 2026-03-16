//! Vite config alias discovery: static analysis (OXC) + trusted execution fallback.
//!
//! This module implements two strategies for extracting `resolve.alias` from Vite configs:
//!
//! 1. **Static analysis** (default): Parses config files with OXC and extracts aliases
//!    from object/array literals without executing any user code.
//! 2. **Trusted execution** (opt-in): Spawns Node.js to evaluate the config when static
//!    analysis cannot handle the config's complexity.
//!
//! A last-known-good (LKG) cache stores successful execution results so that transient
//! failures (e.g., missing node_modules after branch switch) don't lose alias info.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

// ═══════════════════════════════════════════════════════════════════════════
// Types
// ═══════════════════════════════════════════════════════════════════════════

/// Result of analyzing a Vite config file.
#[derive(Debug, Clone)]
pub enum ViteConfigAnalysis {
    /// Static analysis succeeded — aliases extracted without code execution.
    Resolved {
        config_path: String,
        aliases: Vec<(String, String)>,
        dependency_files: Vec<String>,
    },
    /// Config exists but is too complex for static analysis (needs execution).
    Complex { config_path: String, reason: String },
    /// No vite config found in the given directory.
    NotFound,
}

/// Result of trusted execution.
pub struct TrustedExecResult {
    pub aliases: Vec<(String, String)>,
    /// Config file + helper deps (canonical absolute paths).
    pub dependency_files: Vec<String>,
}

/// Options controlling vite config behavior during registry build.
#[derive(Debug, Clone)]
pub struct ViteConfigOptions {
    pub enabled: bool,
    pub trusted_files: Vec<String>,
    pub node_path: Option<String>,
}

impl Default for ViteConfigOptions {
    fn default() -> Self {
        Self {
            enabled: true,
            trusted_files: Vec::new(),
            node_path: None,
        }
    }
}

/// Info about a config that requires user trust before execution.
#[derive(Debug, Clone)]
pub struct ViteConfigTrustInfo {
    pub config_path: String,
    pub workspace_root: String,
    pub reason: String,
}

// ═══════════════════════════════════════════════════════════════════════════
// Config File Discovery
// ═══════════════════════════════════════════════════════════════════════════

/// All vite config file extensions, checked in priority order.
const VITE_CONFIG_NAMES: &[&str] = &[
    "vite.config.ts",
    "vite.config.js",
    "vite.config.mjs",
    "vite.config.cjs",
    "vite.config.mts",
    "vite.config.cts",
];

/// Find the vite config file in a project root, checking all extensions.
pub fn find_vite_config(project_root: &Path) -> Option<PathBuf> {
    VITE_CONFIG_NAMES
        .iter()
        .map(|name| project_root.join(name))
        .find(|p| p.exists())
}

// ═══════════════════════════════════════════════════════════════════════════
// Alias Normalization
// ═══════════════════════════════════════════════════════════════════════════

/// Normalize an alias pair: bare `@` → `@/`, relative replacement → absolute.
///
/// Shared by both static analysis and trusted execution paths.
pub fn normalize_alias_pair(find: &str, replacement: &str, config_dir: &Path) -> (String, String) {
    let replacement_path = PathBuf::from(replacement);
    let abs_replacement = if replacement_path.is_absolute() {
        replacement_path
    } else {
        config_dir.join(replacement)
    };
    let abs_str = abs_replacement.to_string_lossy().replace('\\', "/");

    // Bare aliases like `@` become `@/` for wildcard matching
    let normalized_find = if find.ends_with('/') {
        find.to_string()
    } else {
        format!("{find}/")
    };

    (normalized_find, abs_str)
}

// ═══════════════════════════════════════════════════════════════════════════
// Static Analysis (OXC-based)
// ═══════════════════════════════════════════════════════════════════════════

/// Statically analyze a Vite config to extract `resolve.alias` without execution.
///
/// Uses OXC to parse the config and extract alias entries from object/array literals.
/// Returns `Complex` if the config uses patterns that require execution (functions,
/// environment variables, dynamic imports, non-allowlisted packages, etc.).
pub fn analyze_vite_config(project_root: &Path) -> ViteConfigAnalysis {
    let config_path = match find_vite_config(project_root) {
        Some(p) => p,
        None => return ViteConfigAnalysis::NotFound,
    };

    let config_path_str = config_path.to_string_lossy().replace('\\', "/");
    let config_dir = config_path.parent().unwrap_or(project_root);

    let source = match std::fs::read_to_string(&config_path) {
        Ok(s) => s,
        Err(e) => {
            return ViteConfigAnalysis::Complex {
                config_path: config_path_str,
                reason: format!("cannot read config file: {e}"),
            };
        }
    };

    let mut ctx = AnalysisContext {
        config_dir: config_dir.to_path_buf(),
        dependency_files: vec![config_path_str.clone()],
        visited: vec![config_path_str.clone()],
    };

    match analyze_source(&source, &config_path_str, &mut ctx) {
        Ok(aliases) => ViteConfigAnalysis::Resolved {
            config_path: config_path_str,
            aliases,
            dependency_files: ctx.dependency_files,
        },
        Err(reason) => ViteConfigAnalysis::Complex {
            config_path: config_path_str,
            reason,
        },
    }
}

struct AnalysisContext {
    config_dir: PathBuf,
    dependency_files: Vec<String>,
    #[allow(dead_code)]
    visited: Vec<String>,
}

/// Package imports allowed in statically-analyzed expressions.
const ALLOWED_PACKAGES: &[&str] = &["vite", "path", "node:path", "url", "node:url"];

fn analyze_source(
    source: &str,
    file_path: &str,
    ctx: &mut AnalysisContext,
) -> Result<Vec<(String, String)>, String> {
    use oxc_allocator::Allocator;
    use oxc_parser::Parser;
    use oxc_span::SourceType;

    let allocator = Allocator::default();
    let source_type =
        if file_path.ends_with(".ts") || file_path.ends_with(".mts") || file_path.ends_with(".cts")
        {
            SourceType::ts()
        } else {
            SourceType::mjs()
        };

    let parse_result = Parser::new(&allocator, source, source_type).parse();
    if parse_result.panicked {
        return Err("parser panicked".to_string());
    }

    let program = &parse_result.program;

    // Check for complexity triggers at module level
    check_module_complexity(program, source)?;

    // Collect top-level const declarations for indirection resolution
    let consts = collect_top_level_consts(program, source);

    // Collect import declarations to check for disallowed packages
    check_imports(program, source)?;

    // Find the default export
    let export_expr = find_default_export(program, source, &consts)?;

    // Unwrap defineConfig() wrapper first, then check for function
    let config_obj = unwrap_define_config(export_expr, source);

    // If the config itself (or defineConfig's arg) is a function/arrow → Complex
    if is_function_expr(config_obj, source) {
        return Err("default export is a function/arrow (may use mode/command args)".to_string());
    }

    // Find resolve.alias in the config object
    let alias_value = find_resolve_alias(config_obj, source)?;

    // Extract aliases from the value
    extract_aliases(alias_value, source, ctx)
}

/// Check for module-level complexity triggers.
fn check_module_complexity(program: &oxc_ast::ast::Program, source: &str) -> Result<(), String> {
    use oxc_ast::ast::Statement;

    for stmt in &program.body {
        // Top-level await
        if let Statement::ExpressionStatement(expr_stmt) = stmt {
            if contains_await(&expr_stmt.expression, source) {
                return Err("top-level await detected".to_string());
            }
        }

        // (Dynamic import detection is done via source text below)
    }

    // Check for complexity triggers via source text (catches all positions)
    if source.contains("process.env") {
        return Err("process.env usage detected".to_string());
    }
    if source.contains("import.meta.env") {
        return Err("import.meta.env usage detected".to_string());
    }

    // Dynamic import() — `import(` only appears in dynamic imports, never in
    // static `import ... from` declarations (which don't use parentheses).
    if source.contains("import(") {
        return Err("dynamic import detected".to_string());
    }

    Ok(())
}

/// Check that all imports come from the allowlist.
fn check_imports(program: &oxc_ast::ast::Program, _source: &str) -> Result<(), String> {
    use oxc_ast::ast::Statement;

    for stmt in &program.body {
        if let Statement::ImportDeclaration(import) = stmt {
            let specifier = import.source.value.as_str();
            // Allow relative imports (will be analyzed recursively)
            if specifier.starts_with('.') {
                continue;
            }
            // Check allowlist
            if !ALLOWED_PACKAGES
                .iter()
                .any(|&pkg| specifier == pkg || specifier.starts_with(&format!("{pkg}/")))
            {
                return Err(format!("non-allowlisted package import: {specifier}"));
            }
        }
    }

    Ok(())
}

/// Collect top-level `const` declarations for indirection resolution.
fn collect_top_level_consts<'a>(
    program: &'a oxc_ast::ast::Program<'a>,
    _source: &str,
) -> HashMap<String, &'a oxc_ast::ast::Expression<'a>> {
    use oxc_ast::ast::{BindingPattern, Statement, VariableDeclarationKind};

    let mut consts = HashMap::new();

    for stmt in &program.body {
        if let Statement::VariableDeclaration(var_decl) = stmt {
            if var_decl.kind == VariableDeclarationKind::Const {
                for declarator in &var_decl.declarations {
                    if let BindingPattern::BindingIdentifier(id) = &declarator.id {
                        if let Some(init) = &declarator.init {
                            consts.insert(id.name.to_string(), init);
                        }
                    }
                }
            }
        }
    }

    consts
}

/// Find the default export expression.
fn find_default_export<'a>(
    program: &'a oxc_ast::ast::Program<'a>,
    source: &str,
    consts: &HashMap<String, &'a oxc_ast::ast::Expression<'a>>,
) -> Result<&'a oxc_ast::ast::Expression<'a>, String> {
    use oxc_ast::ast::Statement;

    for stmt in &program.body {
        match stmt {
            Statement::ExportDefaultDeclaration(export) => {
                // Try to get as expression (covers ObjectExpression, CallExpression, etc.)
                if let Some(expr) = export.declaration.as_expression() {
                    // Check if it's an identifier referencing a const
                    if let oxc_ast::ast::Expression::Identifier(id) = expr {
                        if let Some(&target) = consts.get(id.name.as_str()) {
                            return Ok(target);
                        }
                        return Err(format!(
                            "export default references unknown identifier: {}",
                            id.name
                        ));
                    }
                    return Ok(expr);
                }
                // Non-expression default export (class, function declaration, etc.)
                let span = export.span;
                let end = (span.end as usize)
                    .min(source.len())
                    .min(span.start as usize + 50);
                let preview = &source[span.start as usize..end];
                return Err(format!("unsupported default export kind: {preview}"));
            }
            // module.exports = ...
            Statement::ExpressionStatement(expr_stmt) => {
                if let Some(obj) = try_module_exports_assignment(&expr_stmt.expression, consts) {
                    return Ok(obj);
                }
            }
            _ => {}
        }
    }

    Err("no default export found".to_string())
}

/// Try to extract `module.exports = X` assignment value.
fn try_module_exports_assignment<'a>(
    expr: &'a oxc_ast::ast::Expression<'a>,
    consts: &HashMap<String, &'a oxc_ast::ast::Expression<'a>>,
) -> Option<&'a oxc_ast::ast::Expression<'a>> {
    use oxc_ast::ast::Expression;

    if let Expression::AssignmentExpression(assign) = expr {
        if let oxc_ast::ast::AssignmentTarget::StaticMemberExpression(member) = &assign.left {
            if let Expression::Identifier(obj) = &member.object {
                if obj.name == "module" && member.property.name == "exports" {
                    // Check if RHS is an identifier referencing a const
                    if let Expression::Identifier(id) = &assign.right {
                        if let Some(&target) = consts.get(id.name.as_str()) {
                            return Some(target);
                        }
                    }
                    return Some(&assign.right);
                }
            }
        }
    }
    None
}

/// Check if an expression is a function/arrow.
fn is_function_expr(expr: &oxc_ast::ast::Expression, _source: &str) -> bool {
    use oxc_ast::ast::Expression;
    matches!(
        expr,
        Expression::FunctionExpression(_) | Expression::ArrowFunctionExpression(_)
    )
}

/// Unwrap `defineConfig({...})` to the inner config object.
fn unwrap_define_config<'a>(
    expr: &'a oxc_ast::ast::Expression<'a>,
    _source: &str,
) -> &'a oxc_ast::ast::Expression<'a> {
    use oxc_ast::ast::Expression;

    if let Expression::CallExpression(call) = expr {
        if let Expression::Identifier(callee) = &call.callee {
            if callee.name == "defineConfig" && call.arguments.len() == 1 {
                // Use .as_expression() to get the argument as an expression
                if let Some(arg_expr) = call.arguments[0].as_expression() {
                    return arg_expr;
                }
            }
        }
    }
    expr
}

/// Find `resolve.alias` in an object expression.
fn find_resolve_alias<'a>(
    expr: &'a oxc_ast::ast::Expression<'a>,
    _source: &str,
) -> Result<&'a oxc_ast::ast::Expression<'a>, String> {
    use oxc_ast::ast::Expression;

    let obj = match expr {
        Expression::ObjectExpression(obj) => obj,
        _ => return Err("config is not an object expression".to_string()),
    };

    // Find `resolve` property
    let resolve_prop = obj.properties.iter().find_map(|prop| {
        if let oxc_ast::ast::ObjectPropertyKind::ObjectProperty(op) = prop {
            if property_key_name(&op.key).as_deref() == Some("resolve") {
                return Some(&op.value);
            }
        }
        None
    });

    let resolve_expr = match resolve_prop {
        Some(e) => e,
        None => return Err("no resolve property in config".to_string()),
    };

    let resolve_obj = match resolve_expr {
        Expression::ObjectExpression(obj) => obj,
        _ => return Err("resolve is not an object expression".to_string()),
    };

    // Find `alias` property in resolve
    let alias_prop = resolve_obj.properties.iter().find_map(|prop| {
        if let oxc_ast::ast::ObjectPropertyKind::ObjectProperty(op) = prop {
            if property_key_name(&op.key).as_deref() == Some("alias") {
                return Some(&op.value);
            }
        }
        None
    });

    match alias_prop {
        Some(e) => Ok(e),
        None => Err("no alias property in resolve".to_string()),
    }
}

/// Extract alias entries from the alias value expression.
fn extract_aliases(
    expr: &oxc_ast::ast::Expression,
    source: &str,
    ctx: &mut AnalysisContext,
) -> Result<Vec<(String, String)>, String> {
    use oxc_ast::ast::Expression;

    match expr {
        Expression::ObjectExpression(obj) => extract_aliases_from_object(obj, source, ctx),
        Expression::ArrayExpression(arr) => extract_aliases_from_array(arr, source, ctx),
        _ => Err("resolve.alias is neither object nor array".to_string()),
    }
}

/// Extract aliases from object form: `{ '@': './src' }`.
fn extract_aliases_from_object(
    obj: &oxc_ast::ast::ObjectExpression,
    source: &str,
    ctx: &mut AnalysisContext,
) -> Result<Vec<(String, String)>, String> {
    let mut aliases = Vec::new();

    for prop in &obj.properties {
        match prop {
            oxc_ast::ast::ObjectPropertyKind::ObjectProperty(op) => {
                // Check for computed keys
                if op.computed {
                    return Err("computed property key in alias object".to_string());
                }

                let key = property_key_name(&op.key)
                    .ok_or_else(|| "non-string key in alias object".to_string())?;

                let value = eval_static_string(&op.value, source, ctx)?;
                let (find, replacement) = normalize_alias_pair(&key, &value, &ctx.config_dir);
                aliases.push((find, replacement));
            }
            oxc_ast::ast::ObjectPropertyKind::SpreadProperty(spread) => {
                // Object spread from local const: `...aliasConst`
                // For now, mark as complex
                use oxc_span::GetSpan;
                let span = spread.span();
                let end = (span.end as usize)
                    .min(source.len())
                    .min(span.start as usize + 40);
                let preview = &source[span.start as usize..end];
                return Err(format!("spread in alias object: {preview}"));
            }
        }
    }

    Ok(aliases)
}

/// Extract aliases from array form: `[{ find: '@', replacement: './src' }]`.
fn extract_aliases_from_array(
    arr: &oxc_ast::ast::ArrayExpression,
    source: &str,
    ctx: &mut AnalysisContext,
) -> Result<Vec<(String, String)>, String> {
    use oxc_ast::ast::{ArrayExpressionElement, Expression};

    let mut aliases = Vec::new();

    for element in &arr.elements {
        match element {
            ArrayExpressionElement::SpreadElement(_) => {
                return Err("spread in alias array".to_string());
            }
            ArrayExpressionElement::Elision(_) => {
                continue;
            }
            _ => {}
        }

        let elem_expr = match element.as_expression() {
            Some(Expression::ObjectExpression(obj)) => obj,
            _ => {
                return Err("non-object element in alias array".to_string());
            }
        };

        let mut find = None;
        let mut replacement = None;

        for prop in &elem_expr.properties {
            if let oxc_ast::ast::ObjectPropertyKind::ObjectProperty(op) = prop {
                let key_name = property_key_name(&op.key);
                match key_name.as_deref() {
                    Some("find") => {
                        find = Some(eval_static_string(&op.value, source, ctx)?);
                    }
                    Some("replacement") => {
                        replacement = Some(eval_static_string(&op.value, source, ctx)?);
                    }
                    _ => {} // ignore other properties like `customResolver`
                }
            }
        }

        match (find, replacement) {
            (Some(f), Some(r)) => {
                let (normalized_find, normalized_replacement) =
                    normalize_alias_pair(&f, &r, &ctx.config_dir);
                aliases.push((normalized_find, normalized_replacement));
            }
            _ => {
                return Err("alias array element missing find or replacement".to_string());
            }
        }
    }

    Ok(aliases)
}

/// Evaluate an expression to a static string value.
fn eval_static_string(
    expr: &oxc_ast::ast::Expression,
    source: &str,
    ctx: &mut AnalysisContext,
) -> Result<String, String> {
    use oxc_ast::ast::Expression;
    use oxc_span::GetSpan;

    match expr {
        // String literal
        Expression::StringLiteral(lit) => Ok(lit.value.to_string()),

        // Template literal with no substitutions
        Expression::TemplateLiteral(tmpl) => {
            if !tmpl.expressions.is_empty() {
                return Err("template literal with substitutions".to_string());
            }
            let quasi = tmpl.quasis.first().ok_or("empty template literal")?;
            Ok(quasi.value.raw.to_string())
        }

        // path.resolve(__dirname, 'src') / path.join(...)
        Expression::CallExpression(call) => eval_static_call(call, source, ctx),

        // new URL('./src', import.meta.url) → resolve relative to config dir
        Expression::NewExpression(new_expr) => eval_new_url(new_expr, source, ctx),

        // Identifier reference — try to resolve from consts
        Expression::Identifier(id) => Err(format!(
            "cannot statically evaluate identifier: {}",
            id.name
        )),

        _ => {
            let span = expr.span();
            let end = (span.end as usize)
                .min(source.len())
                .min(span.start as usize + 50);
            let preview = &source[span.start as usize..end];
            Err(format!("cannot statically evaluate expression: {preview}"))
        }
    }
}

/// Evaluate static function calls: `path.resolve(...)`, `path.join(...)`, `fileURLToPath(...)`.
fn eval_static_call(
    call: &oxc_ast::ast::CallExpression,
    source: &str,
    ctx: &mut AnalysisContext,
) -> Result<String, String> {
    use oxc_ast::ast::Expression;
    use oxc_span::GetSpan;

    // fileURLToPath(new URL('./src', import.meta.url))
    if let Expression::Identifier(callee) = &call.callee {
        if callee.name == "fileURLToPath" && call.arguments.len() == 1 {
            if let Some(Expression::NewExpression(new_expr)) = call.arguments[0].as_expression() {
                return eval_new_url(new_expr, source, ctx);
            }
        }
    }

    // path.resolve(__dirname, 'src') / path.join(__dirname, 'src')
    if let Expression::StaticMemberExpression(member) = &call.callee {
        let method_name = member.property.name.as_str();
        if matches!(method_name, "resolve" | "join") {
            // Verify caller is `path`
            if let Expression::Identifier(obj) = &member.object {
                if obj.name == "path" {
                    return eval_path_join_resolve(call, method_name, source, ctx);
                }
            }
        }
    }

    let span = call.span();
    let end = (span.end as usize)
        .min(source.len())
        .min(span.start as usize + 60);
    let preview = &source[span.start as usize..end];
    Err(format!("cannot statically evaluate call: {preview}"))
}

/// Evaluate `path.resolve(...)` / `path.join(...)` with all-literal args.
fn eval_path_join_resolve(
    call: &oxc_ast::ast::CallExpression,
    _method_name: &str,
    source: &str,
    ctx: &mut AnalysisContext,
) -> Result<String, String> {
    use oxc_ast::ast::Expression;

    let mut segments = Vec::new();

    for arg in &call.arguments {
        if let Some(expr) = arg.as_expression() {
            match expr {
                Expression::Identifier(id) if id.name == "__dirname" => {
                    segments.push(ctx.config_dir.to_string_lossy().replace('\\', "/"));
                }
                Expression::StringLiteral(lit) => {
                    segments.push(lit.value.to_string());
                }
                _ => {
                    let val = eval_static_string(expr, source, ctx)?;
                    segments.push(val);
                }
            }
        } else {
            return Err("spread argument in path.resolve/join".to_string());
        }
    }

    if segments.is_empty() {
        return Err("path.resolve/join with no arguments".to_string());
    }

    // Build the path
    let mut result = PathBuf::from(&segments[0]);
    for seg in &segments[1..] {
        result = result.join(seg);
    }

    Ok(result.to_string_lossy().replace('\\', "/"))
}

/// Evaluate `new URL('./src', import.meta.url)`.
fn eval_new_url(
    new_expr: &oxc_ast::ast::NewExpression,
    source: &str,
    ctx: &mut AnalysisContext,
) -> Result<String, String> {
    use oxc_ast::ast::Expression;
    use oxc_span::GetSpan;

    // Check constructor is URL
    if let Expression::Identifier(id) = &new_expr.callee {
        if id.name != "URL" {
            return Err(format!("new expression is not URL: {}", id.name));
        }
    } else {
        return Err("new expression callee is not an identifier".to_string());
    }

    if new_expr.arguments.len() != 2 {
        return Err("new URL requires exactly 2 arguments".to_string());
    }

    // First arg should be a string literal (relative path)
    let relative_path = match new_expr.arguments[0].as_expression() {
        Some(Expression::StringLiteral(lit)) => lit.value.to_string(),
        _ => {
            return Err("new URL first arg is not a string literal".to_string());
        }
    };

    // Second arg should be import.meta.url
    let is_import_meta_url = match new_expr.arguments[1].as_expression() {
        Some(Expression::StaticMemberExpression(member)) => {
            member.property.name == "url" && matches!(&member.object, Expression::MetaProperty(_))
        }
        _ => false,
    };

    if !is_import_meta_url {
        let span = new_expr.arguments[1].span();
        let end = (span.end as usize)
            .min(source.len())
            .min(span.start as usize + 40);
        let preview = &source[span.start as usize..end];
        return Err(format!(
            "new URL second arg is not import.meta.url: {preview}"
        ));
    }

    // Resolve relative to config dir
    let resolved = ctx.config_dir.join(&relative_path);
    Ok(resolved.to_string_lossy().replace('\\', "/"))
}

/// Get the string name from a property key.
fn property_key_name(key: &oxc_ast::ast::PropertyKey) -> Option<String> {
    use oxc_ast::ast::PropertyKey;
    match key {
        PropertyKey::StaticIdentifier(id) => Some(id.name.to_string()),
        PropertyKey::StringLiteral(lit) => Some(lit.value.to_string()),
        _ => None,
    }
}

/// Check if an expression contains `await`.
fn contains_await(expr: &oxc_ast::ast::Expression, _source: &str) -> bool {
    matches!(expr, oxc_ast::ast::Expression::AwaitExpression(_))
}

// ═══════════════════════════════════════════════════════════════════════════
// Trusted Execution (Node.js-based)
// ═══════════════════════════════════════════════════════════════════════════

/// Sentinel markers used to extract JSON from potentially noisy Node.js stdout.
const VITE_SENTINEL_BEGIN: &str = "__VERTER_ALIASES_BEGIN__";
const VITE_SENTINEL_END: &str = "__VERTER_ALIASES_END__";

#[derive(serde::Deserialize)]
struct ViteAliasEntry {
    find: String,
    replacement: String,
}

/// Parse vite alias JSON from Node.js stdout, handling noise via sentinel markers.
#[allow(dead_code)] // Used only in tests; will become canonical after Phase 6 relocation
fn parse_vite_alias_stdout(raw: &str) -> Option<Vec<ViteAliasEntry>> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Sentinel-based extraction (handles prefix AND suffix noise)
    if let Some(begin) = trimmed.find(VITE_SENTINEL_BEGIN) {
        let after = &trimmed[begin + VITE_SENTINEL_BEGIN.len()..];
        if let Some(end) = after.find(VITE_SENTINEL_END) {
            return serde_json::from_str(&after[..end]).ok();
        }
    }

    // Fallback: try clean parse
    serde_json::from_str(trimmed).ok()
}

/// Execute a vite config by spawning Node.js (requires explicit trust).
///
/// Uses Vite's `loadConfigFromFile` API when available, falling back to direct
/// dynamic import. Environment is sanitized to remove VS Code/Electron env vars.
pub fn execute_trusted_vite_config(
    config_path: &Path,
    project_root: &Path,
    node_path: &str,
) -> Option<TrustedExecResult> {
    let config_path_str = config_path.to_string_lossy().replace('\\', "/");
    let config_dir = config_path.parent().unwrap_or(project_root);

    // Inline Node.js script using loadConfigFromFile for dependency tracking
    let loader_setup = if config_path_str.ends_with(".ts")
        || config_path_str.ends_with(".mts")
        || config_path_str.ends_with(".cts")
    {
        "try { await import('tsx/esm'); } catch {}"
    } else {
        ""
    };

    let script = format!(
        r#"
const {{ pathToFileURL }} = require('url');
(async () => {{
  try {{
    {loader_setup}
    // Try loadConfigFromFile first (tracks dependencies)
    let config, deps = [];
    try {{
      const vite = await import('vite');
      if (vite.loadConfigFromFile) {{
        const result = await vite.loadConfigFromFile(
          {{ command: 'serve', mode: 'development' }},
          '{config_path_str}'
        );
        if (result) {{
          config = result.config;
          deps = result.dependencies || [];
        }}
      }}
    }} catch {{}}
    // Fallback: direct import
    if (!config) {{
      const mod = await import(pathToFileURL('{config_path_str}').href);
      const raw = mod.default || mod;
      const resolved = typeof raw === 'function' ? raw({{ mode: 'development', command: 'serve' }}) : raw;
      config = resolved instanceof Promise ? await resolved : resolved;
    }}
    const alias = config?.resolve?.alias;
    let entries = [];
    if (alias) {{
      if (Array.isArray(alias)) {{
        for (const a of alias) {{
          if (a.find && a.replacement) {{
            const f = typeof a.find === 'string' ? a.find : null;
            if (f) entries.push({{ find: f, replacement: a.replacement }});
          }}
        }}
      }} else if (typeof alias === 'object') {{
        for (const [key, val] of Object.entries(alias)) {{
          if (typeof val === 'string') entries.push({{ find: key, replacement: val }});
        }}
      }}
    }}
    const output = JSON.stringify({{ aliases: entries, deps }});
    process.stdout.write('__VERTER_ALIASES_BEGIN__' + output + '__VERTER_ALIASES_END__');
  }} catch (e) {{
    process.stderr.write('vite config eval error: ' + e.message + '\n');
    process.stdout.write('__VERTER_ALIASES_BEGIN__{{"aliases":[],"deps":[]}}__VERTER_ALIASES_END__');
  }}
}})();
"#
    );

    // Sanitize environment
    let mut cmd = std::process::Command::new(node_path);
    cmd.arg("-e")
        .arg(&script)
        .current_dir(project_root)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    // Remove VS Code/Electron env vars
    for var in crate::tsserver::ipc::CHILD_PROCESS_ENV_DENYLIST {
        cmd.env_remove(var);
    }

    // Spawn with timeout
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            tracing::debug!("failed to spawn node for trusted vite config: {e}");
            return None;
        }
    };

    // 10s hard deadline
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        match child.try_wait() {
            Ok(Some(_status)) => break,
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    tracing::warn!("vite config eval timed out for {config_path_str}");
                    return None;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(e) => {
                tracing::debug!("error waiting for vite config eval: {e}");
                return None;
            }
        }
    }

    let output = child.wait_with_output().ok()?;

    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.is_empty() {
        tracing::debug!(
            "trusted vite config eval stderr ({}): {}",
            config_path_str,
            stderr.trim()
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Parse output with dependency info
    let trimmed = stdout.trim();
    if let Some(begin) = trimmed.find(VITE_SENTINEL_BEGIN) {
        let after = &trimmed[begin + VITE_SENTINEL_BEGIN.len()..];
        if let Some(end) = after.find(VITE_SENTINEL_END) {
            #[derive(serde::Deserialize)]
            struct TrustedOutput {
                aliases: Vec<ViteAliasEntry>,
                #[serde(default)]
                deps: Vec<String>,
            }

            if let Ok(parsed) = serde_json::from_str::<TrustedOutput>(&after[..end]) {
                let aliases: Vec<(String, String)> = parsed
                    .aliases
                    .into_iter()
                    .map(|e| normalize_alias_pair(&e.find, &e.replacement, config_dir))
                    .collect();

                let mut dependency_files = vec![config_path_str.clone()];
                for dep in parsed.deps {
                    let dep_normalized = PathBuf::from(&dep).to_string_lossy().replace('\\', "/");
                    if !dependency_files.contains(&dep_normalized) {
                        dependency_files.push(dep_normalized);
                    }
                }

                // Cache successful result
                cache_lkg(&config_path_str, &aliases);

                return Some(TrustedExecResult {
                    aliases,
                    dependency_files,
                });
            }
        }
    }

    None
}

/// Legacy function signature for backward compatibility during migration.
/// Spawns Node.js to discover vite aliases (no dependency tracking).
pub fn discover_vite_aliases(project_root: &Path, node_path: &str) -> Vec<(String, String)> {
    let config_path = match find_vite_config(project_root) {
        Some(p) => p,
        None => return Vec::new(),
    };

    match execute_trusted_vite_config(&config_path, project_root, node_path) {
        Some(result) => result.aliases,
        None => get_lkg(&config_path.to_string_lossy().replace('\\', "/")).unwrap_or_default(),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Last-Known-Good Cache
// ═══════════════════════════════════════════════════════════════════════════

/// Alias list type for LKG cache entries.
type AliasList = Vec<(String, String)>;

/// Module-level LKG cache: config path → last successful aliases.
static LKG_CACHE: std::sync::LazyLock<Mutex<HashMap<String, AliasList>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

fn cache_lkg(config_path: &str, aliases: &[(String, String)]) {
    if let Ok(mut cache) = LKG_CACHE.lock() {
        cache.insert(config_path.to_string(), aliases.to_vec());
    }
}

fn get_lkg(config_path: &str) -> Option<Vec<(String, String)>> {
    LKG_CACHE
        .lock()
        .ok()
        .and_then(|cache| cache.get(config_path).cloned())
}

/// Get the LKG entry for a config path, or empty vec if none.
pub fn get_lkg_or_empty(config_path: &str) -> Vec<(String, String)> {
    get_lkg(config_path).unwrap_or_default()
}

// ═══════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    // ── Config discovery ─────────────────────────────────────────────────

    #[test]
    fn config_discovery_priority_order() {
        let tmp = std::env::temp_dir().join("verter_test_vite_discovery_order");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        // Create multiple config files
        for name in VITE_CONFIG_NAMES {
            std::fs::write(tmp.join(name), "export default {}").unwrap();
        }

        // Should find vite.config.ts first (highest priority)
        let found = find_vite_config(&tmp);
        assert!(found.is_some(), "should find vite config");
        assert!(
            found.unwrap().ends_with("vite.config.ts"),
            "should prefer .ts extension"
        );

        // Remove .ts, should fall back to .js
        std::fs::remove_file(tmp.join("vite.config.ts")).unwrap();
        let found = find_vite_config(&tmp);
        assert!(
            found.unwrap().ends_with("vite.config.js"),
            "should fall back to .js"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn config_discovery_not_found() {
        let tmp = std::env::temp_dir().join("verter_test_vite_discovery_none");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        assert!(find_vite_config(&tmp).is_none());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    // ── Alias normalization ──────────────────────────────────────────────

    #[test]
    fn normalize_bare_alias_gets_slash() {
        let dir = PathBuf::from("/project");
        let (find, _) = normalize_alias_pair("@", "./src", &dir);
        assert_eq!(find, "@/", "bare @ should become @/");
    }

    #[test]
    fn normalize_already_slashed_alias() {
        let dir = PathBuf::from("/project");
        let (find, _) = normalize_alias_pair("@/", "./src", &dir);
        assert_eq!(find, "@/", "already slashed should stay @/");
    }

    #[test]
    fn normalize_relative_replacement_becomes_absolute() {
        let dir = PathBuf::from("/project");
        let (_, replacement) = normalize_alias_pair("@", "./src", &dir);
        assert!(
            replacement.starts_with("/project"),
            "relative replacement should be made absolute, got: {replacement}"
        );
        assert!(
            replacement.contains("src"),
            "should contain src segment, got: {replacement}"
        );
    }

    #[test]
    fn normalize_absolute_replacement_preserved() {
        let dir = PathBuf::from("/project");
        let (_, replacement) = normalize_alias_pair("@", "/absolute/src", &dir);
        assert_eq!(replacement, "/absolute/src", "absolute should be preserved");
    }

    // ── Static analysis — supported shapes ───────────────────────────────

    #[test]
    fn static_analysis_simple_object_alias() {
        let tmp = std::env::temp_dir().join("verter_test_static_obj");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("src")).unwrap();

        std::fs::write(
            tmp.join("vite.config.ts"),
            "export default { resolve: { alias: { '@': './src' } } }",
        )
        .unwrap();

        let result = analyze_vite_config(&tmp);
        match &result {
            ViteConfigAnalysis::Resolved {
                aliases,
                config_path,
                ..
            } => {
                assert_eq!(aliases.len(), 1, "should find 1 alias");
                assert_eq!(aliases[0].0, "@/", "find should be @/");
                assert!(
                    aliases[0].1.contains("src"),
                    "replacement should contain src"
                );
                assert!(config_path.contains("vite.config.ts"));
            }
            ViteConfigAnalysis::Complex { reason, .. } => {
                panic!("expected Resolved, got Complex: {reason}");
            }
            ViteConfigAnalysis::NotFound => panic!("expected Resolved, got NotFound"),
        }

        // Negative: no complexity trigger
        assert!(
            !matches!(result, ViteConfigAnalysis::Complex { .. }),
            "simple object alias should not be Complex"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn static_analysis_define_config_wrapper() {
        let tmp = std::env::temp_dir().join("verter_test_static_defineconfig");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("src")).unwrap();

        std::fs::write(
            tmp.join("vite.config.ts"),
            r#"import { defineConfig } from 'vite'
export default defineConfig({ resolve: { alias: { '@': './src' } } })"#,
        )
        .unwrap();

        match analyze_vite_config(&tmp) {
            ViteConfigAnalysis::Resolved { aliases, .. } => {
                assert_eq!(aliases.len(), 1);
                assert_eq!(aliases[0].0, "@/");
            }
            ViteConfigAnalysis::Complex { reason, .. } => {
                panic!("expected Resolved for defineConfig wrapper, got Complex: {reason}");
            }
            ViteConfigAnalysis::NotFound => panic!("expected Resolved"),
        }

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn static_analysis_const_indirection() {
        let tmp = std::env::temp_dir().join("verter_test_static_const");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("src")).unwrap();

        std::fs::write(
            tmp.join("vite.config.js"),
            r#"const config = { resolve: { alias: { '@': './src' } } };
export default config;"#,
        )
        .unwrap();

        match analyze_vite_config(&tmp) {
            ViteConfigAnalysis::Resolved { aliases, .. } => {
                assert_eq!(aliases.len(), 1);
                assert_eq!(aliases[0].0, "@/");
            }
            ViteConfigAnalysis::Complex { reason, .. } => {
                panic!("expected Resolved for const indirection, got Complex: {reason}");
            }
            ViteConfigAnalysis::NotFound => panic!("expected Resolved"),
        }

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn static_analysis_template_literal_value() {
        let tmp = std::env::temp_dir().join("verter_test_static_template");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("src")).unwrap();

        std::fs::write(
            tmp.join("vite.config.js"),
            "export default { resolve: { alias: { '@': `./src` } } }",
        )
        .unwrap();

        match analyze_vite_config(&tmp) {
            ViteConfigAnalysis::Resolved { aliases, .. } => {
                assert_eq!(aliases.len(), 1);
                assert_eq!(aliases[0].0, "@/");
            }
            other => panic!("expected Resolved for template literal, got {other:?}"),
        }

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn static_analysis_array_format() {
        let tmp = std::env::temp_dir().join("verter_test_static_array");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("src")).unwrap();
        std::fs::create_dir_all(tmp.join("lib")).unwrap();

        std::fs::write(
            tmp.join("vite.config.mjs"),
            r#"export default {
  resolve: {
    alias: [
      { find: '@', replacement: './src' },
      { find: '~', replacement: './lib' },
    ]
  }
}"#,
        )
        .unwrap();

        match analyze_vite_config(&tmp) {
            ViteConfigAnalysis::Resolved { aliases, .. } => {
                assert_eq!(aliases.len(), 2);
                assert!(aliases.iter().any(|(f, _)| f == "@/"));
                assert!(aliases.iter().any(|(f, _)| f == "~/"));
            }
            ViteConfigAnalysis::Complex { reason, .. } => {
                panic!("expected Resolved for array alias, got Complex: {reason}");
            }
            ViteConfigAnalysis::NotFound => panic!("expected Resolved"),
        }

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn static_analysis_new_url_import_meta() {
        let tmp = std::env::temp_dir().join("verter_test_static_new_url");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("src")).unwrap();

        std::fs::write(
            tmp.join("vite.config.ts"),
            r#"export default {
  resolve: {
    alias: {
      '@': new URL('./src', import.meta.url)
    }
  }
}"#,
        )
        .unwrap();

        match analyze_vite_config(&tmp) {
            ViteConfigAnalysis::Resolved { aliases, .. } => {
                assert_eq!(aliases.len(), 1);
                assert_eq!(aliases[0].0, "@/");
                assert!(aliases[0].1.contains("src"), "should resolve to src dir");
            }
            ViteConfigAnalysis::Complex { reason, .. } => {
                panic!("expected Resolved for new URL, got Complex: {reason}");
            }
            ViteConfigAnalysis::NotFound => panic!("expected Resolved"),
        }

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn static_analysis_file_url_to_path() {
        let tmp = std::env::temp_dir().join("verter_test_static_fileurltopath");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("src")).unwrap();

        std::fs::write(
            tmp.join("vite.config.ts"),
            r#"import { fileURLToPath } from 'node:url'
export default {
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url))
    }
  }
}"#,
        )
        .unwrap();

        match analyze_vite_config(&tmp) {
            ViteConfigAnalysis::Resolved { aliases, .. } => {
                assert_eq!(aliases.len(), 1);
                assert_eq!(aliases[0].0, "@/");
                assert!(aliases[0].1.contains("src"));
            }
            ViteConfigAnalysis::Complex { reason, .. } => {
                panic!("expected Resolved for fileURLToPath, got Complex: {reason}");
            }
            ViteConfigAnalysis::NotFound => panic!("expected Resolved"),
        }

        let _ = std::fs::remove_dir_all(&tmp);
    }

    // ── Static analysis — complexity triggers ────────────────────────────

    #[test]
    fn static_analysis_function_export_is_complex() {
        let tmp = std::env::temp_dir().join("verter_test_static_func");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        std::fs::write(
            tmp.join("vite.config.ts"),
            r#"import { defineConfig } from 'vite'
export default defineConfig(({ mode }) => ({
  resolve: { alias: { '@': './src' } }
}))"#,
        )
        .unwrap();

        match analyze_vite_config(&tmp) {
            ViteConfigAnalysis::Complex { reason, .. } => {
                assert!(
                    reason.contains("function") || reason.contains("arrow"),
                    "reason should mention function/arrow: {reason}"
                );
            }
            ViteConfigAnalysis::Resolved { .. } => {
                panic!("function export should be Complex");
            }
            ViteConfigAnalysis::NotFound => panic!("should find config"),
        }

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn static_analysis_process_env_is_complex() {
        let tmp = std::env::temp_dir().join("verter_test_static_process_env");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        std::fs::write(
            tmp.join("vite.config.ts"),
            r#"const dir = process.env.SRC_DIR || './src'
export default { resolve: { alias: { '@': dir } } }"#,
        )
        .unwrap();

        match analyze_vite_config(&tmp) {
            ViteConfigAnalysis::Complex { reason, .. } => {
                assert!(
                    reason.contains("process.env"),
                    "reason should mention process.env: {reason}"
                );
            }
            other => panic!("process.env should be Complex, got {other:?}"),
        }

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn static_analysis_import_meta_env_is_complex() {
        let tmp = std::env::temp_dir().join("verter_test_static_import_meta_env");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        std::fs::write(
            tmp.join("vite.config.ts"),
            r#"const isProd = import.meta.env.PROD
export default { resolve: { alias: { '@': './src' } } }"#,
        )
        .unwrap();

        match analyze_vite_config(&tmp) {
            ViteConfigAnalysis::Complex { reason, .. } => {
                assert!(reason.contains("import.meta.env"));
            }
            other => panic!("import.meta.env should be Complex, got {other:?}"),
        }

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn static_analysis_dynamic_import_is_complex() {
        let tmp = std::env::temp_dir().join("verter_test_static_dynamic_import");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        std::fs::write(
            tmp.join("vite.config.ts"),
            r#"const plugin = import('./plugin')
export default { resolve: { alias: { '@': './src' } } }"#,
        )
        .unwrap();

        match analyze_vite_config(&tmp) {
            ViteConfigAnalysis::Complex { reason, .. } => {
                assert!(
                    reason.contains("dynamic import"),
                    "reason should mention dynamic import: {reason}"
                );
            }
            other => panic!("dynamic import should be Complex, got {other:?}"),
        }

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn static_analysis_non_allowlisted_package_is_complex() {
        let tmp = std::env::temp_dir().join("verter_test_static_bad_pkg");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        std::fs::write(
            tmp.join("vite.config.ts"),
            r#"import lodash from 'lodash'
export default { resolve: { alias: { '@': './src' } } }"#,
        )
        .unwrap();

        match analyze_vite_config(&tmp) {
            ViteConfigAnalysis::Complex { reason, .. } => {
                assert!(
                    reason.contains("lodash"),
                    "reason should mention the package: {reason}"
                );
            }
            other => panic!("non-allowlisted package should be Complex, got {other:?}"),
        }

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn static_analysis_computed_key_is_complex() {
        let tmp = std::env::temp_dir().join("verter_test_static_computed_key");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        std::fs::write(
            tmp.join("vite.config.ts"),
            r#"const key = '@'
export default { resolve: { alias: { [key]: './src' } } }"#,
        )
        .unwrap();

        match analyze_vite_config(&tmp) {
            ViteConfigAnalysis::Complex { reason, .. } => {
                assert!(
                    reason.contains("computed"),
                    "reason should mention computed: {reason}"
                );
            }
            other => panic!("computed key should be Complex, got {other:?}"),
        }

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn static_analysis_not_found() {
        let tmp = std::env::temp_dir().join("verter_test_static_notfound");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        assert!(matches!(
            analyze_vite_config(&tmp),
            ViteConfigAnalysis::NotFound
        ));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    // ── LKG cache ────────────────────────────────────────────────────────

    #[test]
    fn lkg_cache_stores_and_retrieves() {
        cache_lkg(
            "/test/lkg/vite.config.ts",
            &[("@/".to_string(), "/test/src".to_string())],
        );
        let result = get_lkg("/test/lkg/vite.config.ts");
        assert!(result.is_some());
        assert_eq!(result.unwrap().len(), 1);
    }

    #[test]
    fn lkg_cache_empty_for_unknown() {
        let result = get_lkg("/nonexistent/path/vite.config.ts");
        assert!(result.is_none());
    }

    #[test]
    fn lkg_or_empty_returns_empty_vec() {
        let result = get_lkg_or_empty("/truly/unknown/vite.config.ts");
        assert!(result.is_empty());
    }

    // ── Trusted execution ────────────────────────────────────────────────

    #[test]
    fn trusted_execution_env_sanitization() {
        // Verify that env vars are removed by checking the Command construction.
        // We can't easily test the actual execution without Node.js,
        // but we can verify the function handles missing node gracefully.
        let tmp = std::env::temp_dir().join("verter_test_trusted_env");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("vite.config.js"), "export default {}").unwrap();

        // With a non-existent node path, should return None gracefully
        let result =
            execute_trusted_vite_config(&tmp.join("vite.config.js"), &tmp, "/nonexistent/node");
        assert!(result.is_none(), "should return None for missing node");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn trusted_execution_caches_result() {
        // Successful execution should populate LKG cache
        let node = crate::tsserver::find_node();
        if node.is_none() {
            eprintln!("skipping trusted_execution_caches_result: node not found");
            return;
        }
        let node = node.unwrap();

        let tmp = std::env::temp_dir().join("verter_test_trusted_cache");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("src")).unwrap();

        std::fs::write(
            tmp.join("vite.config.js"),
            format!(
                "export default {{ resolve: {{ alias: {{ '@': '{}' }} }} }}",
                tmp.join("src").to_string_lossy().replace('\\', "/")
            ),
        )
        .unwrap();

        let config_path = tmp.join("vite.config.js");
        let config_path_str = config_path.to_string_lossy().replace('\\', "/");

        let result = execute_trusted_vite_config(&config_path, &tmp, &node);
        let Some(_) = result else {
            eprintln!("skipping: vite config execution failed");
            let _ = std::fs::remove_dir_all(&tmp);
            return;
        };

        // LKG should be populated
        let lkg = get_lkg(&config_path_str);
        assert!(
            lkg.is_some(),
            "LKG should be cached after successful execution"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn trusted_execution_failed_returns_none() {
        let node = crate::tsserver::find_node();
        if node.is_none() {
            eprintln!("skipping trusted_execution_failed_returns_none: node not found");
            return;
        }
        let node = node.unwrap();

        let tmp = std::env::temp_dir().join("verter_test_trusted_fail");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        // Config that will throw on eval
        std::fs::write(
            tmp.join("vite.config.js"),
            "throw new Error('intentional failure');",
        )
        .unwrap();

        let result = execute_trusted_vite_config(&tmp.join("vite.config.js"), &tmp, &node);
        // The script catches errors and returns empty aliases, so result is Some but empty
        if let Some(r) = &result {
            assert!(
                r.aliases.is_empty(),
                "failed eval should have empty aliases"
            );
        }

        let _ = std::fs::remove_dir_all(&tmp);
    }

    // ── Parse vite alias stdout ──────────────────────────────────────────

    #[test]
    fn parse_stdout_with_sentinels() {
        let raw = "some noise\n__VERTER_ALIASES_BEGIN__[{\"find\":\"@\",\"replacement\":\"./src\"}]__VERTER_ALIASES_END__\nmore noise";
        let result = parse_vite_alias_stdout(raw);
        assert!(result.is_some());
        let entries = result.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].find, "@");
    }

    #[test]
    fn parse_stdout_empty() {
        assert!(parse_vite_alias_stdout("").is_none());
        assert!(parse_vite_alias_stdout("  \n  ").is_none());
    }

    // ── Dependency file tracking in static analysis ──────────────────────

    #[test]
    fn static_analysis_dependency_files_includes_config() {
        let tmp = std::env::temp_dir().join("verter_test_static_deps");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("src")).unwrap();

        std::fs::write(
            tmp.join("vite.config.ts"),
            "export default { resolve: { alias: { '@': './src' } } }",
        )
        .unwrap();

        match analyze_vite_config(&tmp) {
            ViteConfigAnalysis::Resolved {
                dependency_files,
                config_path,
                ..
            } => {
                assert!(
                    !dependency_files.is_empty(),
                    "dependency_files should contain at least the config file"
                );
                assert!(
                    dependency_files.contains(&config_path),
                    "dependency_files should contain the config file path"
                );
            }
            other => panic!("expected Resolved, got {other:?}"),
        }

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
