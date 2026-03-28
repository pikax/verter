import fs from "node:fs";
import path from "node:path";

function parseArgs(argv) {
  let tracePath = "";
  let ownerFilter = "";
  let limit = 20;

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--trace") {
      tracePath = argv[++index] ?? "";
      continue;
    }
    if (arg === "--owner") {
      ownerFilter = argv[++index] ?? "";
      continue;
    }
    if (arg === "--limit") {
      const value = Number.parseInt(argv[++index] ?? "", 10);
      if (!Number.isFinite(value) || value <= 0) {
        throw new Error(`Invalid --limit value: ${argv[index]}`);
      }
      limit = value;
      continue;
    }
    throw new Error(`Unknown argument: ${arg}`);
  }

  if (!tracePath) {
    throw new Error("Missing required --trace <path>");
  }

  return {
    tracePath: path.resolve(tracePath),
    ownerFilter,
    limit,
  };
}

function increment(map, key) {
  map.set(key, (map.get(key) ?? 0) + 1);
}

function toSortedEntries(map, limit) {
  return [...map.entries()]
    .sort((left, right) => right[1] - left[1] || left[0].localeCompare(right[0]))
    .slice(0, limit);
}

function extractDetailValue(detail, key) {
  const match = detail.match(new RegExp(`${key}=([^ ]+)`));
  return match?.[1] ?? "";
}

function shouldKeepOwner(owner, ownerFilter) {
  return !ownerFilter || owner.includes(ownerFilter);
}

function main() {
  const { tracePath, ownerFilter, limit } = parseArgs(process.argv.slice(2));
  const contents = fs.readFileSync(tracePath, "utf8");
  const lines = contents.split(/\r?\n/);

  const parseCounts = new Map();
  const externalBuiltCounts = new Map();
  const externalHitCounts = new Map();
  const snapshotHostHitCounts = new Map();
  const snapshotHostMissCounts = new Map();
  const typeSourceHitCounts = new Map();
  const typeSourceMissCounts = new Map();
  const typeDependencyHitCounts = new Map();
  const typeDependencyMissCounts = new Map();
  const hostDependencyHitCounts = new Map();
  const hostDependencyMissCounts = new Map();

  for (const line of lines) {
    if (!line.includes("[verter-meta-trace]")) {
      continue;
    }
    const nameMatch = line.match(/name="([^"]+)"/);
    const detailMatch = line.match(/detail="([^"]*)"/);
    if (!nameMatch || !detailMatch) {
      continue;
    }
    const name = nameMatch[1];
    const detail = detailMatch[1];

    if (name === "parse_non_sfc_snapshot_result" || name === "parse_vue_snapshot_result") {
      const owner = extractDetailValue(detail, "owner");
      if (shouldKeepOwner(owner, ownerFilter)) {
        increment(parseCounts, owner);
      }
      continue;
    }

    if (name === "external_type_analysis_built" || name === "external_type_analysis_cache_hit") {
      const owner = extractDetailValue(detail, "owner");
      if (!shouldKeepOwner(owner, ownerFilter)) {
        continue;
      }
      if (name === "external_type_analysis_built") {
        increment(externalBuiltCounts, owner);
      } else {
        increment(externalHitCounts, owner);
      }
      continue;
    }

    if (name === "get_raw_analysis_snapshot_host_cache") {
      const owner = extractDetailValue(detail, "owner");
      if (!shouldKeepOwner(owner, ownerFilter)) {
        continue;
      }
      if (detail.includes("hit=true")) {
        increment(snapshotHostHitCounts, owner);
      } else {
        increment(snapshotHostMissCounts, owner);
      }
      continue;
    }

    if (name === "type_resolution_source_cache") {
      const owner = extractDetailValue(detail, "owner");
      if (!shouldKeepOwner(owner, ownerFilter)) {
        continue;
      }
      if (detail.includes("hit=true")) {
        increment(typeSourceHitCounts, owner);
      } else {
        increment(typeSourceMissCounts, owner);
      }
      continue;
    }

    if (name === "type_resolution_dependency_cache") {
      const owner = extractDetailValue(detail, "owner");
      const importSource = extractDetailValue(detail, "import");
      const key = `${owner} -> ${importSource}`;
      if (!shouldKeepOwner(owner, ownerFilter)) {
        continue;
      }
      if (detail.includes("hit=true")) {
        increment(typeDependencyHitCounts, key);
      } else {
        increment(typeDependencyMissCounts, key);
      }
      continue;
    }

    if (name === "cached_dependency_resolution_in_view_result") {
      const owner = extractDetailValue(detail, "owner");
      const importSource = extractDetailValue(detail, "import");
      const source = extractDetailValue(detail, "source");
      const key = `${owner} -> ${importSource}`;
      if (!shouldKeepOwner(owner, ownerFilter)) {
        continue;
      }
      if (source === "miss") {
        increment(hostDependencyMissCounts, key);
      } else {
        increment(hostDependencyHitCounts, `${key} [${source}]`);
      }
    }
  }

  const sections = [
    ["Parse Counts", parseCounts],
    ["External Analysis Built", externalBuiltCounts],
    ["External Analysis Cache Hits", externalHitCounts],
    ["Raw Snapshot Host Cache Hits", snapshotHostHitCounts],
    ["Raw Snapshot Host Cache Misses", snapshotHostMissCounts],
    ["Type Resolution Source Cache Hits", typeSourceHitCounts],
    ["Type Resolution Source Cache Misses", typeSourceMissCounts],
    ["Type Resolution Dependency Cache Hits", typeDependencyHitCounts],
    ["Type Resolution Dependency Cache Misses", typeDependencyMissCounts],
    ["Host Dependency Resolution Hits", hostDependencyHitCounts],
    ["Host Dependency Resolution Misses", hostDependencyMissCounts],
  ];

  console.log(`# Trace Summary`);
  console.log(`trace: ${tracePath}`);
  if (ownerFilter) {
    console.log(`owner filter: ${ownerFilter}`);
  }
  console.log(`limit: ${limit}`);

  for (const [title, map] of sections) {
    console.log(`\n## ${title}`);
    const rows = toSortedEntries(map, limit);
    if (rows.length === 0) {
      console.log("(none)");
      continue;
    }
    for (const [key, count] of rows) {
      console.log(`${count}\t${key}`);
    }
  }
}

main();
