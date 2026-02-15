/**
 * Vue Codegen Performance Check Script
 *
 * This script reads .vue files from examples/check/source/ directory,
 * compiles them using Vue's official compiler (matching @vitejs/plugin-vue),
 * and outputs per-block files for development, production, and SSR builds:
 *   - {name}.script.{mode}.vue.js   - Compiled script
 *   - {name}.render.{mode}.vue.js   - Compiled render function
 *   - {name}.style{n}.{mode}.vue.js - Compiled styles
 *   - {name}.{block}.{mode}.vue.js  - Custom blocks
 *
 * Where {mode} is dev, prod, or ssr.
 *
 * Optimized for performance benchmarking (silent errors, no source maps).
 *
 * Usage: node examples/check.js
 *
 * Requirements: npm install @vue/compiler-sfc
 */

const fs = require("fs");
const path = require("path");
const crypto = require("crypto");

// Try to load Vue compiler
let compileTemplate, compileScript, compileStyle, compileStyleAsync, parse;
try {
  const sfc = require("@vue/compiler-sfc");
  compileTemplate = sfc.compileTemplate;
  compileScript = sfc.compileScript;
  compileStyle = sfc.compileStyle;
  compileStyleAsync = sfc.compileStyleAsync;
  parse = sfc.parse;
} catch (e) {
  console.error("Error: @vue/compiler-sfc not found.");
  console.error("Please install it with: npm install @vue/compiler-sfc");
  process.exit(1);
}

// ============================================================================
// Component ID Generation (mirrors vite-plugin-vue)
// ============================================================================

function getHash(text) {
  return crypto.createHash("sha256").update(text).digest("hex").substring(0, 8);
}

function generateComponentId(filepath, source, isProd) {
  const normalizedPath = filepath.replace(/\\/g, "/");
  if (isProd) {
    return getHash(normalizedPath);
  } else {
    return getHash(normalizedPath + source);
  }
}

// ============================================================================
// Helpers matching @vitejs/plugin-vue behaviour
// ============================================================================

const scriptIdentifier = "_sfc_main";

/**
 * Matches isUseInlineTemplate() in @vitejs/plugin-vue/src/script.ts
 * In production, inline template into script setup (no separate render fn).
 */
function isUseInlineTemplate(descriptor, isProd) {
  return (
    isProd && // production only (no devServer)
    !!descriptor.scriptSetup && // must have <script setup>
    !descriptor.template?.src // template not from external src
  );
}

/**
 * Matches canInlineMain() in @vitejs/plugin-vue/src/script.ts
 * Determines if script code can be placed directly in the main module.
 */
function canInlineMain(descriptor) {
  if (descriptor.script?.src || descriptor.scriptSetup?.src) {
    return false;
  }
  const lang = descriptor.script?.lang || descriptor.scriptSetup?.lang;
  if (!lang || lang === "js" || lang === "ts") {
    return true;
  }
  return false;
}

/**
 * Build template compiler options matching resolveTemplateCompilerOptions()
 * in @vitejs/plugin-vue/src/template.ts
 */
function resolveTemplateCompilerOptions(descriptor, filename, isProd, ssr, resolvedScript) {
  const block = descriptor.template;
  if (!block) return undefined;

  const hasScoped = descriptor.styles.some((s) => s.scoped);
  const id = descriptor.id;

  // if using TS, support TS syntax in template expressions
  const expressionPlugins = [];
  const lang = descriptor.scriptSetup?.lang || descriptor.script?.lang;
  if (lang && /tsx?$/.test(lang) && !expressionPlugins.includes("typescript")) {
    expressionPlugins.push("typescript");
  }

  return {
    id,
    filename,
    scoped: hasScoped,
    slotted: descriptor.slotted,
    isProd,
    ssr,
    ssrCssVars: descriptor.cssVars,
    sourceMap: false,
    compilerOptions: {
      scopeId: hasScoped ? `data-v-${id}` : undefined,
      bindingMetadata: resolvedScript ? resolvedScript.bindings : undefined,
      expressionPlugins,
      sourceMap: false,
    },
  };
}

// ============================================================================
// Main Compilation
// ============================================================================

const CODEGEN_DIR = path.join(__dirname, "check");
const SOURCE_DIR = path.join(CODEGEN_DIR, "source");
const GENERATED_DIR = path.join(CODEGEN_DIR, "generated");

// Ensure directories exist
if (!fs.existsSync(SOURCE_DIR)) {
  fs.mkdirSync(SOURCE_DIR, { recursive: true });
}
if (!fs.existsSync(GENERATED_DIR)) {
  fs.mkdirSync(GENERATED_DIR, { recursive: true });
}

// Get all .vue files from source directory
const vueFiles = fs.existsSync(SOURCE_DIR)
  ? fs.readdirSync(SOURCE_DIR).filter((file) => file.endsWith(".vue"))
  : [];

if (vueFiles.length === 0) {
  console.log("No .vue files found in source/ directory");
  console.log("Run the Rust example first to create sample files:");
  console.log("  cargo run --example check");
  process.exit(0);
}

console.log(`Found ${vueFiles.length} .vue file(s) to process`);
console.log("Generating per-block dev, prod, and SSR builds...\n");

/**
 * Compile a Vue SFC file and return individual blocks.
 * Mirrors the compilation flow from @vitejs/plugin-vue:
 *   1. resolveScript() with inlineTemplate + genDefaultAs options
 *   2. compileTemplate() with bindingMetadata from resolved script
 *
 * @param {string} source
 * @param {string} filename
 * @param {string} filepath
 * @param {boolean} isProd
 * @param {boolean} [ssr=false]
 * @returns {Promise<{blocks: Record<string, string>, id: string}>}
 */
