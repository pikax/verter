/**
 * Vue Codegen Script (Official Compiler)
 *
 * This script reads .vue files from examples/codegen/source/ directory,
 * compiles them using Vue's official compiler (like vite-plugin-vue does),
 * and outputs BOTH production and development builds:
 *   - {filename}.dev.vue.js  - Development build
 *   - {filename}.prod.vue.js - Production build
 *
 * This allows comparison between verter's codegen output and Vue's official output.
 *
 * Usage: node examples/codegen.js
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

/**
 * Generate 8-character SHA-256 hash (same as vite-plugin-vue)
 * @param {string} text
 * @returns {string}
 */
function getHash(text) {
  return crypto.createHash("sha256").update(text).digest("hex").substring(0, 8);
}

/**
 * Generate component ID following vite-plugin-vue's strategy:
 * - Development: hash(filepath + source)
 * - Production: hash(filepath)
 *
 * @param {string} filepath
 * @param {string} source
 * @param {boolean} isProd
 * @returns {string}
 */
function generateComponentId(filepath, source, isProd) {
  const normalizedPath = filepath.replace(/\\/g, "/");
  if (isProd) {
    return getHash(normalizedPath);
  } else {
    return getHash(normalizedPath + source);
  }
}

// ============================================================================
// Compilation Options (mirrors vite-plugin-vue)
// ============================================================================

/**
 * Create compile options for a specific mode
 * @param {boolean} isProd
 * @returns {object}
 */
function createCompileOptions(isProd) {
  return {
    isProd,
    sourceMap: !isProd,
    features: {
      optionsAPI: true,
      propsDestructure: true,
    },
  };
}

// ============================================================================
// Main Compilation
// ============================================================================

const CODEGEN_DIR = path.join(__dirname, "codegen");
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
  console.log("  cargo run --example codegen");
  process.exit(0);
}

console.log(`Found ${vueFiles.length} .vue file(s) to process`);
console.log("Generating both development and production builds...\n");

/**
 * Compile a Vue SFC file
 * @param {string} source - SFC source code
 * @param {string} filename - File name
 * @param {string} filepath - Full file path
 * @param {boolean} isProd - Production mode
 * @returns {Promise<{output: string, id: string, errors: string[]}>}
 */
