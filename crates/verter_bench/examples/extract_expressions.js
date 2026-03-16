const fs = require("fs");
const path = require("path");
const { glob } = require("glob");
const { parse } = require("@vue/compiler-sfc");
const { baseParse } = require("@vue/compiler-core");

// Built-in Vue directives
const BUILT_IN_DIRECTIVES = new Set([
  "if",
  "else",
  "else-if",
  "for",
  "show",
  "bind",
  "on",
  "model",
  "slot",
  "pre",
  "cloak",
  "once",
  "memo",
  "html",
  "text",
]);

// JavaScript reserved words to exclude from identifiers
const JS_RESERVED = new Set([
  "true",
  "false",
  "null",
  "undefined",
  "NaN",
  "Infinity",
  "if",
  "else",
  "for",
  "while",
  "do",
  "switch",
  "case",
  "default",
  "break",
  "continue",
  "return",
  "throw",
  "try",
  "catch",
  "finally",
  "function",
  "class",
  "const",
  "let",
  "var",
  "new",
  "delete",
  "typeof",
  "instanceof",
  "in",
  "of",
  "void",
  "this",
  "super",
  "import",
  "export",
  "async",
  "await",
  "yield",
  "with",
  "debugger",
]);

/**
 * Convert UTF-16 offset to UTF-8 byte offset
 * @param {string} content - The full file content
 * @param {number} utf16Offset - UTF-16 code unit offset
 * @returns {number} UTF-8 byte offset
 */
function utf16ToUtf8Offset(content, utf16Offset) {
  if (utf16Offset <= 0) return 0;
  const substring = content.slice(0, utf16Offset);
  return Buffer.byteLength(substring, "utf8");
}

/**
 * Convert a location object with UTF-16 offsets to UTF-8 byte offsets
 * @param {string} content - The full file content
 * @param {{ start: number, end: number }} loc - Location with UTF-16 offsets
 * @returns {{ start: number, end: number }} Location with UTF-8 byte offsets
 */
function convertLocToUtf8(content, loc) {
  return {
    start: utf16ToUtf8Offset(content, loc.start),
    end: utf16ToUtf8Offset(content, loc.end),
  };
}

/**
 * Find the opening tag for a block by searching backwards from content start
 * @param {string} content - Full content
 * @param {number} contentOffset - Where the block content starts
 * @returns {{ start: number, end: number }} Start and end of opening tag
 */
function findOpeningTag(content, contentOffset) {
  // Search backwards from content offset to find the opening tag
  let pos = contentOffset - 1;
  while (pos >= 0 && content[pos] !== "<") {
    pos--;
  }

  const tagStart = pos;
  // The content starts after the >
  const tagEnd = contentOffset - 1;

  return { start: tagStart, end: tagEnd };
}

/**
 * Extract attributes from a block
 * @param {string} content - Full file content
 * @param {object} block - SFC block (script/template)
 * @returns {Array} Array of attribute objects
 */
function extractBlockAttributes(content, block) {
  if (!block || !block.attrs) return [];

  const attributes = [];
  const contentOffset = block.loc.start.offset;

  // Find the opening tag
  const { start: tagStart, end: tagEnd } = findOpeningTag(content, contentOffset);
  const tagContent = content.slice(tagStart, tagEnd + 1);

  for (const [name, value] of Object.entries(block.attrs)) {
    // Find attribute position in the opening tag
    let attrMatch;
    let attrRegex;

    if (value === true) {
      // Boolean attribute (no value) - match the attribute name followed by space or >
      attrRegex = new RegExp(`\\s(${escapeRegex(name)})(?=[\\s>/])`, "g");
    } else {
      // Attribute with value - match name="value" or name='value'
      attrRegex = new RegExp(
        `\\s(${escapeRegex(name)}\\s*=\\s*["']${escapeRegex(String(value))}["'])`,
        "g",
      );
    }

    attrMatch = attrRegex.exec(tagContent);

    const attr = {
      name,
      loc: { start: 0, end: 0 },
    };

    if (value !== true) {
      attr.value = String(value);
    }

    if (attrMatch && attrMatch.index !== undefined) {
      // +1 to skip the leading whitespace we matched
      const attrStart = tagStart + attrMatch.index + 1;
      const attrEnd = attrStart + attrMatch[1].length;
      attr.loc = convertLocToUtf8(content, { start: attrStart, end: attrEnd });
    }

    attributes.push(attr);
  }
  return attributes;
}

