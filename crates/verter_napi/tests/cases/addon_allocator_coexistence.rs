//! Two addon images, each carrying its own static mimalloc, in one Node
//! process.
//!
//! The shipped addon installs mimalloc as its Rust global allocator, and a
//! real Node process routinely loads other addons that link their own
//! mimalloc v3 (rolldown's binding is one). Each image's copy must keep its
//! own per-thread state. With the plain `mimalloc` crate's v3 on macOS every
//! copy shares one fixed TLS slot, so the second copy adopts the first's
//! thread heap and the process aborts (microsoft/mimalloc#1301); on Linux an
//! initial-exec TLS model in a `dlopen`ed image can fail to load at all.
//!
//! This suite loads the fixture addon twice, from two copies of its shared
//! library under different names, and first proves the loader mapped two
//! images with two allocators: the images' statics sit at different
//! addresses, and each image's mimalloc owns the block it allocated and
//! rejects the other image's. It then compiles through both images on the
//! main thread and on worker threads that load both, allocate in both, and
//! exit (running both copies' thread-exit hooks), and requires the process
//! to finish cleanly. The fixture inherits `verter_napi`'s
//! `#[global_allocator]` and load-time configuration, so it runs the
//! allocator the shipped addon runs.

use std::path::Path;
use std::process::Command;

use super::support::fixture_addon_library;

const CASES: &[&str] = &[
    "two-images-are-two-allocator-copies",
    "each-image-runs-on-the-selected-allocator",
    "each-image-applied-its-load-time-configuration",
    "interleaved-compiles-on-the-main-thread",
    "worker-threads-compile-through-both-images-and-exit",
];

const DRIVER: &str = r#"
const { Worker, isMainThread, parentPort, workerData } = require("node:worker_threads");

const SOURCE = (n) =>
  `<script setup lang="ts">\nimport { ref } from "vue"\nconst count${n} = ref(${n})\nconst items = ref([${n}, ${n + 1}])\n</script>\n` +
  `<template><ul><li v-for="item in items" :key="item">{{ item + count${n} }}</li></ul></template>`;

function compile(addon, round) {
  const canonicalId = `/coexist/C${round}.vue`;
  const host = new addon.VerterHost();
  try {
    host.upsert({ canonicalId, inputId: canonicalId, source: Buffer.from(SOURCE(round)), fileKind: "vue" });
    const response = host.compileRequest(canonicalId, {
      framework: "vue",
      identity: { filename: canonicalId, componentId: `c${round}`, isProduction: false, forceJs: false },
      products: [{ kind: "runtimeClient", runtimeSourceMap: true }],
      options: { backend: "inferred", ssr: false, isCustomElement: [], babelParserPlugins: [] },
    });
    const product = response && response.products && response.products[0];
    const main = product && Array.isArray(product.nodes) && product.nodes.find((n) => n.node.kind === "main");
    if (!main || typeof main.code !== "string" || main.code.length === 0) {
      throw new Error(`round ${round}: no runtime output: ${JSON.stringify(response)}`);
    }
  } finally {
    host.close();
  }
}

function interleave(a, b, rounds, offset) {
  for (let round = 0; round < rounds; round++) {
    compile(a, offset + round);
    compile(b, offset + round);
  }
}

if (!isMainThread) {
  interleave(require(workerData.a), require(workerData.b), 2, workerData.offset);
  parentPort.postMessage("done");
  return;
}

const [aPath, bPath, expectMimalloc, expectedPurgeDelay] = process.argv.slice(2);
const report = (name, failure) =>
  console.log(failure == null ? `PASS ${name}` : `FAIL ${name}: ${failure}`);
const a = require(aPath);
const b = require(bPath);
const wantMimalloc = expectMimalloc === "true";

{
  const statics = [a.fixtureStaticAddress(), b.fixtureStaticAddress()];
  const heldA = a.fixtureHeldBlockAddress();
  const heldB = b.fixtureHeldBlockAddress();
  const owns = {
    aOwnsA: a.fixtureMimallocOwns(heldA),
    bOwnsB: b.fixtureMimallocOwns(heldB),
    aOwnsB: a.fixtureMimallocOwns(heldB),
    bOwnsA: b.fixtureMimallocOwns(heldA),
  };
  const failures = [];
  if (statics[0] === statics[1]) failures.push(`both images report one static at ${statics[0]}`);
  if (wantMimalloc) {
    if (!owns.aOwnsA || !owns.bOwnsB) failures.push(`an image does not own its own block: ${JSON.stringify(owns)}`);
    if (owns.aOwnsB || owns.bOwnsA) failures.push(`an image owns the other's block: ${JSON.stringify(owns)}`);
  }
  report("two-images-are-two-allocator-copies", failures.length ? failures.join("; ") : null);
}

