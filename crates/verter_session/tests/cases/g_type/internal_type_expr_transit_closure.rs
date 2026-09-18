//! Internal general TypeExpr transit closure.
//!
//! Vue `defineOptions` / `defineExpose` named-member values must classify in
//! the node-domain raised-shape fold (`node_shallow_member_output_with_dispatch`
//! → `NamedTypeMemberOutput::from_raised_shallow`). Raising a `TypeExpr` and
//! classifying it (`classify_shallow`) is the displaced internal TypeExpr
//! route between dispatch raise and the shallow vocabulary.

use std::path::PathBuf;

fn session_src(rel: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("{} must be readable: {err}", path.display()))
}

fn slice_fn_body<'a>(src: &'a str, sig: &str) -> &'a str {
    let start = src.find(sig).unwrap_or_else(|| panic!("missing `{sig}`"));
    let rest = &src[start..];
    let brace = rest
        .find('{')
        .unwrap_or_else(|| panic!("`{sig}` has no body"));
    let body = &rest[brace..];
    let mut depth = 0usize;
    for (i, c) in body.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return &body[..=i];
                }
            }
            _ => {}
        }
    }
    panic!("`{sig}` body unclosed");
}

#[test]
fn vue_named_members_classify_in_node_domain_without_type_expr_transit() {
    let normalize = session_src("typeinfo/framework_surface/vue_exec/normalize.rs");
    let object_members = slice_fn_body(&normalize, "fn object_members_from_typeinfo_surface");
    assert!(
        object_members.contains("node_shallow_member_output_with_dispatch"),
        "object_members_from_typeinfo_surface must classify via \
         node_shallow_member_output_with_dispatch"
    );
    assert!(
        !object_members.contains("raise_member_value"),
        "object_members_from_typeinfo_surface must not raise a TypeExpr to classify named members"
    );
    assert!(
        !object_members.contains("classify_shallow"),
        "object_members_from_typeinfo_surface must not classify a raised TypeExpr"
    );

    let results = session_src("typeinfo/framework_surface/results.rs");
    assert!(
        !results.contains("fn classify_shallow("),
        "NamedTypeMemberOutput::classify_shallow is the displaced TypeExpr transit route"
    );
}
