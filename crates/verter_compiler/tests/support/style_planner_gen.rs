//! The one CSS construct corpus the style-planner test binaries share.
//!
//! Both the allocation canaries and the shared-plan/staged-pipeline
//! equivalence sweep read these generators, so "every construct family the
//! Vue planners rewrite" means the same set of families on both sides. A
//! second hand-written copy of the same shapes would let one side silently
//! stop covering a family the other still measures — which is exactly the
//! divergence the equivalence sweep exists to catch.
//!
//! The byte output is pinned: `generator_mirror` in `allocator_canaries.rs`
//! digests every generator against
//! `test-corpora/style-ir/generator-mirror-digests.json`.

pub fn generate_class_rules(n: usize) -> String {
    (0..n)
        .map(|i| format!(".class-{i} {{ color: red; padding: {i}px; }}"))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn generate_descendant_selectors(n: usize) -> String {
    (0..n)
        .map(|i| format!(".parent-{i} .child-{i} {{ color: blue; }}"))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn generate_pseudo_selectors(n: usize) -> String {
    let pseudos = [":hover", ":focus", ":active", ":first-child", ":last-child"];
    (0..n)
        .map(|i| {
            let pseudo = pseudos[i % pseudos.len()];
            format!(".btn-{i}{pseudo} {{ color: red; }}")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn generate_selector_lists(n: usize) -> String {
    (0..n)
        .map(|i| {
            let selectors = (0..3)
                .map(|j| format!(".sel-{i}-{j}"))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{selectors} {{ margin: {i}px; }}")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn generate_v_bind_rules(n: usize) -> String {
    (0..n)
        .map(|i| format!(".item-{i} {{ color: v-bind(color{i}); }}"))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn generate_v_bind_dotted(n: usize) -> String {
    (0..n)
        .map(|i| format!(".item-{i} {{ color: v-bind('theme.colors.primary{i}'); }}"))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn generate_deep_rules(n: usize) -> String {
    (0..n)
        .map(|i| format!(":deep(.inner-{i}) {{ color: red; }}"))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn generate_slotted_rules(n: usize) -> String {
    (0..n)
        .map(|i| format!(":slotted(.slot-{i}) {{ color: red; }}"))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn generate_mixed_vue(n: usize) -> String {
    (0..n)
        .map(|i| match i % 3 {
            0 => format!(".item-{i} {{ color: v-bind(color{i}); }}"),
            1 => format!(":deep(.inner-{i}) {{ padding: {i}px; }}"),
            _ => format!(":slotted(.slot-{i}) {{ margin: {i}px; }}"),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn generate_global_rules(n: usize) -> String {
    (0..n)
        .map(|i| format!(":global(.reset-{i}) {{ margin: 0; }}"))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn generate_repeated_classes(unique: usize, repeats: usize) -> String {
    let mut rules = Vec::new();
    for r in 0..repeats {
        for i in 0..unique {
            rules.push(format!(".btn-{i} {{ padding: {r}px; }}"));
        }
    }
    rules.join("\n")
}

pub const N: usize = 50;

pub fn all_categories() -> [(&'static str, String); 11] {
    [
        ("class_rules", generate_class_rules(N)),
        ("descendant_selectors", generate_descendant_selectors(N)),
        ("pseudo_selectors", generate_pseudo_selectors(N)),
        ("selector_lists", generate_selector_lists(N)),
        ("v_bind_rules", generate_v_bind_rules(N)),
        ("v_bind_dotted", generate_v_bind_dotted(N)),
        ("deep_rules", generate_deep_rules(N)),
        ("slotted_rules", generate_slotted_rules(N)),
        ("mixed_vue", generate_mixed_vue(N)),
        ("global_rules", generate_global_rules(N)),
        ("repeated_classes", generate_repeated_classes(5, 10)),
    ]
}