function escapeRegex(str) {
  return str.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/**
 * Get the type name for an expression
 * @param {string} directiveName - The directive name (without v-)
 * @param {string} argName - The argument name if any
 * @param {boolean} isArg - Whether this is an argument expression
 * @returns {string} The type name
 */
function getExpressionType(directiveName, argName, isArg) {
  if (isArg) return "arg";
  if (BUILT_IN_DIRECTIVES.has(directiveName)) {
    return directiveName;
  }
  return directiveName;
}

/**
 * Process template AST and extract all expressions
 * @param {object} ast - Template AST from compiler-core
 * @param {string} content - Full file content
 * @param {number} templateOffset - Offset where template content starts
 * @returns {Array} Array of expression objects
 */
function extractExpressionsFromTemplate(ast, content, templateOffset) {
  const expressions = [];

  function getAbsoluteLoc(loc) {
    return {
      start: templateOffset + loc.start.offset,
      end: templateOffset + loc.end.offset,
    };
  }

  function processNode(node, parentId) {
    const nodeId = node.loc ? templateOffset + node.loc.start.offset : parentId;

    switch (node.type) {
      case 1: // ELEMENT
        processElement(node, parentId);
        break;
      case 5: // INTERPOLATION
        processInterpolation(node, parentId);
        break;
      case 11: // FOR
        processFor(node, parentId);
        break;
      case 9: // IF
        processIf(node, parentId);
        break;
    }

    // Process children
    if (node.children) {
      for (const child of node.children) {
        processNode(child, nodeId);
      }
    }
  }

  function processElement(node, parentId) {
    const elementId = templateOffset + node.loc.start.offset;

    // Process props (directives and attributes)
    if (node.props) {
      for (const prop of node.props) {
        processProp(prop, elementId);
      }
    }
  }

  function processProp(prop, parentId) {
    const propId = templateOffset + prop.loc.start.offset;

    if (prop.type === 7) {
      // DIRECTIVE
      const directiveName = prop.name;
      const type = getExpressionType(directiveName, prop.arg?.content, false);

      // Process argument if it's dynamic
      if (prop.arg && !prop.arg.isStatic && prop.arg.content) {
        const argLoc = getAbsoluteLoc(prop.arg.loc);
        const argExpr = {
          id: utf16ToUtf8Offset(content, argLoc.start),
          type: "arg",
          loc: convertLocToUtf8(content, argLoc),
          parentId: utf16ToUtf8Offset(content, propId),
          expression: {
            content: prop.arg.content,
            loc: convertLocToUtf8(content, argLoc),
            identifiers: safeExtractIdentifiers(prop.arg),
          },
        };
        expressions.push(argExpr);
      }

      // Process expression
      if (prop.exp && prop.exp.content) {
        const expLoc = getAbsoluteLoc(prop.exp.loc);
        const expr = {
          id: utf16ToUtf8Offset(content, expLoc.start),
          type,
          loc: convertLocToUtf8(content, expLoc),
          parentId: utf16ToUtf8Offset(content, parentId),
          expression: {
            content: prop.exp.content,
            loc: convertLocToUtf8(content, expLoc),
            identifiers: safeExtractIdentifiers(prop.exp),
          },
        };
        expressions.push(expr);
      }
    } else if (prop.type === 6) {
      // ATTRIBUTE
      // Regular attributes with potential binding expressions
      if (prop.value && prop.value.content) {
        // Only process if it looks like an expression (for v-bind shorthand that wasn't caught)
        // Regular attributes don't have expressions
      }
    }
  }

  function processInterpolation(node, parentId) {
    if (!node.content) return;

    const interpLoc = getAbsoluteLoc(node.loc);
    const contentLoc = getAbsoluteLoc(node.content.loc);

    const expr = {
      id: utf16ToUtf8Offset(content, interpLoc.start),
      type: "interpolation",
      loc: convertLocToUtf8(content, interpLoc),
      parentId: utf16ToUtf8Offset(content, parentId),
      expression: {
        content: node.content.content,
        loc: convertLocToUtf8(content, contentLoc),
        identifiers: safeExtractIdentifiers(node.content),
      },
    };
    expressions.push(expr);
  }

  function processFor(node, parentId) {
    // v-for creates a special node structure
    if (node.source) {
      const forLoc = getAbsoluteLoc(node.loc);
      const sourceLoc = getAbsoluteLoc(node.source.loc);

      const expr = {
        id: utf16ToUtf8Offset(content, forLoc.start),
        type: "for",
        loc: convertLocToUtf8(content, forLoc),
        parentId: utf16ToUtf8Offset(content, parentId),
        expression: {
          content: node.source.content,
          loc: convertLocToUtf8(content, sourceLoc),
          identifiers: safeExtractIdentifiers(node.source),
        },
      };
      expressions.push(expr);
    }

    // Process the for body
    if (node.children) {
      for (const child of node.children) {
        processNode(child, templateOffset + node.loc.start.offset);
      }
    }
  }

  function processIf(node, parentId) {
    // v-if creates branches
    if (node.branches) {
      for (const branch of node.branches) {
        if (branch.condition) {
          const ifLoc = getAbsoluteLoc(branch.loc);
          const condLoc = getAbsoluteLoc(branch.condition.loc);
          const type = branch.isElse
            ? "else"
            : branch.condition
              ? expressions.some(
                  (e) => e.type === "if" && e.parentId === utf16ToUtf8Offset(content, parentId),
                )
                ? "else-if"
                : "if"
              : "else";

          const expr = {
            id: utf16ToUtf8Offset(content, ifLoc.start),
            type,
            loc: convertLocToUtf8(content, ifLoc),
            parentId: utf16ToUtf8Offset(content, parentId),
            expression: {
              content: branch.condition.content,
              loc: convertLocToUtf8(content, condLoc),
              identifiers: safeExtractIdentifiers(branch.condition),
            },
          };
          expressions.push(expr);
        }

        // Process branch children
        if (branch.children) {
          for (const child of branch.children) {
            processNode(child, templateOffset + branch.loc.start.offset);
          }
        }
      }
    }
  }

  // Start processing from root
  if (ast.children) {
    for (const child of ast.children) {
      processNode(child, templateOffset);
    }
  }

  return expressions;
}

/**
 * Remove string literals from an expression to avoid extracting identifiers from them
 * @param {string} expression - The expression string
 * @returns {string} Expression with strings replaced by placeholders
 */
function removeStringLiterals(expression) {
  // Remove template literals, double-quoted, and single-quoted strings
  return expression
    .replace(/`(?:[^`\\]|\\.)*`/g, '""') // Template literals
    .replace(/"(?:[^"\\]|\\.)*"/g, '""') // Double quotes
    .replace(/'(?:[^'\\]|\\.)*'/g, '""'); // Single quotes
}

/**
 * Extract identifiers from an expression string using simple tokenization
 * This extracts variable names that would need to be resolved from the component scope
 * @param {string} expression - The expression string
 * @returns {Array} Array of identifier strings
 */
function extractIdentifiersFromExpression(expression) {
  if (!expression || typeof expression !== "string") return [];

  // First remove string literals to avoid extracting identifiers from them
  const cleanedExpr = removeStringLiterals(expression);

  const identifiers = new Set();

  // Match potential identifiers: word characters that start with a letter or underscore
  // but not preceded by a dot (to exclude property access like obj.prop)
  const idPattern = /(?<![.\w$])([a-zA-Z_$][a-zA-Z0-9_$]*)/g;

  let match;
  while ((match = idPattern.exec(cleanedExpr)) !== null) {
    const id = match[1];
    // Exclude JavaScript reserved words and common globals
    if (!JS_RESERVED.has(id)) {
      identifiers.add(id);
    }
  }

  return Array.from(identifiers);
}

/**
 * Safely extract identifiers from an expression node
 * @param {object} node - AST node with expression
 * @returns {Array} Array of identifier strings
 */
function safeExtractIdentifiers(node) {
  try {
    if (!node) return [];
    const content = node.content || "";
    return extractIdentifiersFromExpression(content);
  } catch (e) {
    // If extraction fails, return empty array
    return [];
  }
}

/**
 * Process a single Vue file
 * @param {string} filePath - Path to the Vue file
 * @returns {{ script: object|null, expressions: object|null }}
 */
function processVueFile(filePath) {
  const content = fs.readFileSync(filePath, "utf8");
  const { descriptor, errors } = parse(content, { filename: filePath });

  if (errors.length > 0) {
    console.warn(`Warnings parsing ${filePath}:`, errors);
  }

  let scriptResult = null;
  let expressionResult = null;

  // Process script block (prefer setup script)
  const scriptBlock = descriptor.scriptSetup || descriptor.script;
  if (scriptBlock) {
    const lang = scriptBlock.lang || "js";
    const loc = {
      start: scriptBlock.loc.start.offset,
      end: scriptBlock.loc.end.offset,
    };

    scriptResult = {
      path: filePath,
      lang,
      loc: convertLocToUtf8(content, loc),
      attributes: extractBlockAttributes(content, scriptBlock),
      content: scriptBlock.content,
    };
  }

  // Process template block
  if (descriptor.template) {
    const templateContent = descriptor.template.content;
    const templateOffset = descriptor.template.loc.start.offset;

    // Find where the actual template content starts (after the opening tag)
    const templateTagMatch = content.slice(templateOffset).match(/^<template[^>]*>/);
    const contentOffset = templateOffset + (templateTagMatch ? templateTagMatch[0].length : 0);

    // Parse template with compiler-core for detailed AST
    const templateAst = baseParse(templateContent, {
      getTextMode: () => 0,
      isVoidTag: (tag) =>
        [
          "area",
          "base",
          "br",
          "col",
          "embed",
          "hr",
          "img",
          "input",
          "link",
          "meta",
          "param",
          "source",
          "track",
          "wbr",
        ].includes(tag),
      isPreTag: (tag) => tag === "pre",
    });

    const lang = descriptor.template.lang || "html";
    const loc = {
      start: descriptor.template.loc.start.offset,
      end: descriptor.template.loc.end.offset,
    };

    const expressions = extractExpressionsFromTemplate(templateAst, content, contentOffset);

    expressionResult = {
      path: filePath,
      lang,
      loc: convertLocToUtf8(content, loc),
      attributes: extractBlockAttributes(content, descriptor.template),
      expressions,
    };
  }

  return { script: scriptResult, expressions: expressionResult };
}

/**
 * Main function to process all Vue files in a directory
 * @param {string} inputGlob - Glob pattern for input files
 * @param {string} outputDir - Output directory for JSON files
 */
async function main(inputGlob, outputDir) {
  console.log(`Searching for Vue files matching: ${inputGlob}`);

  const files = await glob(inputGlob, {
    nodir: true,
    windowsPathsNoEscape: true,
  });

  console.log(`Found ${files.length} Vue files`);

  const scripts = [];
  const expressions = [];

  for (const file of files) {
    console.log(`Processing: ${file}`);
    try {
      const result = processVueFile(file);
      if (result.script) {
        scripts.push(result.script);
      }
      if (result.expressions) {
        expressions.push(result.expressions);
      }
    } catch (e) {
      console.error(`Error processing ${file}:`, e.message);
    }
  }

  // Ensure output directory exists
  fs.mkdirSync(outputDir, { recursive: true });

  // Write script.json
  const scriptPath = path.join(outputDir, "script.json");
  fs.writeFileSync(scriptPath, JSON.stringify(scripts, null, 2));
  console.log(`Wrote ${scripts.length} script entries to ${scriptPath}`);

  // Write expressions.json
  const expressionsPath = path.join(outputDir, "expressions.json");
  fs.writeFileSync(expressionsPath, JSON.stringify(expressions, null, 2));
  console.log(`Wrote ${expressions.length} expression entries to ${expressionsPath}`);
}

// CLI usage
const args = process.argv.slice(2);
if (args.length < 1) {
  console.log("Usage: node extract_expressions.js <input-glob> [output-dir]");
  console.log('Example: node extract_expressions.js "/path/to/project/**/*.vue" ./expressions/source');
  process.exit(1);
}

const inputGlob = args[0];
const outputDir = args[1] || path.join(__dirname, "expressions", "source");

main(inputGlob, outputDir).catch((e) => {
  console.error("Fatal error:", e);
  process.exit(1);
});
