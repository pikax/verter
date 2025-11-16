import fs from "fs";
import path from "path";
import ts from "typescript";
import { fileURLToPath } from "url";
import { createRequire } from "module";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const require = createRequire(import.meta.url);

// Output configuration (renamed to vue.macros.ts)
const OUTPUT_FILE = path.join(__dirname, "../src/vue/vue.macros.ts");

// Targets to generate from Vue's actual types
const FUNCTIONS_TO_OVERRIDE = [
  "defineProps",
  "withDefaults",
  "defineEmits",
  "defineOptions",
  "defineModel",
  "defineExpose",
  "defineSlots",
];

const NAME_APPEND = "_Box";
const NAME_PREPEND = "";

function resolveVueTypeFiles() {
  const files = [];
  const candidates = [
    "@vue/runtime-core/dist/runtime-core.d.ts",
    "@vue/runtime-dom/dist/runtime-dom.d.ts",
    "@vue/shared/dist/shared.d.ts",
    "vue/dist/vue.d.ts",
  ];
  for (const id of candidates) {
    try {
      const p = require.resolve(id);
      if (fs.existsSync(p)) files.push(p);
    } catch {}
  }

  // Also scan pnpm store to ensure we include all Vue type files
  const pnpmRoot = path.resolve(__dirname, "../../..", "node_modules/.pnpm");
  if (fs.existsSync(pnpmRoot)) {
    const walk = (dir) => {
      const entries = fs.readdirSync(dir, { withFileTypes: true });
      for (const e of entries) {
        const p = path.join(dir, e.name);
        if (e.isDirectory()) {
          // Traverse all; we'll filter on files below
          walk(p);
        } else if (e.isFile()) {
          if (
            e.name === "runtime-core.d.ts" &&
            p.includes(
              `${path.sep}@vue${path.sep}runtime-core${path.sep}dist${path.sep}runtime-core.d.ts`
            )
          ) {
            files.push(p);
          }
          if (
            e.name === "shared.d.ts" &&
            p.includes(
              `${path.sep}@vue${path.sep}shared${path.sep}dist${path.sep}shared.d.ts`
            )
          ) {
            files.push(p);
          }
        }
      }
    };
    walk(pnpmRoot);
  }

  files.sort();
  return Array.from(new Set(files));
}

function extractFunctionDeclarations(files, names) {
  const printer = ts.createPrinter({ newLine: ts.NewLineKind.LineFeed });
  const out = new Map();
  for (const n of names) out.set(n, []);

  for (const filePath of files) {
    const source = fs.readFileSync(filePath, "utf-8");
    const sf = ts.createSourceFile(
      filePath,
      source,
      ts.ScriptTarget.Latest,
      true
    );
    const visit = (node) => {
      if (
        ts.isFunctionDeclaration(node) &&
        node.name &&
        names.includes(node.name.text)
      ) {
        // Get JSDoc comments if they exist
        const jsDocComments = ts.getJSDocCommentsAndTags(node);
        let jsDocText = "";
        if (jsDocComments.length > 0) {
          // Get the full text including JSDoc
          const start = jsDocComments[0].pos;
          const end = node.end;
          jsDocText = source.substring(start, end).trim();
        }

        const text = printer
          .printNode(ts.EmitHint.Unspecified, node, sf)
          .trim();
        out.get(node.name.text).push({ text, node, sf, jsDocText });
      }
      ts.forEachChild(node, visit);
    };
    visit(sf);
  }

  return out;
}

