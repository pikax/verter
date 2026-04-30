//! Type constructs, helper imports, and the `@verter/types` ambient module
//! constants (D11 of Phase 11d ownership-domain analysis).

use oxc_ast::ast::{BindingPattern, Statement};
use oxc_ast::{Comment, CommentContent};
use rustc_hash::FxHashMap;

use crate::ide::IdeGenericInfo;
use crate::ide::IdeScriptOptions;
use crate::template::code_gen::types::CodeGenOutput;

use super::PREFIX;

/// Info about a binding's position and leading JSDoc.
pub(super) struct BindingSourceInfo {
    /// Leading JSDoc comment text (e.g. `/** My counter */`), if any.
    pub(super) jsdoc: Option<String>,
    /// SFC-absolute byte offset of identifier start.
    pub(super) sfc_start: u32,
    /// SFC-absolute byte offset of identifier end.
    pub(super) sfc_end: u32,
}

/// Find a leading JSDoc comment for a declaration at the given position.
///
/// OXC's `Comment.attached_to` is the byte offset of the token the comment precedes.
/// We match comments where `attached_to == target_start` and the comment is a JSDoc
/// block comment (starts with `/**`).
fn find_leading_jsdoc(
    comments: &[Comment],
    target_start: u32,
    content_str: &str,
) -> Option<String> {
    for comment in comments {
        if comment.attached_to == target_start
            && comment.is_block()
            && matches!(
                comment.content,
                CommentContent::Jsdoc | CommentContent::JsdocLegal
            )
        {
            let text = &content_str[comment.span.start as usize..comment.span.end as usize];
            return Some(text.to_string());
        }
    }
    None
}

/// Build a map of binding name → source info (JSDoc + SFC-absolute identifier span).
///
/// Walks OXC's parsed program body to find variable declarations and function declarations,
/// extracting identifier spans and any leading JSDoc comments.
pub(super) fn build_binding_source_info<'a>(
    body: &'a [Statement<'a>],
    comments: &[Comment],
    content_str: &str,
    content_start: u32,
) -> FxHashMap<&'a str, BindingSourceInfo> {
    let mut info: FxHashMap<&'a str, BindingSourceInfo> = FxHashMap::default();

    for stmt in body {
        match stmt {
            Statement::VariableDeclaration(decl) => {
                let decl_start = decl.span.start;
                for declarator in &decl.declarations {
                    if let BindingPattern::BindingIdentifier(id) = &declarator.id {
                        let jsdoc = find_leading_jsdoc(comments, decl_start, content_str);
                        info.insert(
                            id.name.as_str(),
                            BindingSourceInfo {
                                jsdoc,
                                sfc_start: content_start + id.span.start,
                                sfc_end: content_start + id.span.end,
                            },
                        );
                    }
                }
            }
            Statement::FunctionDeclaration(func) => {
                if let Some(id) = &func.id {
                    let jsdoc = find_leading_jsdoc(comments, func.span.start, content_str);
                    info.insert(
                        id.name.as_str(),
                        BindingSourceInfo {
                            jsdoc,
                            sfc_start: content_start + id.span.start,
                            sfc_end: content_start + id.span.end,
                        },
                    );
                }
            }
            _ => {}
        }
    }

    info
}

