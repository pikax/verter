use std::sync::Arc;

use super::*;

#[derive(Debug, Clone, PartialEq, Unshared)]
enum Tree {
    Leaf(Arc<str>),
    Node {
        children: Arc<[Tree]>,
        label: Option<Arc<Tree>>,
        name: OwnedStr,
        #[unshared(clone)]
        weight: u32,
    },
}

/// The raw allocations a tree reaches, by address.
fn raw_addresses(tree: &Tree, out: &mut Vec<usize>) {
    match tree {
        Tree::Leaf(text) => out.push(Arc::as_ptr(text).cast::<()>() as usize),
        Tree::Node {
            children, label, ..
        } => {
            out.push(Arc::as_ptr(children).cast::<()>() as usize);
            for child in children.iter() {
                raw_addresses(child, out);
            }
            if let Some(label) = label {
                out.push(Arc::as_ptr(label).cast::<()>() as usize);
                raw_addresses(label, out);
            }
        }
    }
}

/// An unshared copy equals its source and reaches none of its raw
/// allocations, while a self-charged handle in it is shared.
#[test]
fn an_unshared_copy_equals_its_source_and_aliases_no_raw_allocation() {
    let shared_leaf = Tree::Leaf(Arc::from("leaf"));
    let name = OwnedStr::new("node");
    let source = Tree::Node {
        children: Arc::from([shared_leaf.clone(), shared_leaf]),
        label: Some(Arc::new(Tree::Leaf(Arc::from("label")))),
        name: name.clone(),
        weight: 7,
    };
    let copy = source.unshared();
    assert_eq!(copy, source);

    let mut from_source = Vec::new();
    raw_addresses(&source, &mut from_source);
    let mut from_copy = Vec::new();
    raw_addresses(&copy, &mut from_copy);
    assert!(
        from_copy
            .iter()
            .all(|address| !from_source.contains(address)),
        "the copy aliases none of its source's raw allocations"
    );
    let Tree::Node { name: copied, .. } = &copy else {
        unreachable!()
    };
    assert!(
        OwnedStr::ptr_eq(copied, &name),
        "an owned handle is shared: its charge follows it"
    );
}