function collectVueTypeNames(files) {
  const exportedTypes = new Map(); // name -> package
  const internalTypes = new Set();
  const exportedInVue = new Set(); // names exported directly by 'vue'

  // Priority order: prefer more specific packages over 'vue'
  const pkgPriority = { "@vue/shared": 3, "@vue/runtime-core": 2, vue: 1 };

  for (const filePath of files) {
    const source = fs.readFileSync(filePath, "utf-8");
    const sf = ts.createSourceFile(
      filePath,
      source,
      ts.ScriptTarget.Latest,
      true
    );

    // Determine package from file path
    let pkg = "vue";
    let priority = pkgPriority[pkg];
    if (filePath.includes("@vue/shared")) {
      pkg = "@vue/shared";
      priority = pkgPriority[pkg];
    } else if (filePath.includes("@vue/runtime-core")) {
      pkg = "@vue/runtime-core";
      priority = pkgPriority[pkg];
    } else if (filePath.includes(path.join("vue", "dist", "vue.d.ts"))) {
      pkg = "vue";
      priority = pkgPriority[pkg];
    }

    const visit = (node) => {
      // Collect type names (interfaces, type aliases, etc.)
      if (ts.isInterfaceDeclaration(node) || ts.isTypeAliasDeclaration(node)) {
        const isExported = node.modifiers?.some(
          (m) => m.kind === ts.SyntaxKind.ExportKeyword
        );
        if (isExported) {
          const name = node.name.text;
          const existing = exportedTypes.get(name);
          // Only set if not already set or if current package has higher priority
          if (!existing || pkgPriority[existing] < priority) {
            exportedTypes.set(name, pkg);
          }
          if (pkg === "vue") {
            exportedInVue.add(name);
          }
        } else {
          internalTypes.add(node.name.text);
        }
      }
      ts.forEachChild(node, visit);
    };
    visit(sf);
  }

  return { exportedTypes, internalTypes, exportedInVue };
}

function collectUsedInternalTypes(
  files,
  declsMap,
  internalTypes,
  exportedTypes,
  exportedInVue
) {
  const usedInternal = new Set();
  const neededLocalExternal = new Set();

  // Only collect internal types that are directly referenced in function parameters or type parameters
  for (const [, decls] of declsMap.entries()) {
    for (const { node } of decls) {
      // Check parameters
      if (node.parameters) {
        for (const param of node.parameters) {
          if (param.type) {
            const visit = (node) => {
              if (
                ts.isTypeReferenceNode(node) &&
                ts.isIdentifier(node.typeName)
              ) {
                const name = node.typeName.text;
                if (internalTypes.has(name)) {
                  usedInternal.add(name);
                }
                if (exportedTypes.has(name) && !exportedInVue.has(name)) {
                  neededLocalExternal.add(name);
                }
              }
              ts.forEachChild(node, visit);
            };
            visit(param.type);
          }
        }
      }

      // Check type parameters (constraints)
      if (node.typeParameters) {
        for (const typeParam of node.typeParameters) {
          if (typeParam.constraint) {
            const visit = (node) => {
              if (
                ts.isTypeReferenceNode(node) &&
                ts.isIdentifier(node.typeName)
              ) {
                const name = node.typeName.text;
                if (internalTypes.has(name)) {
                  usedInternal.add(name);
                }
                if (exportedTypes.has(name) && !exportedInVue.has(name)) {
                  neededLocalExternal.add(name);
                }
              }
              ts.forEachChild(node, visit);
            };
            visit(typeParam.constraint);
          }
        }
      }

      // Check return types as well (to include e.g. PropsWithDefaults)
      if (node.type && false) {
        const visit = (node) => {
          if (ts.isTypeReferenceNode(node) && ts.isIdentifier(node.typeName)) {
            const name = node.typeName.text;
            if (internalTypes.has(name)) {
              usedInternal.add(name);
            }
            if (exportedTypes.has(name) && !exportedInVue.has(name)) {
              neededLocalExternal.add(name);
            }
          }
          ts.forEachChild(node, visit);
        };
        visit(node.type);
      }
    }
  }

  // Collect all type nodes first
  const typeNodes = new Map();
  const printer = ts.createPrinter({ newLine: ts.NewLineKind.LineFeed });

  for (const filePath of files) {
    const source = fs.readFileSync(filePath, "utf-8");
    const sf = ts.createSourceFile(
      filePath,
      source,
      ts.ScriptTarget.Latest,
      true
    );

    const visit = (node) => {
      if (ts.isInterfaceDeclaration(node) || ts.isTypeAliasDeclaration(node)) {
        const text = printer.printNode(ts.EmitHint.Unspecified, node, sf);
        typeNodes.set(node.name.text, { text, node, sf });
      }
      ts.forEachChild(node, visit);
    };
    visit(sf);
  }

  // Transitive closure: add dependencies of included internal types
  const typeDefinitions = new Map();
  const localExternalDefinitions = new Map();
  const toProcess = Array.from(usedInternal);
  const processed = new Set();

  while (toProcess.length > 0) {
    const typeName = toProcess.pop();
    if (processed.has(typeName)) continue;
    processed.add(typeName);

    const typeInfo = typeNodes.get(typeName);
    if (!typeInfo) continue;

    typeDefinitions.set(typeName, typeInfo.text);

    // Find internal and external (non-vue) type dependencies
    const visit = (node) => {
      if (ts.isTypeReferenceNode(node) && ts.isIdentifier(node.typeName)) {
        const name = node.typeName.text;
        if (internalTypes.has(name) && !processed.has(name)) {
          toProcess.push(name);
        }
        if (exportedTypes.has(name) && !exportedInVue.has(name)) {
          neededLocalExternal.add(name);
        }
      }
      ts.forEachChild(node, visit);
    };
    visit(typeInfo.node);
  }

  // Populate local external type definitions (e.g., IfAny)
  for (const name of neededLocalExternal) {
    const typeInfo = typeNodes.get(name);
    if (typeInfo) {
      localExternalDefinitions.set(name, typeInfo.text);
    }
  }

  return { typeDefinitions, localExternalDefinitions };
}

