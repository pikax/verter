#!/usr/bin/env node

/**
 * Script to run benchmarks and generate a markdown report
 */

import { exec } from "child_process";
import { promisify } from "util";
import * as fs from "fs";
import * as path from "path";
import * as os from "os";
import { fileURLToPath } from "url";
import { createRequire } from "module";

const execAsync = promisify(exec);
const __dirname = path.dirname(fileURLToPath(import.meta.url));
const require = createRequire(import.meta.url);

async function runBenchmarks() {
  console.log("🏃 Running benchmarks...\n");

  try {
    const { stdout, stderr } = await execAsync(
      "pnpm vitest bench --run --reporter=verbose",
      {
        cwd: path.resolve(__dirname, ".."),
        maxBuffer: 10 * 1024 * 1024, // 10MB buffer
      }
    );

    const output = stdout + stderr;
    return parseBenchmarkOutput(output);
  } catch (error) {
    // Vitest may exit with non-zero even on success
    const output = error.stdout + error.stderr;
    return parseBenchmarkOutput(output);
  }
}

function getSystemInfo() {
  const cpus = os.cpus();
  const totalMemory = (os.totalmem() / (1024 ** 3)).toFixed(2);
  
  return {
    platform: os.platform(),
    arch: os.arch(),
    cpu: cpus[0]?.model || "Unknown",
    cores: cpus.length,
    memory: `${totalMemory} GB`,
    nodeVersion: process.version,
  };
}

