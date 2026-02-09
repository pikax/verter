/**
 * AST Diff Generator
 *
 * This script compares the Vue official compiler AST with Verter's AST output
 * For each file in the generated/ directory, it runs json-diff between:
 * - {filename}.json (official Vue AST)
 * - {filename}.verter.json (Verter's transformed AST)
 *
 * Output: {filename}.diff (contains diff for each file)
 *
 * Usage: node ast.diff.js
 *
 * Requirements: npm install json-diff
 */

const fs = require("fs");
const path = require("path");
const { diffString } = require("json-diff");

const AST_DIR = path.join(__dirname, "ast");
const GENERATED_DIR = path.join(AST_DIR, "generated");

/**
 * Get base filename without extension
 * Filters out .json and .verter.json to get unique filenames
 */
function getUniqueFileNames() {
  const files = fs.readdirSync(GENERATED_DIR);
  const uniqueNames = new Set();

  for (const file of files) {
    if (file.endsWith(".json") && !file.endsWith(".verter.json")) {
      const baseName = file.replace(".json", "");
      uniqueNames.add(baseName);
    }
  }

  return Array.from(uniqueNames).sort();
}

/**
 * Load and parse JSON file
 */
function loadJSON(filePath) {
  try {
    const content = fs.readFileSync(filePath, "utf-8");
    return JSON.parse(content);
  } catch (error) {
    console.error(`Error loading ${filePath}:`, error.message);
    return null;
  }
}

/**
 * Run diff on a single file pair using json-diff library
 */
function diffFile(baseName) {
  const jsonPath = path.join(GENERATED_DIR, `${baseName}.json`);
  const verterPath = path.join(GENERATED_DIR, `${baseName}.verter.json`);
  const diffPath = path.join(GENERATED_DIR, `${baseName}.diff`);

  // Check if both files exist
  if (!fs.existsSync(jsonPath)) {
    console.warn(`⚠️  Missing: ${baseName}.json`);
    return false;
  }

  if (!fs.existsSync(verterPath)) {
    console.warn(`⚠️  Missing: ${baseName}.verter.json`);
    return false;
  }

  // Load both JSON files
  const official = loadJSON(jsonPath);
  const verter = loadJSON(verterPath);

  if (official === null || verter === null) {
    console.error(`❌ Failed to load files for ${baseName}`);
    return false;
  }

  try {
    // Use diffString with sort option to match CLI output
    const diffOutput = diffString(official, verter, {
      sort: true,
      color: false,
      excludeKeys: ['parsed']
    });

    // Write diff to file
    fs.writeFileSync(diffPath, diffOutput);

    // Check if there are differences
    const hasDifferences = diffOutput.trim().length > 0;

    if (hasDifferences) {
      console.log(
        `📝 ${baseName}: Has differences (written to ${baseName}.diff)`,
      );
    } else {
      console.log(`✅ ${baseName}: Identical`);
    }
    return true;
  } catch (error) {
    console.error(`❌ Error running diff for ${baseName}:`, error.message);
    return false;
  }
}

/**
 * Main execution
 */
function main() {
  console.log(`🔍 Scanning ${GENERATED_DIR} for AST files...\n`);

  const fileNames = getUniqueFileNames();

  if (fileNames.length === 0) {
    console.log("No AST files found in generated directory");
    return;
  }

  console.log(`Found ${fileNames.length} file pair(s):\n`);

  let successCount = 0;
  for (const fileName of fileNames) {
    if (diffFile(fileName)) {
      successCount++;
    }
  }

  console.log(
    `\n✨ Completed: ${successCount}/${fileNames.length} files processed`,
  );
}

main();
