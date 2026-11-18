use super::*;
use std::mem::size_of;
use verter_span::Span;

enum VueNode {}
type Branch = u8;
enum Attr {}

// Dense identity must not depend on source length or equal authored positions.
#[test]
fn dense_identity_and_exact_source_index() {
    let mut tree = Structure::<VueNode, Branch, u32>::new("aéz");
    let region = tree.add_region(None, None, 7).unwrap();
    let root = tree.push(None, region, Span::new(0, 4), 1).unwrap();
    let first = tree.push(Some(root), region, Span::new(1, 3), 2).unwrap();
    let second = tree.push(Some(root), region, Span::new(1, 3), 3).unwrap();
    assert_eq!((root.index(), first.index(), second.index()), (0, 1, 2));
    assert_eq!(tree.source_text(first), Some("é"));
    assert_eq!(
        tree.push(Some(root), region, Span::new(2, 3), 0),
        Err(StructureError::InvalidSpan)
    );
    let tree = tree.finish();
    assert_eq!(tree.at_start(1).collect::<Vec<_>>(), [first, second]);
    assert_eq!(tree.at_start(2).count(), 0);
    assert_eq!(tree.node(second), Some(&3));
    assert_eq!(tree.span(root), Some(Span::new(0, 4)));
}

// Closed subtrees cannot be reopened, and branch regions must keep one owner.
#[test]
fn topology_and_region_ownership() {
    let mut tree = Structure::<VueNode, Branch, u8>::new("abc");
    let body = tree.add_region(None, None, 0).unwrap();
    let root = tree.push(None, body, Span::new(0, 3), 0).unwrap();
    let branch = tree.add_region(Some(body), Some(root), 1).unwrap();
    let a = tree.push(Some(root), branch, Span::new(0, 1), 1).unwrap();
    let b = tree.push(Some(a), branch, Span::new(1, 2), 2).unwrap();
    let c = tree.push(Some(root), body, Span::new(2, 3), 3).unwrap();
    assert_eq!(
        tree.push(Some(a), branch, Span::new(0, 1), 4),
        Err(StructureError::ClosedParent)
    );
    assert_eq!(
        tree.push(Some(c), branch, Span::new(2, 3), 4),
        Err(StructureError::RegionOwnership)
    );
    let tree = tree.finish();
    assert_eq!(tree.children(root).collect::<Vec<_>>(), [a, c]);
    assert_eq!(tree.parent(b), Some(a));
    assert_eq!(tree.subtree(a).unwrap().collect::<Vec<_>>(), [a, b]);
    assert_eq!(tree.region(a), Some(branch));
    assert_eq!(tree.region_data(branch), Some(&1));
}

// Flat ranges survive later appends; interning allocates only distinct values.
#[test]
fn flat_ranges_interning_and_layout() {
    let mut attrs = FlatArena::<Attr, u32, Frontend>::new();
    let first = attrs.append([10, 20]).unwrap();
    let empty = attrs.append([]).unwrap();
    attrs.append([30]).unwrap();
    assert_eq!(attrs.get(first), &[10, 20]);
    assert!(attrs.get(empty).is_empty());
    let mut text = TextInterner::<CompilerScratch>::new();
    let a = text.intern("decoded").unwrap();
    assert_eq!(text.intern("decoded"), Ok(a));
    assert_eq!(text.resolve(a), Some("decoded"));
    assert_eq!(text.len(), 1);
    assert_eq!(size_of::<NodeId<VueNode>>(), 4);
    assert_eq!(size_of::<Option<NodeId<VueNode>>>(), 4);
    assert_eq!(size_of::<FlatRange<Attr>>(), 8);
    assert_eq!(size_of::<Topology<VueNode>>(), 20);
}

// Rare facts must not reserve one slot per node, or lose deterministic order.
#[test]
fn sparse_facts_and_capacity_boundary() {
    let mut nodes = Arena::<VueNode, u8, Frontend>::new();
    let a = nodes.push(1).unwrap();
    let b = nodes.push(2).unwrap();
    let mut facts = SparseFacts::<VueNode, &str, Semantic>::new();
    facts.insert(b, "b");
    facts.insert(a, "a");
    assert_eq!(facts.insert(b, "updated"), Some("b"));
    assert_eq!(facts.get(b), Some(&"updated"));
    assert_eq!(facts.iter().map(|(id, _)| id).collect::<Vec<_>>(), [a, b]);
    assert_eq!(facts.len(), 2);
    assert_eq!(
        Id::<VueNode>::at(u32::MAX as usize),
        Err(StructureError::Capacity)
    );
}

