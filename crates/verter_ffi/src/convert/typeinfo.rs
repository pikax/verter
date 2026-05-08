//! FFI ↔ host conversions for the typeinfo substrate.
//!
//! Mirrors the structure of `crate::convert::component_meta`: each FFI
//! type from `verter_protocol::typeinfo` lowers to its host counterpart
//! in `verter_session::typeinfo::types` (and vice-versa). NAPI / WASM
//! adapters use these helpers to keep boundary parsing in one place.

use verter_protocol::typeinfo::{
    FfiEvaluateTypeExpressionRequest, FfiImportSpec, FfiNamedImport, FfiSymbolEntry, MODE_EXPANDED,
    MODE_IDENTITY, MODE_NAVIGATE, MODE_SHALLOW, MODE_SKELETON,
};
use verter_session::semantic_query::ProjectionMode;
use verter_session::typeinfo::types::{
    EvaluateTypeExpressionRequest, ImportSpec, NamedImport, SymbolEntry, SymbolKind,
};

use super::error::FfiConversionError;

// ---------------------------------------------------------------------------
// FFI → host: EvaluateTypeExpressionRequest
// ---------------------------------------------------------------------------

/// Lower an FFI evaluate-request DTO into the host substrate type.
///
/// `mode` lowers via [`parse_projection_mode`]; named-import variants
/// lower via [`parse_named_import`]. Errors carry the offending token so
/// adapters can surface a clear failure to the JS caller.
pub fn ffi_to_host_evaluate_request(
    req: FfiEvaluateTypeExpressionRequest,
) -> Result<EvaluateTypeExpressionRequest, FfiConversionError> {
    let mode = parse_projection_mode(&req.mode)?;
    let mut extra_imports: Vec<ImportSpec> = Vec::with_capacity(req.extra_imports.len());
    for ffi_spec in req.extra_imports {
        extra_imports.push(ffi_to_host_import_spec(ffi_spec)?);
    }
    Ok(EvaluateTypeExpressionRequest {
        scope: req.scope,
        expression: req.expression,
        extra_imports,
        mode,
        cacheable: req.cacheable,
    })
}

fn ffi_to_host_import_spec(spec: FfiImportSpec) -> Result<ImportSpec, FfiConversionError> {
    let mut bindings: Vec<NamedImport> = Vec::with_capacity(spec.bindings.len());
    for ffi_binding in spec.bindings {
        bindings.push(parse_named_import(ffi_binding)?);
    }
    Ok(ImportSpec {
        specifier: spec.specifier,
        bindings,
    })
}

fn parse_named_import(ni: FfiNamedImport) -> Result<NamedImport, FfiConversionError> {
    match ni.kind.as_str() {
        "default" => Ok(NamedImport::Default {
            local_name: ni.local_name,
        }),
        "named" => {
            let local_alias = if ni.local_alias.is_empty() {
                None
            } else {
                Some(ni.local_alias)
            };
            Ok(NamedImport::Named {
                exported_name: ni.exported_name,
                local_alias,
                type_only: ni.type_only,
            })
        }
        "namespace" => Ok(NamedImport::Namespace {
            local_name: ni.local_name,
        }),
        other => Err(FfiConversionError::InvalidNamedImportKind(
            other.to_string(),
        )),
    }
}

// ---------------------------------------------------------------------------
// FFI ↔ host: ProjectionMode
// ---------------------------------------------------------------------------

/// Lower a stringly-tagged FFI projection-mode value to its host enum.
///
/// Accepts the canonical lower-case tags `"identity" | "navigate" |
/// "shallow" | "expanded" | "skeleton"`. Unknown tags surface as
/// [`FfiConversionError::InvalidProjectionMode`].
pub fn parse_projection_mode(tag: &str) -> Result<ProjectionMode, FfiConversionError> {
    match tag {
        MODE_IDENTITY => Ok(ProjectionMode::Identity),
        MODE_NAVIGATE => Ok(ProjectionMode::Navigate),
        MODE_SHALLOW => Ok(ProjectionMode::Shallow),
        MODE_EXPANDED => Ok(ProjectionMode::Expanded),
        MODE_SKELETON => Ok(ProjectionMode::Skeleton),
        other => Err(FfiConversionError::InvalidProjectionMode(other.to_string())),
    }
}

