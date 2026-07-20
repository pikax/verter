//! Shallow declaration and route publication from one retained OXC program.
//!
//! The product is deliberately closed: declaration names and locators come
//! from semantic headers, while parser syntax contributes authored routes.
//! Declaration bodies, dependency projection, raw surfaces, and resolution
//! are demand-time responsibilities and cannot enter this index.

use oxc_ast::ast::Program;
use verter_parser::utils::oxc::script::route_inventory::{
    build_script_route_inventory_with_owner_iter, RouteOwnerTableError, ScriptRouteInventory,
};

use super::decl_headers::{build_decl_header_index_with_owners, DeclHeaderIndex};
use super::TopLevelOwnerTable;

#[derive(Debug, Clone)]
pub struct ScriptShallowIndex {
    pub declaration_headers: DeclHeaderIndex,
    pub routes: ScriptRouteInventory,
}

/// Publish shallow declarations and authored routes for an ordinary source
/// file from one already-parsed program.
#[must_use]
pub fn build_script_shallow_index(program: &Program<'_>, source: &str) -> ScriptShallowIndex {
    let owners = TopLevelOwnerTable::ordinary_file(program.body.len());
    build_script_shallow_index_with_owners(program, source, &owners)
        .expect("ordinary owner table exactly covers Program.body")
}

/// Publish shallow declarations and authored routes under one validated owner
/// table. A length mismatch fails before either index is built.
pub fn build_script_shallow_index_with_owners(
    program: &Program<'_>,
    source: &str,
    owners: &TopLevelOwnerTable,
) -> Result<ScriptShallowIndex, RouteOwnerTableError> {
    let routes = build_script_route_inventory_with_owner_iter(
        program,
        owners.statements().iter().map(|statement| statement.owner),
    )?;
    let declaration_headers = build_decl_header_index_with_owners(program, source, owners);
    Ok(ScriptShallowIndex {
        declaration_headers,
        routes,
    })
}