function parseBenchmarkOutput(output) {
  // Strip ANSI color codes
  const cleaned = output.replace(/\x1b\[[0-9;]*m/g, "");
  const lines = cleaned.split("\n");
  
  let markdown = `# Verter Benchmark Results\n\n`;

  const now = new Date();
  const year = now.getUTCFullYear();
  const month = String(now.getUTCMonth() + 1).padStart(2, '0');
  const day = String(now.getUTCDate()).padStart(2, '0');
  const hours = String(now.getUTCHours()).padStart(2, '0');
  const minutes = String(now.getUTCMinutes()).padStart(2, '0');
  const date = `${year}-${month}-${day} ${hours}:${minutes} UTC`;

  markdown += `**Generated:** ${date}\n\n`;
  
  // Add system information
  const sysInfo = getSystemInfo();
  markdown += `## System Information\n\n`;
  markdown += `| Property | Value |\n`;
  markdown += `|----------|-------|\n`;
  markdown += `| Platform | ${sysInfo.platform} ${sysInfo.arch} |\n`;
  markdown += `| CPU | ${sysInfo.cpu} |\n`;
  markdown += `| Cores | ${sysInfo.cores} |\n`;
  markdown += `| Memory | ${sysInfo.memory} |\n`;
  markdown += `| Node.js | ${sysInfo.nodeVersion} |\n\n`;

  // Add library versions
  const libVersions = getLibraryVersions();
  markdown += `## Library Versions\n\n`;
  markdown += `| Library | Version |\n`;
  markdown += `|---------|---------|\n`;
  for (const { name, version } of libVersions) {
    markdown += `| ${name} | ${version || "(not found)"} |\n`;
  }
  markdown += `\n`;
  
  markdown += `---\n\n`;

  // Group benchmarks by suite and collect all comparisons
  const benchmarkGroups = new Map();
  const allComparisons = [];
  let currentSuite = "";
  let currentFile = "";
  let inBenchmark = false;
  let benchmarkData = [];

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];

    // Detect suite headers (e.g., "✓ src/__benchmarks__/completions.bench.ts > ...")
    if (line.includes("✓") && line.includes("ms") && !line.trim().startsWith("·")) {
      if (benchmarkData.length > 0 && currentSuite) {
        if (!benchmarkGroups.has(currentFile)) {
          benchmarkGroups.set(currentFile, []);
        }
        benchmarkGroups.get(currentFile).push({
          suite: currentSuite,
          data: benchmarkData,
        });
        benchmarkData = [];
      }

      const fileMatch = line.match(/src\/__benchmarks__\/([^>]+\.bench\.ts)/);
      if (fileMatch) {
        currentFile = fileMatch[1];
      }

      currentSuite = extractSuiteName(line);
      inBenchmark = true;
      continue;
    }

    // Collect benchmark data lines (starts with · or contains column headers)
    if (inBenchmark && (line.trim().startsWith("·") || line.includes("name") && line.includes("hz"))) {
      benchmarkData.push(line);
    }

    // Stop collecting when we hit summary
    if (line.includes("BENCH  Summary") || line.includes("faster than")) {
      inBenchmark = false;
    }
  }

  // Add remaining benchmark data
  if (benchmarkData.length > 0 && currentSuite) {
    if (!benchmarkGroups.has(currentFile)) {
      benchmarkGroups.set(currentFile, []);
    }
    benchmarkGroups.get(currentFile).push({
      suite: currentSuite,
      data: benchmarkData,
    });
  }

  // Format grouped benchmarks
  for (const [file, benchmarks] of benchmarkGroups) {
    const result = formatBenchmarkFile(file, benchmarks);
    markdown += result.markdown;
    allComparisons.push(...result.comparisons);
  }

  // Add comparison table at the bottom
  if (allComparisons.length > 0) {
    markdown += `\n---\n\n## How to Run Benchmarks\n\n`;
    markdown += `To reproduce these results:\n\n`;
    markdown += `\`\`\`bash\n`;
    markdown += `pnpm i\n`;
    markdown += `# Run all benchmarks and generate this report\n`;
    markdown += `pnpm bench:report\n\n`;
    markdown += `# Or run benchmarks without generating report\n`;
    markdown += `pnpm bench\n\n`;
    markdown += `# Run benchmarks in watch mode\n`;
    markdown += `pnpm bench:watch\n\n`;
    markdown += `# Run benchmarks with verbose output\n`;
    markdown += `pnpm bench:compare\n`;
    markdown += `\`\`\`\n\n`;
    
    markdown += `---\n\n## Performance Summary\n\n`;
    
    // Add disclaimer
    markdown += `> **⚠️ Important Disclaimer**\n`;
    markdown += `>\n`;
    markdown += `> These benchmarks are **simulated tests** and may not fully represent real-world performance characteristics:\n`;
    markdown += `>\n`;
    markdown += `> - **Both systems use LSP+IPC architecture** for fair comparison in completion benchmarks\n`;
    markdown += `> - **Parser benchmarks measure different scopes**: Verter does AST parsing, Volar does full virtual code generation\n`;
    markdown += `> - **Verter is in heavy development** and currently lacks many features required for real-world usage (template completions, Vue directives, HTML tag completions, etc.)\n`;
    markdown += `> - These results primarily demonstrate TypeScript completion performance within Vue files\n`;
    markdown += `> - Production performance will vary based on project size, configuration, and usage patterns\n`;
    markdown += `>\n`;
    markdown += `> Use these benchmarks as **relative indicators** rather than absolute performance guarantees.\n\n`;
    
    markdown += `| Benchmark | Verter | Volar | Performance |\n`;
    markdown += `|-----------|--------|-------|-------------|\n`;
    
    for (const comp of allComparisons) {
      const verterOps = comp.verterHz.toLocaleString('en-US', { maximumFractionDigits: 2 });
      const volarOps = comp.volarHz.toLocaleString('en-US', { maximumFractionDigits: 2 });
      const perfIndicator = comp.ratio > 1 
        ? `✅ ${comp.ratio.toFixed(2)}x faster`
        : `⚠️ ${(1 / comp.ratio).toFixed(2)}x slower`;
      
      markdown += `| ${comp.name} | ${verterOps} ops/sec | ${volarOps} ops/sec | ${perfIndicator} |\n`;
    }
  }

  return markdown;
}