/// Inverse of [`parse_projection_mode`] — render a host
/// `ProjectionMode` to its canonical FFI tag.
pub fn projection_mode_tag(mode: ProjectionMode) -> &'static str {
    match mode {
        ProjectionMode::Identity => MODE_IDENTITY,
        ProjectionMode::Navigate => MODE_NAVIGATE,
        ProjectionMode::Shallow => MODE_SHALLOW,
        ProjectionMode::Expanded => MODE_EXPANDED,
        ProjectionMode::Skeleton => MODE_SKELETON,
    }
}

// ---------------------------------------------------------------------------
// host → FFI: SymbolEntry
// ---------------------------------------------------------------------------

/// Project a host [`SymbolEntry`] to its FFI mirror so consumers can
/// JSON-encode the inventory at the boundary.
pub fn host_to_ffi_symbol_entry(entry: SymbolEntry) -> FfiSymbolEntry {
    let kind = symbol_kind_tag(entry.kind);
    let (span_start, span_end, has_span) = match entry.span {
        Some(span) => (span.start, span.end, true),
        None => (0, 0, false),
    };
    FfiSymbolEntry {
        name: entry.name,
        kind: kind.to_string(),
        span_start,
        span_end,
        has_span,
        is_exported: entry.is_exported,
    }
}

/// Inverse of [`host_to_ffi_symbol_entry`] for round-trip tests and
/// downstream decoders that want to reconstruct the host enum.
pub fn parse_symbol_kind(tag: &str) -> Option<SymbolKind> {
    match tag {
        "typeAlias" => Some(SymbolKind::TypeAlias),
        "interface" => Some(SymbolKind::Interface),
        "class" => Some(SymbolKind::Class),
        "const" => Some(SymbolKind::Const),
        "let" => Some(SymbolKind::Let),
        "var" => Some(SymbolKind::Var),
        "function" => Some(SymbolKind::Function),
        "asyncFunction" => Some(SymbolKind::AsyncFunction),
        "classValue" => Some(SymbolKind::ClassValue),
        "enum" => Some(SymbolKind::Enum),
        _ => None,
    }
}