const served = [a.fixtureMimallocServesThisImage(), b.fixtureMimallocServesThisImage()];
report(
  "each-image-runs-on-the-selected-allocator",
  served.every((s) => s === wantMimalloc) ? null : `served=${served}, expected ${wantMimalloc}`,
);

const delays = [a.fixtureMimallocPurgeDelayMs(), b.fixtureMimallocPurgeDelayMs()];
const wantDelay = expectedPurgeDelay === "none" ? null : Number(expectedPurgeDelay);
report(
  "each-image-applied-its-load-time-configuration",
  delays.every((d) => d === wantDelay) ? null : `purge delays ${delays}, expected ${wantDelay}`,
);

let mainFailure = null;
try {
  interleave(a, b, 2, 0);
} catch (error) {
  mainFailure = String((error && error.stack) || error);
}
report("interleaved-compiles-on-the-main-thread", mainFailure);

// One worker: it loads both images, allocates in both, and its thread exit
// runs both copies' thread-exit hooks.
const workers = [100].map(
  (offset) =>
    new Promise((resolve, reject) => {
      const worker = new Worker(__filename, { workerData: { a: aPath, b: bPath, offset } });
      let done = false;
      worker.on("message", () => (done = true));
      worker.on("error", reject);
      worker.on("exit", (code) =>
        code === 0 && done ? resolve() : reject(new Error(`worker exited ${code}`)),
      );
    }),
);
Promise.all(workers).then(
  () => {
    // Allocate in both images again after the workers' threads are gone.
    interleave(a, b, 1, 900);
    report("worker-threads-compile-through-both-images-and-exit", null);
  },
  (error) => report("worker-threads-compile-through-both-images-and-exit", String((error && error.stack) || error)),
);
"#;

#[test]
fn two_addon_images_with_their_own_mimalloc_share_one_node_process() {
    let library = fixture_addon_library();

    let scratch = Path::new(env!("CARGO_TARGET_TMPDIR")).join("napi_allocator_coexistence");
    let (dir_a, dir_b) = (scratch.join("a"), scratch.join("b"));
    std::fs::create_dir_all(&dir_a).expect("the scratch directory is creatable");
    std::fs::create_dir_all(&dir_b).expect("the scratch directory is creatable");
    // Two files, two names: the loader maps each as its own image.
    let addon_a = dir_a.join("coexist_a.node");
    let addon_b = dir_b.join("coexist_b.node");
    std::fs::copy(&library, &addon_a).expect("the built addon is copyable");
    std::fs::copy(&library, &addon_b).expect("the built addon is copyable");
    let driver = scratch.join("coexist.cjs");
    std::fs::write(&driver, DRIVER).expect("the driver is writable");

    let expect_mimalloc = verter_alloc::SELECTED == verter_alloc::AllocatorKind::Mimalloc;
    let expected_purge_delay = if expect_mimalloc {
        verter_alloc::PURGE_DELAY_MS.to_string()
    } else {
        "none".to_owned()
    };

    // The child's mimalloc copies read no purge delay from the environment,
    // so the load-time configuration is what decides it.
    let run = Command::new("node")
        .env_remove("MIMALLOC_PURGE_DELAY")
        .env_remove("MIMALLOC_RESET_DELAY")
        .arg(&driver)
        .arg(&addon_a)
        .arg(&addon_b)
        .arg(expect_mimalloc.to_string())
        .arg(&expected_purge_delay)
        .output()
        .expect("node is on PATH");

    let stdout = String::from_utf8_lossy(&run.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&run.stderr).into_owned();
    let transcript = format!(
        "status: {}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        run.status
    );

    assert!(
        run.status.success(),
        "a process holding both images must exit cleanly; {transcript}"
    );
    for case in CASES {
        assert!(
            stdout.contains(&format!("PASS {case}")),
            "`{case}` did not pass; {transcript}"
        );
    }
    assert!(
        !stdout.contains("FAIL "),
        "every case must pass; {transcript}"
    );
}
