//! Vite config alias discovery: static analysis (OXC) + trusted execution fallback.
//!
//! Extracts `resolve.alias` from Vite config files using two strategies:
//! 1. **Static analysis** (default): Parses config with OXC, extracts aliases
//!    from object/array literals without executing code.
//! 2. **Trusted execution** (opt-in): Spawns Node.js when static analysis
//!    cannot handle the config's complexity.

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
pub fn find_vite_config(
    ws: &dyn crate::traits::WorkspaceAccess,
    project_root: &str,
) -> Option<String> {
    VITE_CONFIG_NAMES
        .iter()
        .map(|name| crate::resolver::join_paths(project_root, name))
        .find(|p| ws.file_exists(p))
}

// ═══════════════════════════════════════════════════════════════════════════
// Alias Normalization
// ═══════════════════════════════════════════════════════════════════════════

/// Normalize an alias pair: bare `@` -> `@/`, relative replacement -> absolute.
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
pub fn analyze_vite_config(
    ws: &dyn crate::traits::WorkspaceAccess,
    project_root: &str,
) -> ViteConfigAnalysis {
    let config_path_str = match find_vite_config(ws, project_root) {
        Some(p) => p,
        None => return ViteConfigAnalysis::NotFound,
    };

    let config_dir_str = crate::resolver::parent_dir(&config_path_str);
    let config_dir = std::path::Path::new(&config_dir_str);

    let source = match ws.read_file(&config_path_str) {
        Some(s) => s.to_string(),
        None => {
            return ViteConfigAnalysis::Complex {
                config_path: config_path_str,
                reason: "cannot read config file".to_string(),
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
    #[allow(dead_code)] // Reserved for circular reference detection
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

    check_module_complexity(program, source)?;

    let consts = collect_top_level_consts(program, source);
    check_imports(program, source)?;

    let export_expr = find_default_export(program, source, &consts)?;
    let config_obj = unwrap_define_config(export_expr, source);

    if is_function_expr(config_obj, source) {
        return Err("default export is a function/arrow (may use mode/command args)".to_string());
    }

    let alias_value = find_resolve_alias(config_obj, source)?;
    extract_aliases(alias_value, source, ctx)
}

fn check_module_complexity(program: &oxc_ast::ast::Program, source: &str) -> Result<(), String> {
    use oxc_ast::ast::Statement;

    for stmt in &program.body {
        if let Statement::ExpressionStatement(expr_stmt) = stmt {
            if contains_await(&expr_stmt.expression, source) {
                return Err("top-level await detected".to_string());
            }
        }
    }

    if source.contains("process.env") {
        return Err("process.env usage detected".to_string());
    }
    if source.contains("import.meta.env") {
        return Err("import.meta.env usage detected".to_string());
    }
    if source.contains("import(") {
        return Err("dynamic import detected".to_string());
    }

    Ok(())
}

fn check_imports(program: &oxc_ast::ast::Program, _source: &str) -> Result<(), String> {
    use oxc_ast::ast::Statement;

    for stmt in &program.body {
        if let Statement::ImportDeclaration(import) = stmt {
            let specifier = import.source.value.as_str();
            if specifier.starts_with('.') {
                continue;
            }
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

fn find_default_export<'a>(
    program: &'a oxc_ast::ast::Program<'a>,
    source: &str,
    consts: &HashMap<String, &'a oxc_ast::ast::Expression<'a>>,
) -> Result<&'a oxc_ast::ast::Expression<'a>, String> {
    use oxc_ast::ast::Statement;

    for stmt in &program.body {
        match stmt {
            Statement::ExportDefaultDeclaration(export) => {
                if let Some(expr) = export.declaration.as_expression() {
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
                let span = export.span;
                let end = (span.end as usize)
                    .min(source.len())
                    .min(span.start as usize + 50);
                let preview = &source[span.start as usize..end];
                return Err(format!("unsupported default export kind: {preview}"));
            }
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

fn try_module_exports_assignment<'a>(
    expr: &'a oxc_ast::ast::Expression<'a>,
    consts: &HashMap<String, &'a oxc_ast::ast::Expression<'a>>,
) -> Option<&'a oxc_ast::ast::Expression<'a>> {
    use oxc_ast::ast::Expression;

    if let Expression::AssignmentExpression(assign) = expr {
        if let oxc_ast::ast::AssignmentTarget::StaticMemberExpression(member) = &assign.left {
            if let Expression::Identifier(obj) = &member.object {
                if obj.name == "module" && member.property.name == "exports" {
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

fn is_function_expr(expr: &oxc_ast::ast::Expression, _source: &str) -> bool {
    use oxc_ast::ast::Expression;
    matches!(
        expr,
        Expression::FunctionExpression(_) | Expression::ArrowFunctionExpression(_)
    )
}

fn unwrap_define_config<'a>(
    expr: &'a oxc_ast::ast::Expression<'a>,
    _source: &str,
) -> &'a oxc_ast::ast::Expression<'a> {
    use oxc_ast::ast::Expression;

    if let Expression::CallExpression(call) = expr {
        if let Expression::Identifier(callee) = &call.callee {
            if callee.name == "defineConfig" && call.arguments.len() == 1 {
                if let Some(arg_expr) = call.arguments[0].as_expression() {
                    return arg_expr;
                }
            }
        }
    }
    expr
}

fn find_resolve_alias<'a>(
    expr: &'a oxc_ast::ast::Expression<'a>,
    _source: &str,
) -> Result<&'a oxc_ast::ast::Expression<'a>, String> {
    use oxc_ast::ast::Expression;

    let obj = match expr {
        Expression::ObjectExpression(obj) => obj,
        _ => return Err("config is not an object expression".to_string()),
    };

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

fn extract_aliases_from_object(
    obj: &oxc_ast::ast::ObjectExpression,
    source: &str,
    ctx: &mut AnalysisContext,
) -> Result<Vec<(String, String)>, String> {
    let mut aliases = Vec::new();

    for prop in &obj.properties {
        match prop {
            oxc_ast::ast::ObjectPropertyKind::ObjectProperty(op) => {
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
                    _ => {}
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

fn eval_static_string(
    expr: &oxc_ast::ast::Expression,
    source: &str,
    ctx: &mut AnalysisContext,
) -> Result<String, String> {
    use oxc_ast::ast::Expression;
    use oxc_span::GetSpan;

    match expr {
        Expression::StringLiteral(lit) => Ok(lit.value.to_string()),

        Expression::TemplateLiteral(tmpl) => {
            if !tmpl.expressions.is_empty() {
                return Err("template literal with substitutions".to_string());
            }
            let quasi = tmpl.quasis.first().ok_or("empty template literal")?;
            Ok(quasi.value.raw.to_string())
        }

        Expression::CallExpression(call) => eval_static_call(call, source, ctx),

        Expression::NewExpression(new_expr) => eval_new_url(new_expr, source, ctx),

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

    let mut result = PathBuf::from(&segments[0]);
    for seg in &segments[1..] {
        result = result.join(seg);
    }

    Ok(result.to_string_lossy().replace('\\', "/"))
}

fn eval_new_url(
    new_expr: &oxc_ast::ast::NewExpression,
    source: &str,
    ctx: &mut AnalysisContext,
) -> Result<String, String> {
    use oxc_ast::ast::Expression;
    use oxc_span::GetSpan;

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

    let relative_path = match new_expr.arguments[0].as_expression() {
        Some(Expression::StringLiteral(lit)) => lit.value.to_string(),
        _ => {
            return Err("new URL first arg is not a string literal".to_string());
        }
    };

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

    let resolved = ctx.config_dir.join(&relative_path);
    Ok(resolved.to_string_lossy().replace('\\', "/"))
}

fn property_key_name(key: &oxc_ast::ast::PropertyKey) -> Option<String> {
    use oxc_ast::ast::PropertyKey;
    match key {
        PropertyKey::StaticIdentifier(id) => Some(id.name.to_string()),
        PropertyKey::StringLiteral(lit) => Some(lit.value.to_string()),
        _ => None,
    }
}

fn contains_await(expr: &oxc_ast::ast::Expression, _source: &str) -> bool {
    matches!(expr, oxc_ast::ast::Expression::AwaitExpression(_))
}

// ═══════════════════════════════════════════════════════════════════════════
// Trusted Execution (Node.js-based)
// ═══════════════════════════════════════════════════════════════════════════

/// Environment variables removed from child processes to avoid VS Code/Electron
/// interference with Node.js script execution.
const CHILD_PROCESS_ENV_DENYLIST: &[&str] = &[
    "NODE_OPTIONS",
    "VSCODE_INSPECTOR_OPTIONS",
    "ELECTRON_RUN_AS_NODE",
];

const VITE_SENTINEL_BEGIN: &str = "__VERTER_ALIASES_BEGIN__";
const VITE_SENTINEL_END: &str = "__VERTER_ALIASES_END__";

#[derive(serde::Deserialize)]
struct ViteAliasEntry {
    find: String,
    replacement: String,
}

/// Execute a vite config by spawning Node.js (requires explicit trust).
pub fn execute_trusted_vite_config(
    config_path: &Path,
    project_root: &Path,
    node_path: &str,
) -> Option<TrustedExecResult> {
    let config_path_str = config_path.to_string_lossy().replace('\\', "/");
    let config_dir = config_path.parent().unwrap_or(project_root);

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

    let mut cmd = std::process::Command::new(node_path);
    cmd.arg("-e")
        .arg(&script)
        .current_dir(project_root)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    for var in CHILD_PROCESS_ENV_DENYLIST {
        cmd.env_remove(var);
    }

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            tracing::debug!("failed to spawn node for trusted vite config: {e}");
            return None;
        }
    };

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

/// Discover vite aliases via trusted execution.
pub fn discover_vite_aliases(
    ws: &dyn crate::traits::WorkspaceAccess,
    project_root: &str,
    node_path: &str,
) -> Vec<(String, String)> {
    let config_path_str = match find_vite_config(ws, project_root) {
        Some(p) => p,
        None => return Vec::new(),
    };
    let config_path =
        PathBuf::from(config_path_str.replace('/', if cfg!(windows) { "\\" } else { "/" }));
    let root_path =
        PathBuf::from(project_root.replace('/', if cfg!(windows) { "\\" } else { "/" }));

    match execute_trusted_vite_config(&config_path, &root_path, node_path) {
        Some(result) => result.aliases,
        None => get_lkg(&config_path_str).unwrap_or_default(),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Last-Known-Good Cache
// ═══════════════════════════════════════════════════════════════════════════

type AliasList = Vec<(String, String)>;

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
#[path = "vite_config_tests.rs"]
mod tests;