function extractSuiteName(line) {
  const match = line.match(/>\s*([^>]+?)\s+\d+ms/);
  if (match) {
    return match[1].trim();
  }
  return line.trim();
}

function getBenchmarkDescription(file) {
  const descriptions = {
    "parser.bench.ts": "Vue file parsing performance comparison. Note: Verter parses to AST while Volar generates full virtual TypeScript code.",
    "completions.bench.ts": "Tests Vue.js template completion performance in a simple component. Both use LSP+IPC architecture.",
    "real-world-components.bench.ts": "Measures completion performance in real-world Vue components with multiple edits and completion requests, simulating actual development workflows. Both use LSP+IPC architecture.",
  };
  return descriptions[file] || "Benchmark comparison between Volar and Verter.";
}

function formatBenchmarkFile(file, benchmarks) {
  let section = `## ${file}\n\n`;
  section += `**Description:** ${getBenchmarkDescription(file)}\n\n`;

  const comparisons = [];

  // Deduplicate benchmarks with same suite name
  const uniqueBenchmarks = new Map();
  for (const bench of benchmarks) {
    if (!uniqueBenchmarks.has(bench.suite)) {
      uniqueBenchmarks.set(bench.suite, bench);
    }
  }

  for (const bench of uniqueBenchmarks.values()) {
    section += `### ${bench.suite}\n\n`;
    const result = formatBenchmarkTable(bench.suite, bench.data);
    section += result.markdown;
    if (result.comparison) {
      comparisons.push(result.comparison);
    }
  }

  return { markdown: section, comparisons };
}

function formatBenchmarkTable(suiteName, data) {
  if (data.length === 0) return { markdown: "", comparison: null };

  let section = `\`\`\`\n`;
  
  for (const line of data) {
    section += line + "\n";
  }
  
  section += `\`\`\`\n\n`;

  let comparison = null;

  // Try to extract and format comparison - check for both naming patterns
  const volarLine = data.find((l) => (l.includes("Volar") || l.includes("volar")) && !l.includes("name"));
  const verterLine = data.find((l) => (l.includes("Verter") || l.includes("verter")) && !l.includes("name") && !l.includes("AcornLoose"));

  if (volarLine && verterLine) {
    const volarHz = extractHz(volarLine);
    const verterHz = extractHz(verterLine);

    if (volarHz && verterHz) {
      const ratio = verterHz / volarHz;
      section += `**Result:** `;
      
      if (ratio > 1) {
        section += `Verter is **${ratio.toFixed(2)}x faster** than Volar\n\n`;
      } else {
        section += `Volar is **${(1 / ratio).toFixed(2)}x faster** than Verter\n\n`;
      }

      comparison = {
        name: suiteName,
        verterHz,
        volarHz,
        ratio,
      };
    }
  }

  return { markdown: section, comparison };
}

function extractHz(line) {
  const match = line.match(/(\d+[,\d]*\.?\d*)\s+\d+\.\d+/);
  if (match) {
    return parseFloat(match[1].replace(/,/g, ""));
  }
  return null;
}

function getLibraryVersions() {
  const libraries = [
    "@verter/core",
    "@verter/language-server",
    "@volar/language-server",
    "vue",
    "typescript",
    "vitest",
    "@vue/language-server",
    "@vue/language-core",
    "@vue/language-service"
  ];

  const results = [];

  for (const name of libraries) {
    let version = null;
    try {
      const pkgPath = require.resolve(`${name}/package.json`);
      const pkg = JSON.parse(fs.readFileSync(pkgPath, "utf-8"));
      version = pkg.version;
    } catch {
      // ignore
    }
    results.push({ name, version });
  }

  return results;
}

// Main execution
runBenchmarks()
  .then((markdown) => {
    const outputPath = path.resolve(__dirname, "../results.md");
    fs.writeFileSync(outputPath, markdown, "utf-8");
    console.log(`\n✅ Benchmark results written to: ${outputPath}`);
  })
  .catch((error) => {
    console.error("Error running benchmarks:", error);
    process.exit(1);
  });
