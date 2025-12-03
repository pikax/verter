import fs from "fs";
import path from "path";
import ts from "typescript";
import { fileURLToPath } from "url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const srcDir = path.join(__dirname, "../src");
const distDir = path.join(__dirname, "../dist");

// Parse exports from index.ts to discover source files
function getSourceFilesFromIndex(indexPath) {
  const indexSource = fs.readFileSync(indexPath, "utf-8");
  const indexFile = ts.createSourceFile(
    "index.ts",
    indexSource,
    ts.ScriptTarget.Latest,
    true
  );

  const sourceFiles = [];

  ts.forEachChild(indexFile, (node) => {
    if (ts.isExportDeclaration(node) && node.moduleSpecifier) {
      if (ts.isStringLiteral(node.moduleSpecifier)) {
        const modulePath = node.moduleSpecifier.text;
        // Convert "./helpers" to "helpers/helpers.ts" or similar
        const relativePath = modulePath.replace(/^\.\//, "");
        const folderName = relativePath.split("/").pop();

        // Try to resolve the actual implementation file (not index.ts)
        // Check for <folder>/<folder>.ts first, then <name>.ts, then index.ts as fallback
        const possiblePaths = [
          path.join(srcDir, `${relativePath}/${folderName}.ts`),
          path.join(srcDir, `${relativePath}.ts`),
          path.join(srcDir, `${relativePath}/index.ts`),
        ];

        for (const filePath of possiblePaths) {
          if (fs.existsSync(filePath)) {
            sourceFiles.push(filePath);
            break;
          }
        }
      }
    }
  });

  return sourceFiles;
}

// Resolve a relative import path to an absolute file path
function resolveImportPath(importPath, fromFile) {
  const fromDir = path.dirname(fromFile);
  const resolved = path.resolve(fromDir, importPath);
  
  // Try with .ts extension first, then as-is if it's a file, then index.ts
  const possiblePaths = [
    resolved + ".ts",
    path.join(resolved, "index.ts"),
  ];
  
  // Only add resolved if it exists and is a file (not directory)
  if (fs.existsSync(resolved) && fs.statSync(resolved).isFile()) {
    possiblePaths.unshift(resolved);
  }
  
  for (const p of possiblePaths) {
    if (fs.existsSync(p) && fs.statSync(p).isFile()) {
      return p;
    }
  }
  return null;
}

// Collect local imports from a source file
function collectLocalImports(sourceFile, filePath) {
  const imports = [];
  
  ts.forEachChild(sourceFile, (node) => {
    if (ts.isImportDeclaration(node) && node.moduleSpecifier) {
      if (ts.isStringLiteral(node.moduleSpecifier)) {
        const modulePath = node.moduleSpecifier.text;
        // Only handle relative imports (local files)
        if (modulePath.startsWith(".")) {
          const resolvedPath = resolveImportPath(modulePath, filePath);
          if (resolvedPath) {
            imports.push({
              modulePath,
              resolvedPath,
              importClause: node.importClause,
            });
          }
        }
      }
    }
  });
  
  return imports;
}

// Recursively collect all source files including local imports
function collectAllSourceFiles(initialFiles) {
  const allFiles = new Set();
  const fileOrder = []; // Maintain order: dependencies first
  const pending = [...initialFiles];
  
  while (pending.length > 0) {
    const filePath = pending.shift();
    if (allFiles.has(filePath)) continue;
    
    const source = fs.readFileSync(filePath, "utf-8");
    const sourceFile = ts.createSourceFile(
      path.basename(filePath),
      source,
      ts.ScriptTarget.Latest,
      true
    );
    
    // Collect local imports from this file
    const imports = collectLocalImports(sourceFile, filePath);
    
    // Add imported files to pending (they should be processed before this file)
    for (const imp of imports) {
      if (!allFiles.has(imp.resolvedPath)) {
        pending.unshift(imp.resolvedPath); // Add to front to process dependencies first
      }
    }
    
    allFiles.add(filePath);
    fileOrder.push(filePath);
  }
  
  return fileOrder;
}

// Collect all declared names (for rewriting) and exported types/interfaces (for ExportedTypes set)
function collectDeclarations(sourceFiles) {
  const allTypes = new Set();
  const exportedTypesOrInterfaces = new Set();

  const addName = (name) => {
    if (name && typeof name.text === "string") allTypes.add(name.text);
  };

  const hasExport = (node) =>
    Array.isArray(node.modifiers) &&
    node.modifiers.some((m) => m.kind === ts.SyntaxKind.ExportKeyword);

  function visit(node) {
    if (ts.isTypeAliasDeclaration(node)) {
      addName(node.name);
      if (hasExport(node)) exportedTypesOrInterfaces.add(node.name.text);
    } else if (ts.isInterfaceDeclaration(node)) {
      addName(node.name);
      if (hasExport(node)) exportedTypesOrInterfaces.add(node.name.text);
    } else if (ts.isClassDeclaration(node) && node.name) {
      addName(node.name);
    } else if (ts.isEnumDeclaration(node)) {
      addName(node.name);
    } else if (ts.isFunctionDeclaration(node) && node.name) {
      addName(node.name);
    } else if (ts.isModuleDeclaration(node)) {
      addName(node.name);
    } else if (ts.isVariableStatement(node)) {
      node.declarationList.declarations.forEach((decl) => {
        if (ts.isIdentifier(decl.name)) addName(decl.name);
      });
    }

    return ts.forEachChild(node, visit);
  }

  sourceFiles.forEach((sf) => visit(sf));
  return { allTypes, exportedTypesOrInterfaces };
}

function transformSourceFile(
  sourceFile,
  allTypes,
  prefix = "$V_",
  keepComments = false
) {
  const rewriteIdentifier = (ident) => {
    if (allTypes.has(ident.text) && !ident.text.startsWith(prefix)) {
      return ts.factory.createIdentifier(prefix + ident.text);
    }
    return ident;
  };

  const rewriteEntityName = (name) => {
    if (ts.isIdentifier(name)) return rewriteIdentifier(name);
    if (ts.isQualifiedName(name)) {
      const left = rewriteEntityName(name.left);
      const right = name.right;
      return ts.factory.updateQualifiedName(left, right);
    }
    return name;
  };

  const transformer = (context) => {
    return (rootNode) => {
      function visit(node) {
        // Prefix all declaration kinds
        if (ts.isInterfaceDeclaration(node)) {
          const name = rewriteIdentifier(node.name);
          return ts.factory.updateInterfaceDeclaration(
            node,
            node.modifiers,
            name,
            node.typeParameters?.map((tp) => ts.visitNode(tp, visit)),
            node.heritageClauses?.map((hc) => ts.visitNode(hc, visit)),
            node.members.map((m) => ts.visitNode(m, visit))
          );
        }

        if (ts.isClassDeclaration(node)) {
          const name = node.name ? rewriteIdentifier(node.name) : node.name;
          return ts.factory.updateClassDeclaration(
            node,
            node.modifiers,
            name,
            node.typeParameters?.map((tp) => ts.visitNode(tp, visit)),
            node.heritageClauses?.map((hc) => ts.visitNode(hc, visit)),
            node.members.map((m) => ts.visitNode(m, visit))
          );
        }

        if (ts.isEnumDeclaration(node)) {
          const name = rewriteIdentifier(node.name);
          return ts.factory.updateEnumDeclaration(
            node,
            node.modifiers,
            name,
            node.members
          );
        }

        if (ts.isFunctionDeclaration(node)) {
          const name = node.name ? rewriteIdentifier(node.name) : node.name;
          return ts.factory.updateFunctionDeclaration(
            node,
            node.modifiers,
            node.asteriskToken,
            name,
            node.typeParameters?.map((tp) => ts.visitNode(tp, visit)),
            node.parameters.map((p) => ts.visitNode(p, visit)),
            node.type ? ts.visitNode(node.type, visit) : node.type,
            node.body ? ts.visitNode(node.body, visit) : node.body
          );
        }

        if (ts.isModuleDeclaration(node)) {
          const name = rewriteIdentifier(node.name);
          return ts.factory.updateModuleDeclaration(
            node,
            node.modifiers,
            name,
            node.body
          );
        }

        // Prefix all type alias declarations (exported and non-exported)
        if (ts.isTypeAliasDeclaration(node)) {
          // debug: list all type aliases visited
          if (process.argv.includes("--debug")) {
            console.log("TypeAlias:", node.name.text);
          }
          const name = rewriteIdentifier(node.name);
          return ts.factory.updateTypeAliasDeclaration(
            node,
            node.modifiers,
            name,
            node.typeParameters?.map((tp) => ts.visitNode(tp, visit)),
            ts.visitNode(node.type, visit)
          );
        }

        // Prefix all variable declarations (identifiers only)
        if (ts.isVariableStatement(node)) {
          const newDecls = node.declarationList.declarations.map((decl) => {
            if (ts.isIdentifier(decl.name)) {
              const name = rewriteIdentifier(decl.name);
              return ts.factory.updateVariableDeclaration(
                decl,
                name,
                decl.exclamationToken,
                decl.type,
                decl.initializer
              );
            }
            return decl;
          });
          return ts.factory.updateVariableStatement(
            node,
            node.modifiers,
            ts.factory.updateVariableDeclarationList(
              node.declarationList,
              newDecls
            )
          );
        }

        // Prefix type references (only for our defined types)
        if (ts.isTypeReferenceNode(node)) {
          const newName = rewriteEntityName(node.typeName);
          const newTypeArgs = node.typeArguments?.map((t) => ts.visitNode(t, visit));
          return ts.factory.updateTypeReferenceNode(
            node,
            newName,
            newTypeArgs
          );
        }

        // Prefix typeof references (TypeQueryNode)
        if (ts.isTypeQueryNode(node)) {
          const newExpr = rewriteEntityName(node.exprName);
          if (newExpr !== node.exprName) {
            return ts.factory.updateTypeQueryNode(node, newExpr);
          }
        }

        // Prefix computed property names that reference our types
        if (ts.isComputedPropertyName(node)) {
          const expr = node.expression;
          if (ts.isIdentifier(expr) && allTypes.has(expr.text)) {
            return ts.factory.updateComputedPropertyName(
              node,
              rewriteIdentifier(expr)
            );
          }
        }

        return ts.visitEachChild(node, visit, context);
      }
      return ts.visitNode(rootNode, visit);
    };
  };

  const result = ts.transform(sourceFile, [transformer]);
  const printer = ts.createPrinter({ removeComments: !keepComments });
  return printer.printFile(result.transformed[0]);
}

function build() {
  // Discover source files from index.ts
  const indexPath = path.join(srcDir, "index.ts");
  const initialSourceFiles = getSourceFilesFromIndex(indexPath);

  if (initialSourceFiles.length === 0) {
    console.error("No source files found in index.ts exports");
    process.exit(1);
  }

  // Collect all source files including local imports (dependencies first)
  const sourceFilePaths = collectAllSourceFiles(initialSourceFiles);

  console.log(
    "Found source files:",
    sourceFilePaths.map((p) => path.relative(srcDir, p)).join(", ")
  );

  // Read and parse all source files
  const sourceFiles = sourceFilePaths.map((filePath) => {
    const source = fs.readFileSync(filePath, "utf-8");
    return ts.createSourceFile(
      path.basename(filePath),
      source,
      ts.ScriptTarget.Latest,
      true
    );
  });

  // CLI flag: keep comments in the output string
  const keepComments = process.argv.includes("--keep-comments");

  // Collect all declarations (for rewriting) and exported types/interfaces (for ExportedTypes)
  const { allTypes, exportedTypesOrInterfaces } = collectDeclarations(sourceFiles);

  // Transform all files
  const transformedSources = sourceFiles.map((sourceFile, index) => {
    let transformed = transformSourceFile(
      sourceFile,
      allTypes,
      "$V_",
      keepComments
    );

    // Remove all local relative imports (they are inlined)
    transformed = transformed.replace(
      /import\s+(?:type\s+)?{[^}]+}\s*from\s*["']\.\.?\/[^"']+["'];?\s*/g,
      ""
    );

    // Remove all export * from statements (content is inlined)
    transformed = transformed.replace(
      /export\s+\*\s+from\s*["'][^"']+["'];?\s*/g,
      ""
    );

    // Remove all export { } from statements (content is inlined)
    transformed = transformed.replace(
      /export\s+(?:type\s+)?{[^}]+}\s*from\s*["'][^"']+["'];?\s*/g,
      ""
    );

    return transformed;
  });

  // Combine all transformed sources
  let combined = transformedSources.join("\n\n");

  // Append auto-generated AvailableTypes from exported types/interfaces (unprefixed string literals)
  const available = Array.from(allTypes);
  if (available.length) {
    combined +=
      "\n\nexport type AvailableTypes = " +
      available.map((n) => `"${n}"`).join(" | ") +
      ";\n";
  }

  // Create output
  const processed = combined.replace(/`/g, "\\`").replace(/\$/g, "\\$");
  // const availableJson = JSON.stringify(available);
  const output = `// Generated file - do not edit directly
const typeHelpersSource = \`${processed}\`;
export default typeHelpersSource;
export function prefixWith(prefix) {
  return typeHelpersSource
    .replaceAll("$V_", prefix);
}
export const ExportedTypes = new Set([${Array.from(exportedTypesOrInterfaces).map((n) => `"${n}"`).join(", ")}]);
`;

  // Ensure dist exists
  if (!fs.existsSync(distDir)) {
    fs.mkdirSync(distDir, { recursive: true });
  }

  // Write files
  fs.writeFileSync(path.join(distDir, "string-export.js"), output);
  fs.writeFileSync(
    path.join(distDir, "string-export.d.ts"),
    `declare const typeHelpersSource: string;\nexport default typeHelpersSource;\nexport type AvailableExports = ${
      available.map((n) => `"${n}"`).join(" | ") || "never"
    };\nexport function prefixWith(prefix: string): string;\nexport const ExportedTypes: Set<string>;\n`
  );

  console.log("✓ Built string export successfully");
}

build();