/// Ambient module declaration for `@verter/types`.
///
/// Appended to `type_constructs` so that every TSX file self-contains the module
/// declaration. TypeScript resolves ambient `declare module` from the same file,
/// making the `import ... from "@verter/types"` at the top resolvable without
/// installing the package or relying on TS plugin / TSGO overlay hacks.
///
/// Uses `import("vue").X` syntax because top-level imports are not allowed inside
/// `declare module` blocks.
///
/// See also [`VERTER_TYPES_STANDALONE_DTS`] for the unwrapped version (used by
/// the LSP to materialise `node_modules/@verter/types/index.d.ts` on disk).
pub const VERTER_TYPES_AMBIENT_MODULE: &str = r#"
declare module "@verter/types" {
  export type Prettify<T> = T extends { (...args: any[]): any } ? T : { [K in keyof T]: T[K] } & {};
  export declare function enhanceElementWithProps<T, P>(el: T, props: P): T & P;
  export declare function shallowUnwrapRef<T>(obj: T): import("vue").ShallowUnwrapRef<T>;
  export type ExtractRenderComponent<T> = T extends { new (): infer I; } ? I extends { $props: any } ? T : I extends HTMLElement ? (props: {}) => I : I : T extends (...args: any) => infer R ? void extends R ? typeof import("vue").Comment : R extends Array<any> ? typeof import("vue").Fragment : HTMLElement : T extends HTMLElement ? (props: {}) => T : T extends keyof import("vue").NativeElements ? (props: import("vue").NativeElements[T]) => JSX.Element : (props: {}) => JSX.Element;
  export declare function extractRenderComponent<T extends string>(t: T): ExtractRenderComponent<T>;
  export declare function extractRenderComponent<T>(t: T): ExtractRenderComponent<T>;
  export type ExtractComponentProps<T> = T extends { new (): infer I } ? ExtractComponentProps<I> : T extends { $props: infer P } ? P : T extends HTMLElement ? import("vue").HTMLAttributes : T extends (p: infer P) => any ? P : {};
  export declare function instantiateComponent<T, P>(comp: T, props: P): T extends { new (...args: any[]): infer I } ? I : T extends (...args: any[]) => infer R ? R : T;
  export declare function extractArgumentsFromRenderSlot<
    TSlots extends Record<string, any>,
    N extends keyof TSlots & string,
  >(
    component: { $slots: TSlots },
    slotName: N,
  ): TSlots[N] extends (...args: infer P) => any ? P[0] : never;
  export type ExtractLeafElement<T> = T extends HTMLElement ? T : T extends { $el: infer E } ? ExtractLeafElement<E> : T extends { new (): infer I } ? ExtractLeafElement<I> : never;
  export type ExtractDirectives<T> = { [K in keyof T as T[K] extends import("vue").Directive<any, any, any, any> ? K extends `v${Capitalize<string>}` ? K : never : never]: T[K]; };
  export declare function runCustomDirective<TInstance, TDirective extends import("vue").Directive<ExtractLeafElement<TInstance>>>(instance: TInstance, directive: TDirective): ExtractLeafElement<TInstance> extends infer El extends HTMLElement ? TDirective extends import("vue").Directive<infer TElement, infer TValue, infer M extends string> ? El extends TElement ? (instance: TInstance, value: TValue, arg: string | undefined, modifiers: { [K in M]?: true }) => void : (instance: TElement, value: TValue, arg: string | undefined, modifiers: { [K in M]?: true }) => void : false : false;
  export declare function retrieveSetupDirectives<T>(o: T): ExtractDirectives<T> extends infer D ? ExtractDirectives<Omit<import("vue").GlobalDirectives, keyof D>> & D : ExtractDirectives<import("vue").GlobalDirectives>;
  export type IsExactlyEqual<A, B> = (<T>() => T extends A ? 1 : 2) extends <T>() => T extends B ? 1 : 2 ? true : false;
  export declare function strictRenderSlot<T extends (...args: any[]) => any, U>(slot: T, child: ReturnType<T> extends infer R ? R extends Array<any> ? never : R extends string ? [R] : R extends U ? [U] : R : ReturnType<T>): any;
  export declare function strictRenderSlot<T extends (...args: any[]) => any, U>(slot: T, children: ReturnType<T> extends infer R ? R extends readonly [any, ...any[]] ? R : R extends Array<infer E> ? U extends Array<infer UE> ? [UE] extends [never] ? U : E extends string | number | boolean | symbol | bigint | null | undefined ? E extends UE ? U : never : UE extends E ? IsExactlyEqual<UE, E> extends true ? U : never : never : never : never : ReturnType<T>): any;
  export declare function checkRequiredSlots<T>(slots: T, provided: { [K in keyof T as undefined extends T[K] ? never : K]: true }): void;
  export declare function eventCallbacks<TArgs extends Array<any>, R extends ($event: TArgs[0]) => any>(event: TArgs, cb: R): R;
}
"#;

/// Standalone `@verter/types` type declarations as a `.d.ts` file.
///
/// This is the same content as [`VERTER_TYPES_AMBIENT_MODULE`] but without the
/// `declare module "@verter/types" { ... }` wrapper.  The LSP writes this to
/// `node_modules/@verter/types/index.d.ts` when the real package is not installed,
/// so that TSGO can resolve `import { ... } from "@verter/types"` via normal
/// `node_modules` resolution.
///
/// Uses `import("vue").X` syntax for Vue type references.
pub const VERTER_TYPES_STANDALONE_DTS: &str = r#"// Auto-generated by verter-lsp — do not edit.
// This file provides @verter/types declarations so that TSGO can resolve
// the imports emitted by Verter's TSX codegen.