fn symbol_kind_tag(kind: SymbolKind) -> &'static str {
    match kind {
        SymbolKind::TypeAlias => "typeAlias",
        SymbolKind::Interface => "interface",
        SymbolKind::Class => "class",
        SymbolKind::Const => "const",
        SymbolKind::Let => "let",
        SymbolKind::Var => "var",
        SymbolKind::Function => "function",
        SymbolKind::AsyncFunction => "asyncFunction",
        SymbolKind::ClassValue => "classValue",
        SymbolKind::Enum => "enum",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projection_mode_round_trips() {
        for mode in [
            ProjectionMode::Identity,
            ProjectionMode::Navigate,
            ProjectionMode::Shallow,
            ProjectionMode::Expanded,
            ProjectionMode::Skeleton,
        ] {
            let tag = projection_mode_tag(mode);
            assert_eq!(parse_projection_mode(tag).unwrap(), mode);
        }
    }

    #[test]
    fn parse_projection_mode_rejects_unknown() {
        let err = parse_projection_mode("WhatIsThis").unwrap_err();
        assert!(matches!(err, FfiConversionError::InvalidProjectionMode(s) if s == "WhatIsThis"));
    }

    #[test]
    fn parse_named_import_default() {
        let ffi = FfiNamedImport {
            kind: "default".to_string(),
            local_name: "foo".to_string(),
            exported_name: String::new(),
            local_alias: String::new(),
            type_only: false,
        };
        assert_eq!(
            parse_named_import(ffi).unwrap(),
            NamedImport::Default {
                local_name: "foo".to_string()
            }
        );
    }

    #[test]
    fn parse_named_import_named_with_alias_and_type_only() {
        let ffi = FfiNamedImport {
            kind: "named".to_string(),
            local_name: String::new(),
            exported_name: "X".to_string(),
            local_alias: "Y".to_string(),
            type_only: true,
        };
        assert_eq!(
            parse_named_import(ffi).unwrap(),
            NamedImport::Named {
                exported_name: "X".to_string(),
                local_alias: Some("Y".to_string()),
                type_only: true,
            }
        );
    }

    #[test]
    fn parse_named_import_named_empty_alias_means_none() {
        let ffi = FfiNamedImport {
            kind: "named".to_string(),
            local_name: String::new(),
            exported_name: "X".to_string(),
            local_alias: String::new(),
            type_only: false,
        };
        assert_eq!(
            parse_named_import(ffi).unwrap(),
            NamedImport::Named {
                exported_name: "X".to_string(),
                local_alias: None,
                type_only: false,
            }
        );
    }

    #[test]
    fn parse_named_import_namespace() {
        let ffi = FfiNamedImport {
            kind: "namespace".to_string(),
            local_name: "Ns".to_string(),
            exported_name: String::new(),
            local_alias: String::new(),
            type_only: false,
        };
        assert_eq!(
            parse_named_import(ffi).unwrap(),
            NamedImport::Namespace {
                local_name: "Ns".to_string()
            }
        );
    }

    #[test]
    fn parse_named_import_invalid_kind() {
        let ffi = FfiNamedImport {
            kind: "BogusKind".to_string(),
            local_name: String::new(),
            exported_name: String::new(),
            local_alias: String::new(),
            type_only: false,
        };
        let err = parse_named_import(ffi).unwrap_err();
        assert!(matches!(err, FfiConversionError::InvalidNamedImportKind(s) if s == "BogusKind"));
    }

    #[test]
    fn host_to_ffi_symbol_entry_with_span() {
        let entry = SymbolEntry {
            name: "Foo".to_string(),
            kind: SymbolKind::TypeAlias,
            span: Some(verter_span::Span::new(10, 20)),
            is_exported: true,
        };
        let ffi = host_to_ffi_symbol_entry(entry);
        assert_eq!(ffi.name, "Foo");
        assert_eq!(ffi.kind, "typeAlias");
        assert_eq!(ffi.span_start, 10);
        assert_eq!(ffi.span_end, 20);
        assert!(ffi.has_span);
        assert!(ffi.is_exported);
    }

    #[test]
    fn host_to_ffi_symbol_entry_without_span() {
        let entry = SymbolEntry {
            name: "Bar".to_string(),
            kind: SymbolKind::Function,
            span: None,
            is_exported: false,
        };
        let ffi = host_to_ffi_symbol_entry(entry);
        assert_eq!(ffi.name, "Bar");
        assert_eq!(ffi.kind, "function");
        assert!(!ffi.has_span);
        assert_eq!(ffi.span_start, 0);
        assert_eq!(ffi.span_end, 0);
    }

    #[test]
    fn parse_symbol_kind_round_trip() {
        for kind in [
            SymbolKind::TypeAlias,
            SymbolKind::Interface,
            SymbolKind::Class,
            SymbolKind::Const,
            SymbolKind::Let,
            SymbolKind::Var,
            SymbolKind::Function,
            SymbolKind::AsyncFunction,
            SymbolKind::ClassValue,
            SymbolKind::Enum,
        ] {
            let tag = symbol_kind_tag(kind);
            assert_eq!(parse_symbol_kind(tag).unwrap(), kind);
        }
    }

    #[test]
    fn evaluate_request_round_trip_full() {
        let ffi = FfiEvaluateTypeExpressionRequest {
            scope: "/main.ts".to_string(),
            expression: "Pick<Foo, 'a'>".to_string(),
            extra_imports: vec![FfiImportSpec {
                specifier: "./types".to_string(),
                bindings: vec![FfiNamedImport {
                    kind: "named".to_string(),
                    local_name: String::new(),
                    exported_name: "X".to_string(),
                    local_alias: "Y".to_string(),
                    type_only: true,
                }],
            }],
            mode: "expanded".to_string(),
            cacheable: true,
        };
        let host = ffi_to_host_evaluate_request(ffi).unwrap();
        assert_eq!(host.scope, "/main.ts");
        assert_eq!(host.expression, "Pick<Foo, 'a'>");
        assert_eq!(host.extra_imports.len(), 1);
        assert_eq!(host.extra_imports[0].specifier, "./types");
        assert_eq!(host.mode, ProjectionMode::Expanded);
        assert!(host.cacheable);
    }
}