// One owner can nest a region inside another of its own regions (a branch
// around a loop body); the inner region still answers to the same owner.
#[test]
fn two_regions_nest_under_one_owner() {
    let mut tree = Structure::<VueNode, Branch, u8>::new("abc");
    let root_region = tree.add_region(None, None, 0).unwrap();
    let p = tree.push(None, root_region, Span::new(0, 3), 0).unwrap();
    let outer = tree.add_region(Some(root_region), Some(p), 1).unwrap();
    let inner = tree.add_region(Some(outer), Some(p), 2).unwrap();
    let c = tree.push(Some(p), inner, Span::new(1, 2), 1).unwrap();
    let tree = tree.finish();
    assert_eq!(tree.region(c), Some(inner));
    assert_eq!(tree.region_parent(inner), Some(outer));
    assert_eq!(tree.region_owner(inner), Some(p));
    assert_eq!(tree.region_parent(outer), Some(root_region));
}

// Nesting under a region is only allowed for that region's owner or for a
// node inside it; any other owner would break the owner chain.
#[test]
fn a_nested_region_under_another_owner_is_refused() {
    let mut tree = Structure::<VueNode, Branch, u8>::new("abc");
    let root_region = tree.add_region(None, None, 0).unwrap();
    let root = tree.push(None, root_region, Span::new(0, 3), 0).unwrap();
    let q = tree
        .push(Some(root), root_region, Span::new(0, 1), 1)
        .unwrap();
    let owned_by_q = tree.add_region(Some(root_region), Some(q), 1).unwrap();
    let p = tree
        .push(Some(root), root_region, Span::new(1, 2), 2)
        .unwrap();
    assert_eq!(
        tree.add_region(Some(owned_by_q), Some(p), 2),
        Err(StructureError::RegionOwnership)
    );
}

// Node iteration is preorder, and sibling links agree with it.
#[test]
fn node_ids_are_preorder() {
    let mut tree = Structure::<VueNode, Branch, u8>::new("abcde");
    let region = tree.add_region(None, None, 0).unwrap();
    let root = tree.push(None, region, Span::new(0, 5), 0).unwrap();
    let a = tree.push(Some(root), region, Span::new(1, 2), 1).unwrap();
    let a_child = tree.push(Some(a), region, Span::new(1, 2), 2).unwrap();
    let b = tree.push(Some(root), region, Span::new(3, 4), 3).unwrap();
    tree.push(None, region, Span::new(4, 5), 4).unwrap();
    let tree = tree.finish();
    let ids = tree.node_ids().collect::<Vec<_>>();
    let indices = ids.iter().map(|id| id.index()).collect::<Vec<_>>();
    assert_eq!(indices, [0, 1, 2, 3, 4]);
    assert_eq!(tree.first_child(root), Some(a));
    assert_eq!(tree.next_sibling(a), Some(b));
    assert_eq!(tree.first_child(a), Some(a_child));
    assert_eq!(tree.next_sibling(b), None);
    assert_eq!(tree.next_sibling(root), Some(ids[4]));
    assert_eq!(tree.source(), "abcde");
}

// Region iteration follows insertion, not nesting depth or owner position.
#[test]
fn region_ids_follow_insertion() {
    let mut tree = Structure::<VueNode, Branch, u8>::new("ab");
    let root_region = tree.add_region(None, None, 10).unwrap();
    let root = tree.push(None, root_region, Span::new(0, 2), 0).unwrap();
    let outer = tree.add_region(Some(root_region), Some(root), 11).unwrap();
    let inner = tree.add_region(Some(outer), Some(root), 12).unwrap();
    let second_root = tree.add_region(None, None, 13).unwrap();
    let tree = tree.finish();
    assert_eq!(
        tree.region_ids().collect::<Vec<_>>(),
        [root_region, outer, inner, second_root]
    );
    assert_eq!(
        tree.region_ids()
            .map(|id| *tree.region_data(id).unwrap())
            .collect::<Vec<_>>(),
        [10, 11, 12, 13]
    );
}