export type Prettify<T> = T extends { (...args: any[]): any } ? T : { [K in keyof T]: T[K] } & {};
export declare function enhanceElementWithProps<T, P>(el: T, props: P): T & P;
export declare function shallowUnwrapRef<T>(obj: T): import("vue").ShallowUnwrapRef<T>;
export type ExtractRenderComponent<T> = T extends { new (): infer I; } ? I extends { $props: any } ? T : I extends HTMLElement ? (props: {}) => I : I : T extends (...args: any) => infer R ? void extends R ? typeof import("vue").Comment : R extends Array<any> ? typeof import("vue").Fragment : HTMLElement : T extends HTMLElement ? (props: {}) => T : T extends keyof import("vue").NativeElements ? (props: import("vue").NativeElements[T]) => JSX.Element : (props: {}) => JSX.Element;
export declare function extractRenderComponent<T extends string>(t: T): ExtractRenderComponent<T>;
export declare function extractRenderComponent<T>(t: T): ExtractRenderComponent<T>;
export type ExtractComponentProps<T> = T extends { new (): infer I } ? ExtractComponentProps<I> : T extends { $props: infer P } ? P : T extends HTMLElement ? import("vue").HTMLAttributes : T extends (p: infer P) => any ? P : {};
export declare function instantiateComponent<T, P>(comp: T, props: P): T extends { new (...args: any[]): infer I } ? I : T extends (...args: any[]) => infer R ? R : T;
export declare function extractArgumentsFromRenderSlot<
  TSlots extends Record<string, any>,
  N extends keyof TSlots & string,
>(
  component: { $slots: TSlots },
  slotName: N,
): TSlots[N] extends (...args: infer P) => any ? P[0] : never;
export type ExtractLeafElement<T> = T extends HTMLElement ? T : T extends { $el: infer E } ? ExtractLeafElement<E> : T extends { new (): infer I } ? ExtractLeafElement<I> : never;
export type ExtractDirectives<T> = { [K in keyof T as T[K] extends import("vue").Directive<any, any, any, any> ? K extends `v${Capitalize<string>}` ? K : never : never]: T[K]; };
export declare function runCustomDirective<TInstance, TDirective extends import("vue").Directive<ExtractLeafElement<TInstance>>>(instance: TInstance, directive: TDirective): ExtractLeafElement<TInstance> extends infer El extends HTMLElement ? TDirective extends import("vue").Directive<infer TElement, infer TValue, infer M extends string> ? El extends TElement ? (instance: TInstance, value: TValue, arg: string | undefined, modifiers: { [K in M]?: true }) => void : (instance: TElement, value: TValue, arg: string | undefined, modifiers: { [K in M]?: true }) => void : false : false;
export declare function retrieveSetupDirectives<T>(o: T): ExtractDirectives<T> extends infer D ? ExtractDirectives<Omit<import("vue").GlobalDirectives, keyof D>> & D : ExtractDirectives<import("vue").GlobalDirectives>;
export type IsExactlyEqual<A, B> = (<T>() => T extends A ? 1 : 2) extends <T>() => T extends B ? 1 : 2 ? true : false;
export declare function strictRenderSlot<T extends (...args: any[]) => any, U>(slot: T, child: ReturnType<T> extends infer R ? R extends Array<any> ? never : R extends string ? [R] : R extends U ? [U] : R : ReturnType<T>): any;
export declare function strictRenderSlot<T extends (...args: any[]) => any, U>(slot: T, children: ReturnType<T> extends infer R ? R extends readonly [any, ...any[]] ? R : R extends Array<infer E> ? U extends Array<infer UE> ? [UE] extends [never] ? U : E extends string | number | boolean | symbol | bigint | null | undefined ? E extends UE ? U : never : UE extends E ? IsExactlyEqual<UE, E> extends true ? U : never : never : never : never : ReturnType<T>): any;
export declare function checkRequiredSlots<T>(slots: T, provided: { [K in keyof T as undefined extends T[K] ? never : K]: true }): void;
export declare function eventCallbacks<TArgs extends Array<any>, R extends ($event: TArgs[0]) => any>(event: TArgs, cb: R): R;
"#;

