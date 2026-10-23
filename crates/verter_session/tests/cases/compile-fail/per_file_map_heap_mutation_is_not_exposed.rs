use verter_semantic_source::per_file_map::PerFileMap;

#[allow(unused_mut)]
fn main() {
    let map = PerFileMap::<u32, String>::default();
    let mut value = map.entry(1).or_default();
    value.push_str("unaccounted growth");
}