async function compileVueSFC(source, filename, filepath, isProd, ssr = false) {
  const id = generateComponentId(filepath, source, isProd);
  /** @type {Record<string, string>} */
  const blocks = {};

  // Parse the SFC (no source maps for performance)
  const { descriptor, errors: parseErrors } = parse(source, {
    filename,
    sourceMap: false,
  });

  if (parseErrors.length > 0) {
    return { blocks, id };
  }

  // Set descriptor.id (parse doesn't set it, but compileScript needs it)
  descriptor.id = id;

  const hasScoped = descriptor.styles.some((s) => s.scoped);
  const inlineTemplate = isUseInlineTemplate(descriptor, isProd);

  // Step 1: Compile script (mirrors resolveScript in plugin-vue)
  let resolvedScript = null;
  if (descriptor.script || descriptor.scriptSetup) {
    try {
      resolvedScript = compileScript(descriptor, {
        id,
        isProd,
        sourceMap: false,
        // Inline template into setup for production <script setup> (like plugin-vue)
        inlineTemplate,
        // Pass template compiler options when inlining (like plugin-vue)
        templateOptions: resolveTemplateCompilerOptions(descriptor, filename, isProd, ssr, null),
        // genDefaultAs makes compiler output `const _sfc_main = ...` instead of
        // `export default ...` — matches canInlineMain() in plugin-vue
        genDefaultAs: canInlineMain(descriptor) ? scriptIdentifier : undefined,
        propsDestructure: true,
      });
      blocks["script"] = resolvedScript.content;
    } catch {
      // Silent for performance benchmark
    }
  }

  // Step 2: Compile template separately only in dev mode / non-inline
  // (In production with <script setup>, template is already inlined in script)
  if (descriptor.template && !inlineTemplate) {
    try {
      // Build template options matching resolveTemplateCompilerOptions()
      // Pass bindingMetadata from the resolved script for correct prefixing
      const templateOpts = resolveTemplateCompilerOptions(
        descriptor,
        filename,
        isProd,
        ssr,
        resolvedScript,
      );

      const templateResult = compileTemplate({
        ...templateOpts,
        source: descriptor.template.content,
      });

      if (!templateResult.errors || templateResult.errors.length === 0) {
        blocks["render"] = templateResult.code;
      }
    } catch {
      // Silent for performance benchmark
    }
  }

  // Compile styles
  for (let i = 0; i < descriptor.styles.length; i++) {
    const style = descriptor.styles[i];
    const isModule = !!style.module;

    try {
      const styleOptions = {
        source: style.content,
        filename,
        id,
        scoped: style.scoped,
        isProd,
        modules: isModule,
        preprocessLang: style.lang,
      };

      const styleResult = isModule
        ? await compileStyleAsync(styleOptions)
        : compileStyle(styleOptions);

      if (!styleResult.errors || styleResult.errors.length === 0) {
        blocks[`style${i}`] = styleResult.code;
      }
    } catch {
      // Silent for performance benchmark
    }
  }

  // Custom blocks
  for (const block of descriptor.customBlocks) {
    blocks[block.type] = block.content;
  }

  return { blocks, id };
}

/**
 * Write blocks to individual files: {baseName}.{block}.{mode}.vue.js
 */
function writeBlocks(baseName, blocks, mode) {
  let totalSize = 0;
  for (const [block, content] of Object.entries(blocks)) {
    const outputPath = path.join(GENERATED_DIR, `${baseName}.${block}.${mode}.vue.js`);
    fs.writeFileSync(outputPath, content);
    totalSize += content.length;
  }
  return totalSize;
}

// Process files (async)
async function processFiles() {
  const start = Date.now();
  let erroredFiles = 0;
  let totalDevSize = 0;
  let totalProdSize = 0;
  let totalSsrSize = 0;

  for (const file of vueFiles) {
    const filePath = path.join(SOURCE_DIR, file);
    const baseName = path.basename(file, ".vue");

    try {
      const source = fs.readFileSync(filePath, "utf-8");

      // Development build
      const devResult = await compileVueSFC(source, file, filePath, false);
      if (Object.keys(devResult.blocks).length > 0) {
        totalDevSize += writeBlocks(baseName, devResult.blocks, "dev");
      } else {
        erroredFiles++;
      }

      // Production build
      const prodResult = await compileVueSFC(source, file, filePath, true);
      if (Object.keys(prodResult.blocks).length > 0) {
        totalProdSize += writeBlocks(baseName, prodResult.blocks, "prod");
      }

      // SSR build
      const ssrResult = await compileVueSFC(source, file, filePath, false, true);
      if (Object.keys(ssrResult.blocks).length > 0) {
        totalSsrSize += writeBlocks(baseName, ssrResult.blocks, "ssr");
      }
    } catch {
      erroredFiles++;
    }
  }

  const elapsed = Date.now() - start;

  console.log(`Done! ${elapsed}ms`);
  console.log(`  DEV total:  ${totalDevSize} bytes`);
  console.log(`  PROD total: ${totalProdSize} bytes`);
  console.log(`  SSR total:  ${totalSsrSize} bytes`);
  console.log(`  Errors: ${erroredFiles} file(s)`);
  console.log("\nOutput format: {name}.{block}.{dev|prod|ssr}.vue.js");
  console.log("  Blocks: script, render, style0..styleN, <custom>");
}

processFiles().catch(console.error);