function transformSignatureReturnToArgTypeAppendUniqueKey(
  item,
  exportedTypes,
  exportedInVue,
  helperTypes
) {
  const { node, sf } = item;
  const printer = ts.createPrinter({ newLine: ts.NewLineKind.LineFeed });

  // Use only dynamically detected helper types
  const allHelperTypes = new Set([...helperTypes]);

  const qualifyTransform = (context) => {
    const visit = (node) => {
      if (ts.isTypeReferenceNode(node) && ts.isIdentifier(node.typeName)) {
        const name = node.typeName.text;
        if (allHelperTypes.has(name)) {
          return node;
        }
        if (exportedTypes.has(name)) {
          // Qualify all exported Vue types to import('vue')
          return ts.factory.createImportTypeNode(
            ts.factory.createLiteralTypeNode(
              ts.factory.createStringLiteral("vue")
            ),
            undefined,
            ts.factory.createIdentifier(name),
            node.typeArguments,
            false
          );
        } else {
          // Leave as identifier; expect local/internal definition
          return ts.factory.updateTypeReferenceNode(
            node,
            ts.factory.createIdentifier(name),
            node.typeArguments
          );
        }
      }
      return ts.visitEachChild(node, visit, context);
    };
    return (node) => ts.visitEachChild(node, visit, context);
  };

  const qualifyResult = ts.transform(node, [qualifyTransform]);
  const qualifiedNode = qualifyResult.transformed[0];
  qualifyResult.dispose();

  // Determine base type based on new rules:
  // - If there are parameters, use first parameter type (or tuple of all parameter types if multiple)
  // - If no parameters but has type parameters, use first type parameter (or tuple if multiple)
  // - Otherwise use any
  let baseTypeNode;

  if (qualifiedNode.parameters && qualifiedNode.parameters.length > 0) {
    // Get parameter types
    const paramTypes = [];
    for (const param of qualifiedNode.parameters) {
      if (param.type) {
        paramTypes.push(param.type);
      }
    }

    if (paramTypes.length === 1) {
      baseTypeNode = paramTypes[0];
    } else if (paramTypes.length > 1) {
      // Multiple parameters: create tuple type
      baseTypeNode = ts.factory.createTupleTypeNode(paramTypes);
    }
  }

  if (
    !baseTypeNode &&
    qualifiedNode.typeParameters &&
    qualifiedNode.typeParameters.length > 0
  ) {
    // Get type parameter references
    const typeParamRefs = qualifiedNode.typeParameters.map((tp) =>
      ts.factory.createTypeReferenceNode(tp.name)
    );

    if (typeParamRefs.length === 1) {
      baseTypeNode = typeParamRefs[0];
    } else {
      // Multiple type parameters: create tuple type
      baseTypeNode = ts.factory.createTupleTypeNode(typeParamRefs);
    }
  }

  if (!baseTypeNode) {
    baseTypeNode = ts.factory.createKeywordTypeNode(ts.SyntaxKind.AnyKeyword);
  }

  // Create: BaseType & { [UniqueKey]?: ... }
  // For UniqueKey value:
  // - If base type is an array type (single param that is array): BaseType & [BaseType]
  // - If base type is a tuple (multiple params): just BaseType
  // - Otherwise (single non-array param or type param): just BaseType
  let uniqueKeyValueType;

  // Check if baseTypeNode is an array type reference (like EE[])
  const isArrayType = ts.isArrayTypeNode(baseTypeNode);

  if (isArrayType) {
    // For array types like EE[], UniqueKey value is: EE[] & [EE[]]
    uniqueKeyValueType = ts.factory.createIntersectionTypeNode([
      baseTypeNode,
      ts.factory.createTupleTypeNode([baseTypeNode]),
    ]);
  } else {
    // For tuples (multiple params) or single non-array types, just use the base type
    uniqueKeyValueType = baseTypeNode;
  }

  const uniqueKeyProperty = ts.factory.createPropertySignature(
    undefined,
    ts.factory.createComputedPropertyName(
      ts.factory.createIdentifier("UniqueKey")
    ),
    ts.factory.createToken(ts.SyntaxKind.QuestionToken),
    uniqueKeyValueType
  );

  const typeLiteral = ts.factory.createTypeLiteralNode([uniqueKeyProperty]);

  const newReturnType = ts.factory.createIntersectionTypeNode([
    baseTypeNode,
    typeLiteral,
  ]);

  // Clone function declaration with new return type and append _Box to the name
  const originalName = qualifiedNode.name?.text || "";
  const newName = ts.factory.createIdentifier(
    NAME_PREPEND + originalName + NAME_APPEND
  );

  const newFunc = ts.factory.createFunctionDeclaration(
    qualifiedNode.modifiers,
    qualifiedNode.asteriskToken,
    newName,
    qualifiedNode.typeParameters,
    qualifiedNode.parameters,
    newReturnType,
    qualifiedNode.body
  );

  // Create a temporary source file for printing the new node
  const tempSf = ts.createSourceFile(
    "temp.ts",
    "",
    ts.ScriptTarget.Latest,
    false
  );
  return printer.printNode(ts.EmitHint.Unspecified, newFunc, tempSf);
}

