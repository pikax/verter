#!/usr/bin/env node

/**
 * Script to run benchmarks and generate a markdown report
 */

import { exec, spawn } from "child_process";
import { promisify } from "util";
import * as fs from "fs";
import * as path from "path";
import * as os from "os";
import { fileURLToPath } from "url";
import { createRequire } from "module";

const execAsync = promisify(exec);
const __dirname = path.dirname(fileURLToPath(import.meta.url));
const require = createRequire(import.meta.url);

function runBenchmarks() {
  console.log("🏃 Running benchmarks...\n");

  return new Promise((resolve, reject) => {
    const vitest = spawn("pnpm", ["vitest", "bench", "--run", "--reporter=verbose"], {
      cwd: path.resolve(__dirname, ".."),
      stdio: ["ignore", "pipe", "pipe"],
      shell: process.platform === 'win32',
    });

    let buffer = "";
    let processedSuites = 0;
    let totalSuites = null;
    const seenSuiteLines = new Set();
    const startTime = Date.now();
    const suiteTimes = [];

    function formatTime(ms) {
      const seconds = Math.floor(ms / 1000);
      const minutes = Math.floor(seconds / 60);
      const remainingSeconds = seconds % 60;
      if (minutes > 0) {
        return `${minutes}m ${remainingSeconds}s`;
      }
      return `${seconds}s`;
    }

    function handleChunk(chunk, isErr = false) {
      const text = chunk.toString();
      buffer += text;
      const lines = text.split(/\r?\n/);
      for (const line of lines) {
        // Strip ANSI codes for matching
        const cleanLine = line.replace(/\x1b\[[0-9;]*m/g, "");
        
        // Try to detect total from lines like "❯ src/parser/parser.bench.ts 74/80"
        const totalMatch = cleanLine.match(/(\d+)\/(\d+)\s*$/);
        if (totalMatch && !totalSuites) {
          totalSuites = parseInt(totalMatch[2], 10);
        }
        
        // Real-time detection of suite line: starts with optional spaces then ✓ and ends with ms
        if (/^\s*✓\s+.*\b\d+ms\b/.test(cleanLine)) {
          if (!seenSuiteLines.has(cleanLine)) {
            seenSuiteLines.add(cleanLine);
            processedSuites++;
            suiteTimes.push(Date.now());
            
            // Extract suite name if possible (everything after the last >)
            let suiteName = "";
            const parts = cleanLine.split(">");
            if (parts.length > 1) {
              suiteName = parts[parts.length - 1].replace(/\d+ms$/, "").trim();
            }

            // Calculate elapsed and estimate remaining
            const elapsed = Date.now() - startTime;
            const elapsedStr = formatTime(elapsed);
            
            let remainingStr = "";
            if (totalSuites && processedSuites > 1) {
              const avgTimePerSuite = elapsed / processedSuites;
              const remaining = avgTimePerSuite * (totalSuites - processedSuites);
              remainingStr = ` | ETA: ${formatTime(remaining)}`;
            }

            // Progress bar
            const barLength = 20;
            let pct = 0;
            let filled = 0;
            if (totalSuites) {
              pct = Math.round((processedSuites / totalSuites) * 100);
              filled = Math.round((processedSuites / totalSuites) * barLength);
            } else {
              // Unknown total, show spinner-style progress
              filled = processedSuites % barLength;
            }
            const bar = `[${"#".repeat(filled)}${"-".repeat(barLength - filled)}]`;

            // Format total display
            const totalDisplay = totalSuites ? `/${totalSuites}` : "";
            const pctDisplay = totalSuites ? ` (${pct}%)` : "";

            // Clear line and write progress
            const suffix = suiteName ? ` - ${suiteName}` : "";
            process.stdout.write(`\r\x1b[K${bar} ${processedSuites}${totalDisplay}${pctDisplay} | ${elapsedStr}${remainingStr}${suffix}`);
          }
        }
      }
    }

    vitest.stdout.on("data", (chunk) => handleChunk(chunk));
    vitest.stderr.on("data", (chunk) => handleChunk(chunk, true));

    vitest.on("error", (err) => {
      process.stdout.write("\n❌ Benchmark process error.\n");
      reject(err);
    });

    vitest.on("close", (code) => {
      process.stdout.write("\n");
      try {
        const markdown = parseBenchmarkOutput(buffer);
        resolve(markdown);
      } catch (e) {
        reject(e);
      }
    });
  });
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

    // Detect suite headers (lines start with ✓ and end with duration like 1234ms)
    if (line.includes("✓") && /\b\d+ms\b/.test(line) && !line.trim().startsWith("·")) {
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
      // Extract just the benchmark file name (last segment ending with .bench.ts)
      const fileMatch = line.match(/([A-Za-z0-9_.-]+\.bench\.ts)\s*>/);
      if (fileMatch) {
        currentFile = fileMatch[1];
      } else if (!currentFile) {
        currentFile = 'unknown.bench.ts';
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

  // Compute total number of unique suites across all files for progress display
  let totalSuites = 0;
  for (const [file, benchmarks] of benchmarkGroups) {
    const seen = new Set();
    for (const b of benchmarks) {
      if (!seen.has(b.suite)) {
        seen.add(b.suite);
      }
    }
    totalSuites += seen.size;
  }

  // Running counter for suites
  const suiteIndexRef = { value: 0 };

  // Format grouped benchmarks with progress indicators
  for (const [file, benchmarks] of benchmarkGroups) {
    const result = formatBenchmarkFile(file, benchmarks, suiteIndexRef, totalSuites);
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
    "parser.bench.ts": "Vue file parsing and processing performance comparison with three distinct benchmarks:\n\n" +
      "- **parser**: Raw parsing to AST (Verter) vs full virtual code generation (Volar)\n" +
      "- **process**: Processing parsed AST into usable structures (Verter) vs extracting embedded codes (Volar)\n" +
      "- **parser + process**: End-to-end parsing and processing combined\n\n" +
      "Note: Verter uses a two-stage approach (parse AST → process), while Volar generates virtual TypeScript code directly during parsing.",
    "completions.bench.ts": "Tests Vue.js template completion performance in a simple component. Both use LSP+IPC architecture.",
    "real-world-components.bench.ts": "Measures completion performance in real-world Vue components with multiple edits and completion requests, simulating actual development workflows. Both use LSP+IPC architecture.",
  };
  return descriptions[file] || "Benchmark comparison between Volar and Verter.";
}

function formatBenchmarkFile(file, benchmarks, suiteIndexRef, totalSuites) {
  let section = `## ${file}\n\n`;
  section += `**Description:** ${getBenchmarkDescription(file)}\n\n`;
  // File-level progress summary
  const uniqueNames = new Set(benchmarks.map(b => b.suite));
  section += `Suites: ${uniqueNames.size}\n\n`;

  const comparisons = [];

  // Deduplicate benchmarks with same suite name
  const uniqueBenchmarks = new Map();
  for (const bench of benchmarks) {
    if (!uniqueBenchmarks.has(bench.suite)) {
      uniqueBenchmarks.set(bench.suite, bench);
    }
  }

  for (const bench of uniqueBenchmarks.values()) {
    // Progress bar + counter
    suiteIndexRef.value += 1;
    const current = suiteIndexRef.value;
    const pct = Math.round((current / totalSuites) * 100);
    const barLength = 20;
    const filled = Math.round((current / totalSuites) * barLength);
    const bar = `[${"#".repeat(filled)}${"-".repeat(barLength - filled)}] ${current}/${totalSuites} (${pct}%)`;
    section += `### ${bench.suite} ${bar}\n\n`;
    
    // Add context for parser.bench.ts suites
    if (file === "parser.bench.ts") {
      const suiteContext = getParserSuiteContext(bench.suite);
      if (suiteContext) {
        section += `${suiteContext}\n\n`;
      }
    }
    
    const result = formatBenchmarkTable(bench.suite, bench.data);
    section += result.markdown;
    if (result.comparison) {
      comparisons.push(result.comparison);
    }
  }

  return { markdown: section, comparisons };
}

function getParserSuiteContext(suiteName) {
  const lower = suiteName.toLowerCase();
  
  if (lower.includes("parser + process")) {
    return "**Full Pipeline:** Complete end-to-end operation including both parsing and processing steps.";
  } else if (lower.includes("process") && !lower.includes("parser")) {
    return "**Processing Only:** Takes already-parsed AST and processes it into usable structures. " +
           "Verter extracts script blocks and metadata; Volar extracts embedded code segments.";
  } else if (lower.includes("parser")) {
    return "**Parsing Only:** Raw parsing performance. " +
           "Verter parses Vue files into AST using OXC parser; Volar generates complete virtual TypeScript code.";
  }
  
  return null;
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