async function compileVueSFC(source, filename, filepath, isProd) {
  const errors = [];
  const options = createCompileOptions(isProd);

  // Generate component ID (vite-plugin-vue style)
  const id = generateComponentId(filepath, source, isProd);

  // Parse the SFC
  const { descriptor, errors: parseErrors } = parse(source, {
    filename,
    sourceMap: options.sourceMap,
  });

  if (parseErrors.length > 0) {
    errors.push(...parseErrors.map((e) => `Parse: ${e.message}`));
    return { output: "", id, errors };
  }

  // Check for scoped styles
  const hasScoped = descriptor.styles.some((s) => s.scoped);

  let output = "";

  // Production: inline template into script setup
  // Development: separate render function (for HMR)
  const inlineTemplate = isProd && descriptor.template;

  // Compile script if present
  if (descriptor.script || descriptor.scriptSetup) {
    try {
      const scriptResult = compileScript(descriptor, {
        id,
        isProd: options.isProd,
        sourceMap: options.sourceMap,
        inlineTemplate,
        templateOptions: inlineTemplate
          ? {
              scoped: hasScoped,
              compilerOptions: {
                scopeId: hasScoped ? `data-v-${id}` : undefined,
              },
            }
          : undefined,
        propsDestructure: options.features.propsDestructure,
        hoistStatic: options.isProd,
      });

      output += `// Script (${isProd ? "PRODUCTION" : "DEVELOPMENT"})${inlineTemplate ? " [inline template]" : ""}\n`;
      output += `${scriptResult.content}\n\n`;
    } catch (scriptErr) {
      errors.push(`Script: ${scriptErr.message}`);
    }
  }

  // Compile template separately only in development mode (or when no script)
  if (descriptor.template && !inlineTemplate) {
    try {
      // Get bindings from script for better template compilation
      let bindings;
      if (descriptor.script || descriptor.scriptSetup) {
        try {
          const scriptForBindings = compileScript(descriptor, {
            id,
            isProd: options.isProd,
          });
          bindings = scriptForBindings.bindings;
        } catch {
          // Ignore binding extraction errors
        }
      }

      const templateResult = compileTemplate({
        source: descriptor.template.content,
        filename,
        id,
        isProd: options.isProd,
        ssr: false,
        sourceMap: options.sourceMap,
        scoped: hasScoped,
        compilerOptions: {
          mode: "module",
          sourceMap: options.sourceMap,
          bindingMetadata: bindings,
          scopeId: hasScoped ? `data-v-${id}` : undefined,
        },
      });

      if (templateResult.errors && templateResult.errors.length > 0) {
        errors.push(...templateResult.errors.map((e) => `Template: ${e.message}`));
      } else {
        output += `// Template render function (${isProd ? "PRODUCTION" : "DEVELOPMENT"})\n`;
        output += `${templateResult.code}\n`;

        // Append inline source map if available (dev only)
        if (templateResult.map && options.sourceMap) {
          const sourceMapBase64 = Buffer.from(JSON.stringify(templateResult.map)).toString(
            "base64",
          );
          output += `\n//# sourceMappingURL=data:application/json;charset=utf-8;base64,${sourceMapBase64}`;
        }
      }
    } catch (templateErr) {
      errors.push(`Template: ${templateErr.message}`);
    }
  }

  // Compile styles if present
  if (descriptor.styles.length > 0) {
    const compiledStyles = [];
    const cssModules = {};

    for (let i = 0; i < descriptor.styles.length; i++) {
      const style = descriptor.styles[i];
      const isModule = !!style.module;

      try {
        // Use async for CSS Modules, sync otherwise
        const styleOptions = {
          source: style.content,
          filename,
          id,
          scoped: style.scoped,
          isProd: options.isProd,
          modules: isModule,
          preprocessLang: style.lang,
        };

        const styleResult = isModule
          ? await compileStyleAsync(styleOptions)
          : compileStyle(styleOptions);

        if (styleResult.errors && styleResult.errors.length > 0) {
          errors.push(...styleResult.errors.map((e) => `Style[${i}]: ${e.message}`));
        } else {
          compiledStyles.push({
            code: styleResult.code,
            scoped: style.scoped,
            module: style.module,
            lang: style.lang,
          });

          // Capture CSS Module exports
          if (style.module && styleResult.modules) {
            const moduleName = style.module === true ? "$style" : style.module;
            cssModules[moduleName] = styleResult.modules;
          }
        }
      } catch (styleErr) {
        errors.push(`Style[${i}]: ${styleErr.message}`);
      }
    }

    // Output compiled styles
    if (compiledStyles.length > 0) {
      output += `\n// Styles (${isProd ? "PRODUCTION" : "DEVELOPMENT"})\n`;
      output += `export const __css__ = [\n`;
      for (const compiled of compiledStyles) {
        const meta = [];
        if (compiled.scoped) meta.push("scoped");
        if (compiled.module) meta.push("module");
        if (compiled.lang) meta.push(compiled.lang);
        const metaStr = meta.length > 0 ? ` /* ${meta.join(", ")} */` : "";
        output += `  ${JSON.stringify(compiled.code)},${metaStr}\n`;
      }
      output += `];\n`;

      // Output CSS Module mappings
      if (Object.keys(cssModules).length > 0) {
        output += `\n// CSS Modules\n`;
        output += `export const __cssModules__ = ${JSON.stringify(cssModules, null, 2)};\n`;
      }
    }
  }

  return { output, id, errors };
}

// Process each file (async)
async function processFiles() {
  for (const file of vueFiles) {
    const filePath = path.join(SOURCE_DIR, file);
    const baseName = path.basename(file, ".vue");
    const devOutputPath = path.join(GENERATED_DIR, `${baseName}.dev.vue.js`);
    const prodOutputPath = path.join(GENERATED_DIR, `${baseName}.prod.vue.js`);

    console.log(`Processing: ${file}`);

    try {
      const source = fs.readFileSync(filePath, "utf-8");

      // Compile development build
      const devResult = await compileVueSFC(source, file, filePath, false);
      if (devResult.errors.length > 0) {
        console.error(`  DEV errors:`);
        devResult.errors.forEach((e) => console.error(`    - ${e}`));
      }

      // Compile production build
      const prodResult = await compileVueSFC(source, file, filePath, true);
      if (prodResult.errors.length > 0) {
        console.error(`  PROD errors:`);
        prodResult.errors.forEach((e) => console.error(`    - ${e}`));
      }

      // Write outputs
      if (devResult.output) {
        fs.writeFileSync(devOutputPath, devResult.output);
        console.log(`  -> DEV:  ${devOutputPath}`);
        console.log(`           ID: ${devResult.id}, ${devResult.output.length} bytes`);
      }

      if (prodResult.output) {
        fs.writeFileSync(prodOutputPath, prodResult.output);
        console.log(`  -> PROD: ${prodOutputPath}`);
        console.log(`           ID: ${prodResult.id}, ${prodResult.output.length} bytes`);
      }

      // Show ID difference
      if (devResult.id !== prodResult.id) {
        console.log(`  (IDs differ: dev includes source in hash)`);
      }
    } catch (err) {
      console.error(`  Error processing ${file}: ${err.message}`);
    }
  }

  console.log("\nDone!");
  console.log("\nYou can now compare:");
  console.log("  - Verter output: examples/codegen/generated/{name}.js");
  console.log("  - Vue DEV:       examples/codegen/generated/{name}.dev.vue.js");
  console.log("  - Vue PROD:      examples/codegen/generated/{name}.prod.vue.js");
}

processFiles().catch(console.error);
