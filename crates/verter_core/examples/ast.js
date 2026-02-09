/**
 * Vue Template AST Generator
 *
 * This script reads .vue files from the source/ directory,
 * parses them using Vue's official compiler, and outputs
 * the AST to generated/{filename}.json
 *
 * Usage: node script.js
 *
 * Requirements: npm install @vue/compiler-sfc
 */

const fs = require("fs");
const path = require("path");
const { baseParse } = require("@vue/compiler-core");

const AST_DIR = path.join(__dirname, "ast");
const SOURCE_DIR = path.join(AST_DIR, "source");
const GENERATED_DIR = path.join(AST_DIR, "generated");

// Ensure generated directory exists
if (!fs.existsSync(GENERATED_DIR)) {
  fs.mkdirSync(GENERATED_DIR, { recursive: true });
}
/**
 * Recursively sort all object keys alphabetically
 * Uses a WeakSet to handle circular references
 */
function sortObjectKeys(obj, seen = new WeakSet()) {
  if (obj === null || typeof obj !== "object") {
    return obj;
  }

  // Handle circular references
  if (seen.has(obj)) {
    return "[Circular]";
  }
  seen.add(obj);

  if (Array.isArray(obj)) {
    return obj.map((item) => sortObjectKeys(item, seen));
  }

  const sorted = {};
  const keys = Object.keys(obj).sort();
  for (const key of keys) {
    sorted[key] = sortObjectKeys(obj[key], seen);
  }
  return sorted;
}

// Get all .vue files from source directory
const vueFiles = fs
  .readdirSync(SOURCE_DIR)
  .filter((file) => file.endsWith(".vue"));

if (vueFiles.length === 0) {
  console.log("No .vue files found in source/ directory");
  process.exit(0);
}

console.log(`Found ${vueFiles.length} .vue file(s) to process\n`);

for (const file of vueFiles) {
  const filePath = path.join(SOURCE_DIR, file);
  const baseName = path.basename(file, ".vue");
  const outputPath = path.join(GENERATED_DIR, `${baseName}.json`);

  console.log(`Processing: ${file}`);

  try {
    const source = fs.readFileSync(filePath, "utf-8");
    const ast = baseParse(source, {
      filename: file,
      sourceMap: false,
      templateParseOptions: {
        // whitespace is not implemented in verter core yet, but set it to preserve for future compatibility
        whitespace: "preserve",
      },
    });

    // if (errors.length > 0) {
    //   console.error(`  Errors parsing ${file}:`);
    //   errors.forEach((err) => console.error(`    - ${err.message}`));
    //   continue;
    // }

    const result = ast;

    // Sort keys alphabetically for easier comparison
    // const sortedResult = sortObjectKeys(result);
    const sortedResult = result;
    fs.writeFileSync(outputPath, JSON.stringify(sortedResult, null, 2));
    console.log(`  -> ${outputPath}`);
  } catch (err) {
    console.error(`  Error processing ${file}: ${err.message}`);
  }
}

console.log("\nDone!");
