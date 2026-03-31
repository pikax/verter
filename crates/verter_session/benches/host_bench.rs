//! Benchmarks for VerterHost hot paths.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::sync::Arc;
use verter_session::{
    CompileProfile, FileKind, HostConfig, UpsertRequest, VerterHost, VirtualNodeKind, VirtualQuery,
};

const SMALL_SFC: &str = r#"<script setup lang="ts">
import { ref } from 'vue'
const count = ref(0)
</script>

<template>
  <button @click="count++">{{ count }}</button>
</template>

<style scoped>
button { color: red; }
</style>
"#;

const MEDIUM_SFC: &str = r#"<script setup lang="ts">
import { ref, computed, onMounted, provide, inject } from 'vue'
import type { PropType } from 'vue'

interface Item {
  id: number
  label: string
  done: boolean
}

const props = defineProps<{
  title: string
  items: Item[]
  maxItems?: number
}>()

const emit = defineEmits<{
  update: [items: Item[]]
  select: [item: Item]
}>()

const search = ref('')
const selected = ref<Item | null>(null)

const filtered = computed(() =>
  props.items.filter(i => i.label.includes(search.value))
)

provide('searchQuery', search)
const theme = inject<string>('theme', 'light')

onMounted(() => {
  console.log('mounted', props.title)
})

function toggle(item: Item) {
  item.done = !item.done
  emit('update', props.items)
}
</script>

<template>
  <div class="container">
    <h1>{{ title }}</h1>
    <input v-model="search" placeholder="Search..." />
    <ul>
      <li
        v-for="item in filtered"
        :key="item.id"
        :class="{ done: item.done }"
        @click="toggle(item)"
      >
        {{ item.label }}
      </li>
    </ul>
    <p v-if="filtered.length === 0">No items found</p>
  </div>
</template>

<style scoped>
.container { padding: 1rem; }
.done { text-decoration: line-through; opacity: 0.6; }
input { margin-bottom: 0.5rem; }
</style>
"#;

fn make_host() -> VerterHost {
    VerterHost::new_standalone(HostConfig::default())
}

fn upsert(host: &VerterHost, id: &str, source: &str) {
    host.upsert(UpsertRequest {
        canonical_id: None,
        input_id: id.to_string(),
        source: Arc::from(source),
        file_kind: FileKind::VueSfc,
        aliases: Vec::new(),
    })
    .unwrap();
}

fn bench_upsert_first_time(c: &mut Criterion) {
    let mut group = c.benchmark_group("upsert_first_time");

    group.bench_function("small_sfc", |b| {
        b.iter(|| {
            let host = make_host();
            upsert(&host, "Comp.vue", black_box(SMALL_SFC));
        })
    });

    group.bench_function("medium_sfc", |b| {
        b.iter(|| {
            let host = make_host();
            upsert(&host, "Comp.vue", black_box(MEDIUM_SFC));
        })
    });

    group.finish();
}

fn bench_upsert_no_change(c: &mut Criterion) {
    let host = make_host();
    upsert(&host, "Comp.vue", MEDIUM_SFC);

    c.bench_function("upsert_no_change", |b| {
        b.iter(|| {
            upsert(&host, "Comp.vue", black_box(MEDIUM_SFC));
        })
    });
}

fn bench_upsert_style_only_change(c: &mut Criterion) {
    let host = make_host();
    upsert(&host, "Comp.vue", MEDIUM_SFC);

    // Change only the style block content
    let modified = MEDIUM_SFC.replace("padding: 1rem", "padding: 2rem");

    c.bench_function("upsert_style_only_change", |b| {
        b.iter_batched(
            || {
                // Reset to original
                upsert(&host, "Comp.vue", MEDIUM_SFC);
            },
            |()| {
                upsert(&host, "Comp.vue", black_box(&modified));
            },
            criterion::BatchSize::SmallInput,
        )
    });
}

fn bench_compile_cache_hit(c: &mut Criterion) {
    let host = make_host();
    upsert(&host, "Comp.vue", MEDIUM_SFC);

    let profile = CompileProfile::default();

    // Warm the compile cache
    host.get_virtual_file(VirtualQuery {
        raw_id: None,
        canonical_id: Some("Comp.vue".to_string()),
        node_kind: Some(VirtualNodeKind::Main),
        compile_profile: profile.clone(),
    })
    .unwrap();

    c.bench_function("compile_cache_hit", |b| {
        b.iter(|| {
            black_box(
                host.get_virtual_file(VirtualQuery {
                    raw_id: None,
                    canonical_id: Some("Comp.vue".to_string()),
                    node_kind: Some(VirtualNodeKind::Main),
                    compile_profile: profile.clone(),
                })
                .unwrap(),
            );
        })
    });
}

fn bench_compile_cache_miss(c: &mut Criterion) {
    c.bench_function("compile_cache_miss", |b| {
        b.iter_batched(
            || {
                let host = make_host();
                upsert(&host, "Comp.vue", MEDIUM_SFC);
                host
            },
            |host| {
                let profile = CompileProfile::default();
                black_box(
                    host.get_virtual_file(VirtualQuery {
                        raw_id: None,
                        canonical_id: Some("Comp.vue".to_string()),
                        node_kind: Some(VirtualNodeKind::Main),
                        compile_profile: profile,
                    })
                    .unwrap(),
                );
            },
            criterion::BatchSize::SmallInput,
        )
    });
}

fn bench_resolve(c: &mut Criterion) {
    let host = make_host();
    upsert(&host, "/src/Comp.vue", SMALL_SFC);

    let mut group = c.benchmark_group("resolve");

    group.bench_function("canonical", |b| {
        b.iter(|| {
            black_box(host.resolve(black_box("/src/Comp.vue")));
        })
    });

    group.bench_function("bundler_query", |b| {
        b.iter(|| {
            black_box(host.resolve(black_box(
                "/src/Comp.vue?vue&type=style&index=0&scoped=true&lang.css",
            )));
        })
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_upsert_first_time,
    bench_upsert_no_change,
    bench_upsert_style_only_change,
    bench_compile_cache_hit,
    bench_compile_cache_miss,
    bench_resolve,
);
criterion_main!(benches);