function qualifyExportedTypesInText(
  text,
  sf,
  exportedTypes,
  exportedInVue,
  helperTypes
) {
  // Parse the type definition and qualify exported Vue types
  const tempSource = ts.createSourceFile(
    "temp.ts",
    text,
    ts.ScriptTarget.Latest,
    true
  );
  const printer = ts.createPrinter({ newLine: ts.NewLineKind.LineFeed });

  // Use only dynamically detected helper types
  const allHelperTypes = new Set([...helperTypes]);

  const qualifyTransform = (context) => {
    const visit = (node) => {
      if (ts.isTypeReferenceNode(node) && ts.isIdentifier(node.typeName)) {
        const name = node.typeName.text;
        if (allHelperTypes.has(name)) {
          return node;
        }
        if (exportedTypes.has(name)) {
          // Qualify all exported Vue types to import('vue')
          return ts.factory.createImportTypeNode(
            ts.factory.createLiteralTypeNode(
              ts.factory.createStringLiteral("vue")
            ),
            undefined,
            ts.factory.createIdentifier(name),
            node.typeArguments,
            false
          );
        } else {
          // Leave identifier as-is to resolve to local definition if provided
          return ts.factory.updateTypeReferenceNode(
            node,
            ts.factory.createIdentifier(name),
            node.typeArguments
          );
        }
      }
      return ts.visitEachChild(node, visit, context);
    };
    return (node) => ts.visitEachChild(node, visit, context);
  };

  const result = ts.transform(tempSource, [qualifyTransform]);
  const transformed = result.transformed[0];
  const qualified = printer.printFile(transformed);
  result.dispose();

  return qualified.trim();
}