/// Collect Vue built-in component names used in the template AST.
///
/// Walks the flat arena looking for elements with `TagType::Component` whose
/// tag matches a Vue built-in (Suspense, Teleport, KeepAlive, Transition, TransitionGroup).
/// Returns the user-facing Vue export names (e.g., `"Suspense"`, `"KeepAlive"`).
pub(super) fn collect_builtin_components(
    template_ast: Option<&crate::ast::types::TemplateAst>,
    source: &str,
) -> Vec<&'static str> {
    use crate::template::code_gen::shared::helpers::is_builtin_component;

    let ast = match template_ast {
        Some(a) => a,
        None => return Vec::new(),
    };

    let mut seen = 0u8; // bitmask to deduplicate
    let mut result = Vec::new();

    for node in &ast.nodes {
        if let crate::ast::types::AstNodeKind::Element(ref el) = node.kind {
            if !el.tag_type.is_component() {
                continue;
            }
            let tag_name = &source[(el.tag_open.start + 1) as usize..el.tag_open.name_end as usize];
            if let Some((flag_bit, _helper_name)) = is_builtin_component(tag_name) {
                if seen & flag_bit == 0 {
                    seen |= flag_bit;
                    // Map to the user-facing Vue export name (PascalCase)
                    let vue_name = match tag_name {
                        "Teleport" | "teleport" => "Teleport",
                        "Suspense" | "suspense" => "Suspense",
                        "KeepAlive" | "keep-alive" => "KeepAlive",
                        "BaseTransition" | "base-transition" => "BaseTransition",
                        "Transition" | "transition" => "Transition",
                        "TransitionGroup" | "transition-group" => "TransitionGroup",
                        _ => continue,
                    };
                    result.push(vue_name);
                }
            }
        }
    }
    result
}

/// Emit helper imports hoisted before the wrapper function.
pub(super) fn emit_helper_imports(
    out: &mut CodeGenOutput<'_>,
    pos: u32,
    options: &IdeScriptOptions<'_>,
    builtin_components: &[&str],
    template_ast: Option<&crate::ast::types::TemplateAst>,
) {
    emit_helper_imports_inner(out, pos, options, builtin_components, template_ast, false);
}

pub(super) fn emit_helper_imports_with_define_component(
    out: &mut CodeGenOutput<'_>,
    pos: u32,
    options: &IdeScriptOptions<'_>,
    builtin_components: &[&str],
    template_ast: Option<&crate::ast::types::TemplateAst>,
) {
    emit_helper_imports_inner(out, pos, options, builtin_components, template_ast, true);
}

fn emit_helper_imports_inner(
    out: &mut CodeGenOutput<'_>,
    pos: u32,
    options: &IdeScriptOptions<'_>,
    builtin_components: &[&str],
    template_ast: Option<&crate::ast::types::TemplateAst>,
    needs_define_component: bool,
) {
    use std::fmt::Write;

    let mut imports = String::with_capacity(512);

    // Type imports from @verter/types (TS mode only — JSDoc mode doesn't need import type)
    if !options.is_jsx {
        if template_ast.is_some() {
            writeln!(
                imports,
                "import type {{ Prettify as {P}Prettify, ExtractComponentProps as {P}ExtractComponentProps, ExtractLeafElement as {P}ExtractLeafElement }} from \"{}\";",
                options.types_module_name,
                P = PREFIX,
            )
            .expect("write to String is infallible");
        } else {
            writeln!(
                imports,
                "import type {{ Prettify as {P}Prettify }} from \"{}\";",
                options.types_module_name,
                P = PREFIX,
            )
            .expect("write to String is infallible");
        }
    }

    // Runtime imports from @verter/types
    writeln!(
        imports,
        "import {{ shallowUnwrapRef as {P}shallowUnwrapRef, enhanceElementWithProps as {P}enhanceElementWithProps, extractRenderComponent as {P}extractRenderComponent, instantiateComponent as {P}instantiateComponent, extractArgumentsFromRenderSlot as {P}extractArgumentsFromRenderSlot, runCustomDirective as {P}runCustomDirective, retrieveSetupDirectives as {P}retrieveSetupDirectives, strictRenderSlot as {P}strictRenderSlot, checkRequiredSlots as {P}checkRequiredSlots, eventCallbacks as {P}eventCallbacks }} from \"{}\";",
        options.types_module_name,
        P = PREFIX,
    )
    .expect("write to String is infallible");

    // Collect vue imports: built-in components + template helpers (normalizeClass, normalizeStyle)
    let mut vue_imports: Vec<&str> = Vec::new();
    if needs_define_component {
        vue_imports.push("defineComponent as ___VERTER___defineComponent");
    }
    for &name in builtin_components {
        vue_imports.push(name);
    }

    // Check if template needs normalizeClass/normalizeStyle for class/style merging
    if let Some(ast) = template_ast {
        let mut need_class = false;
        let mut need_style = false;
        for node in &ast.nodes {
            if let crate::ast::types::AstNodeKind::Element(ref el) = node.kind {
                if !need_class && el.needs_class_merge() {
                    need_class = true;
                }
                if !need_style && el.needs_style_merge() {
                    need_style = true;
                }
                if need_class && need_style {
                    break;
                }
            }
        }
        if need_class {
            vue_imports.push("normalizeClass as ___VERTER___normalizeClass");
        }
        if need_style {
            vue_imports.push("normalizeStyle as ___VERTER___normalizeStyle");
        }
    }

    if !vue_imports.is_empty() {
        let imports_str = vue_imports.join(", ");
        writeln!(imports, "import {{ {} }} from \"vue\";", imports_str)
            .expect("write to String is infallible");
    }

    out.prepend_alloc(pos, &imports);
}

