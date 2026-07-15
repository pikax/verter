use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::hint::black_box;
use verter_compiler::tokenizer::byte::tokenize;

/// Simple tag with N attributes (no v-pre)
fn generate_tag_with_attrs(n: usize) -> String {
    let attrs: Vec<String> = (0..n)
        .map(|i| format!(r#"attr{}="value{}""#, i, i))
        .collect();
    format!("<div {}>content</div>", attrs.join(" "))
}

/// Tag with N attributes where v-pre is first
fn generate_tag_v_pre_first(n: usize) -> String {
    let attrs: Vec<String> = (0..n)
        .map(|i| format!(r#"attr{}="value{}""#, i, i))
        .collect();
    format!("<div v-pre {}>content</div>", attrs.join(" "))
}

/// Tag with N attributes where v-pre is last
fn generate_tag_v_pre_last(n: usize) -> String {
    let attrs: Vec<String> = (0..n)
        .map(|i| format!(r#"attr{}="value{}""#, i, i))
        .collect();
    format!("<div {} v-pre>content</div>", attrs.join(" "))
}

/// Realistic Vue SFC template with mixed content
fn generate_realistic_template(tag_count: usize) -> String {
    let mut parts = Vec::new();
    parts.push("<template>".to_string());
    parts.push(r#"<div class="app">"#.to_string());

    for i in 0..tag_count {
        match i % 5 {
            0 => parts.push(format!(
                r#"<span class="item-{}" id="el-{}" data-idx="{}">text {}</span>"#,
                i, i, i, i
            )),
            1 => parts.push(format!(
                r#"<input v-model="form.field{}" type="text" placeholder="Enter {}" />"#,
                i, i
            )),
            2 => parts.push(format!(
                r#"<button @click="handler{}" :disabled="loading">Click {}</button>"#,
                i, i
            )),
            3 => parts.push(format!(
                r#"<p v-if="show{}" v-bind:class="cls{}">{{{{ msg{} }}}}</p>"#,
                i, i, i
            )),
            4 => parts.push(format!(
                r#"<div v-for="item in list{}" :key="item.id">{{{{ item.name }}}}</div>"#,
                i
            )),
            _ => unreachable!(),
        }
    }

    parts.push("</div>".to_string());
    parts.push("</template>".to_string());
    parts.join("\n")
}

/// Realistic template with a few v-pre elements mixed in
fn generate_realistic_with_v_pre(tag_count: usize) -> String {
    let mut parts = Vec::new();
    parts.push("<template>".to_string());
    parts.push(r#"<div class="app">"#.to_string());

    for i in 0..tag_count {
        if i % 10 == 7 {
            // Every 10th element (at offset 7) has v-pre
            parts.push(format!(
                r#"<code class="example-{}" id="code-{}" v-pre>{{{{ raw{} }}}}</code>"#,
                i, i, i
            ));
        } else {
            match i % 3 {
                0 => parts.push(format!(r#"<span class="item-{}">text {}</span>"#, i, i)),
                1 => parts.push(format!(
                    r#"<button @click="handle{}" :class="cls{}">btn {}</button>"#,
                    i, i, i
                )),
                2 => parts.push(format!("<p>{{{{ msg{} }}}}</p>", i)),
                _ => unreachable!(),
            }
        }
    }

    parts.push("</div>".to_string());
    parts.push("</template>".to_string());
    parts.join("\n")
}

fn bench_no_v_pre(c: &mut Criterion) {
    let mut group = c.benchmark_group("tokenizer/no_v_pre");

    for attr_count in [1, 5, 10, 20] {
        let input = generate_tag_with_attrs(attr_count);
        let bytes = input.as_bytes();
        group.throughput(Throughput::Bytes(bytes.len() as u64));

        group.bench_with_input(BenchmarkId::new("attrs", attr_count), &bytes, |b, input| {
            b.iter(|| {
                tokenize(black_box(input), |event| {
                    black_box(event);
                });
            });
        });
    }

    group.finish();
}

fn bench_v_pre_first(c: &mut Criterion) {
    let mut group = c.benchmark_group("tokenizer/v_pre_first");

    for attr_count in [1, 5, 10, 20] {
        let input = generate_tag_v_pre_first(attr_count);
        let bytes = input.as_bytes();
        group.throughput(Throughput::Bytes(bytes.len() as u64));

        group.bench_with_input(BenchmarkId::new("attrs", attr_count), &bytes, |b, input| {
            b.iter(|| {
                tokenize(black_box(input), |event| {
                    black_box(event);
                });
            });
        });
    }

    group.finish();
}

fn bench_v_pre_last(c: &mut Criterion) {
    let mut group = c.benchmark_group("tokenizer/v_pre_last");

    for attr_count in [1, 5, 10, 20] {
        let input = generate_tag_v_pre_last(attr_count);
        let bytes = input.as_bytes();
        group.throughput(Throughput::Bytes(bytes.len() as u64));

        group.bench_with_input(BenchmarkId::new("attrs", attr_count), &bytes, |b, input| {
            b.iter(|| {
                tokenize(black_box(input), |event| {
                    black_box(event);
                });
            });
        });
    }

    group.finish();
}

fn bench_realistic_template(c: &mut Criterion) {
    let mut group = c.benchmark_group("tokenizer/realistic");

    for tag_count in [10, 50, 100] {
        let input = generate_realistic_template(tag_count);
        let bytes = input.as_bytes();
        group.throughput(Throughput::Bytes(bytes.len() as u64));

        group.bench_with_input(BenchmarkId::new("tags", tag_count), &bytes, |b, input| {
            b.iter(|| {
                tokenize(black_box(input), |event| {
                    black_box(event);
                });
            });
        });
    }

    group.finish();
}

fn bench_realistic_with_v_pre(c: &mut Criterion) {
    let mut group = c.benchmark_group("tokenizer/realistic_v_pre");

    for tag_count in [10, 50, 100] {
        let input = generate_realistic_with_v_pre(tag_count);
        let bytes = input.as_bytes();
        group.throughput(Throughput::Bytes(bytes.len() as u64));

        group.bench_with_input(BenchmarkId::new("tags", tag_count), &bytes, |b, input| {
            b.iter(|| {
                tokenize(black_box(input), |event| {
                    black_box(event);
                });
            });
        });
    }

    group.finish();
}

fn bench_entity_handling(c: &mut Criterion) {
    let mut group = c.benchmark_group("tokenizer/entities");

    // Text with no entities
    let no_entities = "Hello world, this is a simple text without any entities at all";
    let bytes = no_entities.as_bytes();
    group.throughput(Throughput::Bytes(bytes.len() as u64));
    group.bench_with_input(BenchmarkId::new("none", ""), &bytes, |b, input| {
        b.iter(|| {
            tokenize(black_box(input), |event| {
                black_box(event);
            });
        });
    });

    // Text with entities
    let with_entities = "Hello &amp; world &lt;div&gt; &quot;test&quot; &apos;foo&apos;";
    let bytes = with_entities.as_bytes();
    group.throughput(Throughput::Bytes(bytes.len() as u64));
    group.bench_with_input(BenchmarkId::new("several", ""), &bytes, |b, input| {
        b.iter(|| {
            tokenize(black_box(input), |event| {
                black_box(event);
            });
        });
    });

    group.finish();
}

fn bench_textarea_interpolation(c: &mut Criterion) {
    let mut group = c.benchmark_group("tokenizer/textarea");

    // Textarea with interpolation
    let input = "<textarea>{{ message }} and {{ other }}</textarea>";
    let bytes = input.as_bytes();
    group.throughput(Throughput::Bytes(bytes.len() as u64));
    group.bench_with_input(
        BenchmarkId::new("with_interpolation", ""),
        &bytes,
        |b, input| {
            b.iter(|| {
                tokenize(black_box(input), |event| {
                    black_box(event);
                });
            });
        },
    );

    // Script without interpolation (for comparison)
    let input = "<script>{{ message }} and {{ other }}</script>";
    let bytes = input.as_bytes();
    group.throughput(Throughput::Bytes(bytes.len() as u64));
    group.bench_with_input(
        BenchmarkId::new("script_no_interpolation", ""),
        &bytes,
        |b, input| {
            b.iter(|| {
                tokenize(black_box(input), |event| {
                    black_box(event);
                });
            });
        },
    );

    group.finish();
}

criterion_group!(
    benches,
    bench_no_v_pre,
    bench_v_pre_first,
    bench_v_pre_last,
    bench_realistic_template,
    bench_realistic_with_v_pre,
    bench_entity_handling,
    bench_textarea_interpolation,
);
criterion_main!(benches);