function generateOverrideFileFromDeclarations(
  declsMap,
  exportedTypes,
  exportedInVue,
  internalTypeDefinitions,
  localExternalDefinitions,
  helperTypes,
  usesUnionToIntersection,
  usesIfAny
) {
  // Build import statement conditionally
  let imports = ["UniqueKey"];
  if (usesUnionToIntersection) {
    imports.push("UnionToIntersection");
  }
  let header = `// This file is auto-generated by scripts/generate-vue-overrides.mjs\n// Do not edit manually\n\nimport type { ${imports.join(
    ", "
  )} } from '../helpers/helpers';\n`;

  // Add locally inlined external types first (e.g., IfAny)
  header += `\n// Local copies of external Vue utility types (not exported by 'vue')\n`;
  if (usesIfAny) {
    header += `type IfAny<T, Y, N> = 0 extends 1 & T ? Y : N;\n`;
  }
  if (localExternalDefinitions.size > 0) {
    const sortedLocal = Array.from(localExternalDefinitions.keys()).sort();
    for (const typeName of sortedLocal) {
      const originalDef = localExternalDefinitions.get(typeName);
      const sf = ts.createSourceFile(
        "temp.ts",
        originalDef,
        ts.ScriptTarget.Latest,
        true
      );
      const qualifiedDef = qualifyExportedTypesInText(
        originalDef,
        sf,
        exportedTypes,
        exportedInVue,
        helperTypes
      );
      header += qualifiedDef + "\n";
    }
  }

  // Add internal type definitions if any are used
  if (internalTypeDefinitions.size > 0) {
    const sortedInternal = Array.from(internalTypeDefinitions.keys()).sort();
    header += `\n// Internal Vue types used in signatures\n`;
    for (const typeName of sortedInternal) {
      const originalDef = internalTypeDefinitions.get(typeName);
      // Qualify exported types within internal type definitions
      const sf = ts.createSourceFile(
        "temp.ts",
        originalDef,
        ts.ScriptTarget.Latest,
        true
      );
      const qualifiedDef = qualifyExportedTypesInText(
        originalDef,
        sf,
        exportedTypes,
        exportedInVue,
        helperTypes
      );
      header += qualifiedDef + "\n";
    }
  }

  header += `\n`;
  let content = header;
  for (const [name, decls] of declsMap.entries()) {
    if (!decls.length) continue;
    content += `// Overrides for ${name}\n`;
    const seen = new Set();
    for (const d of decls) {
      const text = transformSignatureReturnToArgTypeAppendUniqueKey(
        d,
        exportedTypes,
        exportedInVue,
        helperTypes
      );
      if (seen.has(text)) continue;
      seen.add(text);

      // Extract JSDoc from original if available
      if (d.jsDocText) {
        // Extract just the JSDoc comment part (before the function declaration)
        const funcDeclStart = d.jsDocText.indexOf("export declare function");
        if (funcDeclStart > 0) {
          const jsDoc = d.jsDocText.substring(0, funcDeclStart).trim();
          if (jsDoc) {
            content += jsDoc + "\n";
          }
        }
      }

      content += text + "\n\n";
    }
  }
  return content;
}

