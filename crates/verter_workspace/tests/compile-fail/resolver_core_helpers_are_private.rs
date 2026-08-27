//! A crate outside `verter_semantic` cannot name its crate-private resolver
//! helpers. `ModuleResolverCore` is the public resolution entry.

use verter_semantic::resolver_core::ModuleResolverCore;
use verter_semantic::resolver_core::node_modules_resolution::resolve_node_modules_package;
use verter_semantic::resolver_core::node_modules_resolution::resolve_node_modules_package_from_dir;
use verter_semantic::resolver_core::node_modules_resolution::resolve_node_modules_package_from_dirs;
use verter_semantic::resolver_core::package_target_resolution::resolve_legacy_package;
use verter_semantic::resolver_core::package_target_resolution::resolve_manifest_types_entry;
use verter_semantic::resolver_core::package_target_resolution::resolve_package_exports;
use verter_semantic::resolver_core::package_target_resolution::resolve_package_path;
use verter_semantic::resolver_core::package_target_resolution::resolve_package_target;

fn assert_public_entry<T>() {}

fn main() {
    assert_public_entry::<ModuleResolverCore>();
}