/// Emit all type constructs to the `buf` string (no sourcemap).
///
/// `emit_attributes_type`: when false, skip the `___VERTER___attributes` type alias.
/// Template-only SFCs have no Comp functions that reference it, so emitting it
/// produces TS6196 "declared but never used".
pub(super) fn emit_type_constructs(
    buf: &mut String,
    generic_info: &Option<IdeGenericInfo>,
    attrs_type: &Option<String>,
    _source: &str,
    options: &IdeScriptOptions<'_>,
    has_get_current_instance: bool,
    emit_attributes_type: bool,
) {
    // Emit getCurrentInstance return type override (#11)
    if has_get_current_instance {
        if options.is_jsx {
            buf.push_str(
                "\n/** @type {function(): import('vue').ComponentInternalInstance | null} */\nvar getCurrentInstance = /** @type {any} */ (undefined);\n",
            );
        } else {
            buf.push_str(
                "\ntype ___VERTER___ComponentInstance = import('vue').ComponentInternalInstance;\ndeclare function getCurrentInstance(): ___VERTER___ComponentInstance | null;\n",
            );
        }
    }

    // Emit ___VERTER___attributes type alias
    if !emit_attributes_type {
        // Skip — caller knows this type won't be referenced
    } else if options.is_jsx {
        // JS mode: JSDoc @typedef
        if let Some(ref attrs) = attrs_type {
            buf.push_str(&format!(
                "\n/** @typedef {{{}}} {P}attributes */\n",
                attrs,
                P = PREFIX,
            ));
        } else {
            buf.push_str(&format!(
                "\n/** @typedef {{{{}}}} {P}attributes */\n",
                P = PREFIX,
            ));
        }
    } else {
        // TS mode: type alias
        let generic_suffix = generic_info
            .as_ref()
            .map(|g| g.source_bracket())
            .unwrap_or_default();
        if let Some(ref attrs) = attrs_type {
            buf.push_str(&format!(
                "\ntype {P}attributes{gs} = {attrs};\n",
                P = PREFIX,
                gs = generic_suffix,
                attrs = attrs,
            ));
        } else {
            buf.push_str(&format!("\ntype {P}attributes = {{}};\n", P = PREFIX,));
        }
    }

    // Append ambient module declaration (TS mode only — declare module is TS syntax)
    if options.embed_ambient_types && !options.is_jsx {
        buf.push_str(VERTER_TYPES_AMBIENT_MODULE);
    }
}

/// Emit RootElement, RootElementProps, and Attrs type aliases inside the function scope.
///
/// These must be inside `templateBindingFN` because they reference `getRootComponent`
/// and `getRootComponentPassedProps` which are function-local.
pub(super) fn emit_attrs_type_aliases(
    buf: &mut String,
    generic_info: &Option<IdeGenericInfo>,
    inherit_attrs: bool,
) {
    let gs = generic_info
        .as_ref()
        .map(|g| g.source_bracket())
        .unwrap_or_default();
    let gn = generic_info
        .as_ref()
        .map(|g| g.names_bracket())
        .unwrap_or_default();

    buf.push_str(&format!(
        "\ntype {P}RootElement{gs} = ReturnType<typeof {P}getRootComponent{gn}>;\
         \ntype {P}RootElementProps{gs} = {P}Prettify<Omit<\
         \n  {P}ExtractComponentProps<{P}RootElement{gn}>,\
         \n  keyof ReturnType<typeof {P}getRootComponentPassedProps{gn}>\
         \n>>;\n",
        P = PREFIX,
        gs = gs,
        gn = gn,
    ));

    if inherit_attrs {
        buf.push_str(&format!(
            "\ntype {P}Attrs{gs} = {P}attributes{gn} & {P}RootElementProps{gn};\n",
            P = PREFIX,
            gs = gs,
            gn = gn,
        ));
    } else {
        buf.push_str(&format!(
            "\ntype {P}Attrs{gs} = {P}attributes{gn};\n",
            P = PREFIX,
            gs = gs,
            gn = gn,
        ));
    }
}