function main() {
  console.log("🔧 Generating Vue function overrides...");
  const files = resolveVueTypeFiles();
  if (!files.length) {
    console.error("❌ Could not locate Vue type declaration files.");
    process.exit(1);
  }
  console.log(
    "📄 Using type files:\n" + files.map((f) => "  - " + f).join("\n")
  );

  const { exportedTypes, internalTypes, exportedInVue } =
    collectVueTypeNames(files);
  console.log(
    `📦 Collected ${exportedTypes.size} exported and ${internalTypes.size} internal Vue type names`
  );

  const decls = extractFunctionDeclarations(files, FUNCTIONS_TO_OVERRIDE);
  for (const name of FUNCTIONS_TO_OVERRIDE) {
    const count = decls.get(name)?.length ?? 0;
    if (!count) console.warn(`⚠️  No declarations found for ${name}`);
    else console.log(`✅ ${name}: ${count} overload(s)`);
  }

  const { typeDefinitions, localExternalDefinitions } =
    collectUsedInternalTypes(
      files,
      decls,
      internalTypes,
      exportedTypes,
      exportedInVue
    );
  console.log(
    `🔍 Found ${
      typeDefinitions.size
    } internal types used in signatures: ${Array.from(
      typeDefinitions.keys()
    ).join(", ")}`
  );
  if (localExternalDefinitions.size) {
    console.log(
      `🔧 Including ${
        localExternalDefinitions.size
      } local copies of external types: ${Array.from(
        localExternalDefinitions.keys()
      ).join(", ")}`
    );
  }

  // Collect helper types: types that are exported but not from vue and are actually used in generated signatures
  // Also scan internal type definitions for helper types they use
  const helperTypes = new Set();

  const collectTypesFromNode = (typeNode) => {
    if (!typeNode) return;
    if (
      ts.isTypeReferenceNode(typeNode) &&
      ts.isIdentifier(typeNode.typeName)
    ) {
      const name = typeNode.typeName.text;
      if (
        exportedTypes.has(name) &&
        !exportedInVue.has(name) &&
        !internalTypes.has(name)
      ) {
        helperTypes.add(name);
      }
    }
    typeNode.forEachChild(collectTypesFromNode);
  };

  // Scan function signatures
  for (const [funcName, declList] of decls.entries()) {
    for (const { node } of declList) {
      if (node.parameters) {
        for (const param of node.parameters) {
          collectTypesFromNode(param.type);
        }
      }
      collectTypesFromNode(node.type);
      if (node.typeParameters) {
        for (const tp of node.typeParameters) {
          collectTypesFromNode(tp.constraint);
          collectTypesFromNode(tp.default);
        }
      }
    }
  }

  // Also scan internal type definitions for helper types they reference
  for (const [typeName, typeText] of typeDefinitions.entries()) {
    // Parse the type definition text to scan for type references
    const tempSf = ts.createSourceFile(
      "temp.ts",
      typeText,
      ts.ScriptTarget.Latest,
      true
    );
    const visitNode = (node) => {
      collectTypesFromNode(node);
      ts.forEachChild(node, visitNode);
    };
    visitNode(tempSf);
  }

  if (helperTypes.size) {
    console.log(
      `🎯 Detected helper types to keep unqualified: ${Array.from(
        helperTypes
      ).join(", ")}`
    );
  }

  // Check if UnionToIntersection or IfAny are used anywhere
  let usesUnionToIntersection = false;
  let usesIfAny = false;

  // Check in function signatures
  for (const [funcName, declList] of decls.entries()) {
    for (const { node } of declList) {
      const checkNode = (n) => {
        if (ts.isTypeReferenceNode(n) && ts.isIdentifier(n.typeName)) {
          if (n.typeName.text === "UnionToIntersection")
            usesUnionToIntersection = true;
          if (n.typeName.text === "IfAny") usesIfAny = true;
        }
        ts.forEachChild(n, checkNode);
      };
      checkNode(node);
    }
  }

  // Check in internal type definitions
  for (const [typeName, typeText] of typeDefinitions.entries()) {
    if (typeText.includes("UnionToIntersection"))
      usesUnionToIntersection = true;
    if (typeText.includes("IfAny")) usesIfAny = true;
  }

  // Check in local external definitions
  for (const [typeName, typeText] of localExternalDefinitions.entries()) {
    if (typeText.includes("UnionToIntersection"))
      usesUnionToIntersection = true;
    if (typeText.includes("IfAny")) usesIfAny = true;
  }

  const output = generateOverrideFileFromDeclarations(
    decls,
    exportedTypes,
    exportedInVue,
    typeDefinitions,
    localExternalDefinitions,
    helperTypes,
    usesUnionToIntersection,
    usesIfAny
  );

  const outDir = path.dirname(OUTPUT_FILE);
  if (!fs.existsSync(outDir)) fs.mkdirSync(outDir, { recursive: true });
  fs.writeFileSync(OUTPUT_FILE, output, "utf-8");
  console.log(`✅ Generated: ${OUTPUT_FILE}`);
}

main();
