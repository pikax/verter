import fs from "node:fs";
import path from "node:path";
import process from "node:process";

const PHASES = [
  { key: "initialize", label: "Initialize" },
  { key: "workspaceScan", label: "Workspace Scan" },
  { key: "didOpenToHover", label: "didOpen -> Hover" },
  { key: "hoverCold", label: "Hover (cold)" },
  { key: "hoverWarmMedian", label: "Hover (median of 5)" },
];

const OS_INFO = {
  linux: { label: "Linux", order: 0 },
  macos: { label: "macOS", order: 1 },
  windows: { label: "Windows", order: 2 },
  other: { label: "Other", order: 99 },
};

function formatMs(value) {
  if (value == null || Number.isNaN(value)) {
    return "N/A";
  }
  if (value === 0) {
    return "N/A";
  }
  if (value >= 1000) {
    return `${(value / 1000).toFixed(1)}s`;
  }
  if (value < 1) {
    return `${value.toFixed(2)}ms`;
  }
  return `${Math.round(value)}ms`;
}

function normalizeConfig(config) {
  return {
    initialize: config.initialize ?? null,
    workspaceScan: config.workspaceScan ?? null,
    didOpenToHover: config.didOpenToHover ?? null,
    hoverCold: config.hoverCold ?? config.hoverWarm ?? null,
    hoverWarmMedian: config.hoverWarmMedian ?? config.hoverMedian ?? null,
  };
}

function inferOsKey(fileName, platform) {
  const lowerName = (fileName ?? "").toLowerCase();
  if (lowerName.includes("windows") || platform === "win32") {
    return "windows";
  }
  if (lowerName.includes("macos") || lowerName.includes("darwin") || platform === "darwin") {
    return "macos";
  }
  if (lowerName.includes("linux") || platform === "linux") {
    return "linux";
  }
  return "other";
}

function compareRuns(a, b) {
  const orderA = OS_INFO[a.osKey]?.order ?? OS_INFO.other.order;
  const orderB = OS_INFO[b.osKey]?.order ?? OS_INFO.other.order;
  if (orderA !== orderB) {
    return orderA - orderB;
  }
  return a.osLabel.localeCompare(b.osLabel);
}

export function normalizeMatrixRun({ fileName, json }) {
  const osKey = inferOsKey(fileName, json.platform);
  const osLabel = OS_INFO[osKey]?.label ?? OS_INFO.other.label;

  return {
    fileName,
    osKey,
    osLabel,
    json: {
      ...json,
      configs: Object.fromEntries(
        Object.entries(json.configs ?? {}).map(([name, config]) => [name, normalizeConfig(config)]),
      ),
    },
  };
}

export function buildMarkdownReport(runs) {
  const sortedRuns = [...runs].sort(compareRuns);
  if (sortedRuns.length === 0) {
    return "## LSP Benchmark Results\n\nNo benchmark results were found.";
  }

  const first = sortedRuns[0].json;
  const lines = [];
  lines.push("## LSP Benchmark Results");
  lines.push("");
  lines.push(
    `**${first.project}** (${first.vueFileCount.toLocaleString()} .vue files) — \`${first.testFile}\` (${first.testFileLines.toLocaleString()} lines)`,
  );
  lines.push("");

  for (const run of sortedRuns) {
    const details = [];
    if (run.json.platform) {
      details.push(run.json.platform);
    }
    if (run.json.arch) {
      details.push(run.json.arch);
    }

    lines.push(`### ${run.osLabel}${details.length ? ` (${details.join("/")})` : ""}`);
    lines.push("");

    const configNames = Object.keys(run.json.configs);
    lines.push(`| Phase | ${configNames.join(" | ")} |`);
    lines.push(`|-------|${configNames.map(() => "---|").join("")}`);

    for (const phase of PHASES) {
      const values = configNames.map((name) => {
        const value = run.json.configs[name]?.[phase.key];
        if (phase.key === "workspaceScan" && value === 0) {
          return "N/A";
        }
        return formatMs(value);
      });
      lines.push(`| ${phase.label} | ${values.join(" | ")} |`);
    }

    lines.push("");
  }

  return lines.join("\n");
}

export function buildAggregateJson(runs) {
  const sortedRuns = [...runs].sort(compareRuns);
  return {
    generatedAt: new Date().toISOString(),
    runs: sortedRuns.map((run) => ({
      osKey: run.osKey,
      osLabel: run.osLabel,
      fileName: run.fileName,
      ...run.json,
    })),
  };
}

function collectJsonFiles(inputDir) {
  return fs
    .readdirSync(inputDir, { withFileTypes: true })
    .filter((entry) => entry.isFile() && entry.name.endsWith(".json"))
    .map((entry) => path.join(inputDir, entry.name))
    .sort();
}

function parseArgs(argv) {
  const args = {
    inputDir: "",
    markdownOut: "",
    jsonOut: "",
  };

  for (const arg of argv) {
    if (arg.startsWith("--input-dir=")) {
      args.inputDir = arg.slice("--input-dir=".length);
    } else if (arg.startsWith("--markdown-out=")) {
      args.markdownOut = arg.slice("--markdown-out=".length);
    } else if (arg.startsWith("--json-out=")) {
      args.jsonOut = arg.slice("--json-out=".length);
    }
  }

  return args;
}

function main() {
  const args = parseArgs(process.argv.slice(2));
  if (!args.inputDir) {
    console.error("Missing --input-dir=/path/to/results");
    process.exit(1);
  }

  const files = collectJsonFiles(args.inputDir);
  if (files.length === 0) {
    console.error(`No benchmark JSON files found in ${args.inputDir}`);
    process.exit(1);
  }

  const runs = files.map((filePath) =>
    normalizeMatrixRun({
      fileName: path.basename(filePath),
      json: JSON.parse(fs.readFileSync(filePath, "utf8")),
    }),
  );

  const markdown = buildMarkdownReport(runs);
  const aggregateJson = JSON.stringify(buildAggregateJson(runs), null, 2);

  if (args.markdownOut) {
    fs.writeFileSync(args.markdownOut, markdown);
  } else {
    process.stdout.write(`${markdown}\n`);
  }

  if (args.jsonOut) {
    fs.writeFileSync(args.jsonOut, aggregateJson);
  }
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main();
}
