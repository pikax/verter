import fs from "fs";
import path from "path";
import ts from "typescript";
import { fileURLToPath } from "url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const srcDir = path.join(__dirname, "../src");
const distDir = path.join(__dirname, "../dist");

// Collect all declared names (types, interfaces, classes, enums, functions, variables, namespaces)
function collectAllTypes(sourceFiles) {
  const allTypes = new Set();

  function addName(name) {
    if (name && typeof name.text === "string") allTypes.add(name.text);
  }

  function visit(node) {
    if (ts.isTypeAliasDeclaration(node)) addName(node.name);
    else if (ts.isInterfaceDeclaration(node)) addName(node.name);
    else if (ts.isClassDeclaration(node) && node.name) addName(node.name);
    else if (ts.isEnumDeclaration(node)) addName(node.name);
    else if (ts.isFunctionDeclaration(node) && node.name) addName(node.name);
    else if (ts.isModuleDeclaration(node)) addName(node.name);
    else if (ts.isVariableStatement(node)) {
      node.declarationList.declarations.forEach((decl) => {
        if (ts.isIdentifier(decl.name)) addName(decl.name);
      });
    }

    return ts.forEachChild(node, visit);
  }

  sourceFiles.forEach((sf) => visit(sf));
  return allTypes;
}

function transformSourceFile(sourceFile, allTypes, prefix = "$V_", keepComments = false) {
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
            node.typeParameters,
            node.heritageClauses,
            node.members
          );
        }

        if (ts.isClassDeclaration(node)) {
          const name = node.name ? rewriteIdentifier(node.name) : node.name;
          return ts.factory.updateClassDeclaration(
            node,
            node.modifiers,
            name,
            node.typeParameters,
            node.heritageClauses,
            node.members
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
            node.typeParameters,
            node.parameters,
            node.type,
            node.body
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
            node.typeParameters,
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
          if (newName !== node.typeName) {
            return ts.factory.updateTypeReferenceNode(
              node,
              newName,
              node.typeArguments?.map((t) => ts.visitNode(t, visit))
            );
          }
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
  // Read source files
  const helpersPath = path.join(srcDir, "helpers/helpers.ts");
  const emitsPath = path.join(srcDir, "emits/emits.ts");

  const helpersSource = fs.readFileSync(helpersPath, "utf-8");
  const emitsSource = fs.readFileSync(emitsPath, "utf-8");

  // Parse TypeScript files
  const helpersFile = ts.createSourceFile(
    "helpers.ts",
    helpersSource,
    ts.ScriptTarget.Latest,
    true
  );
  const emitsFile = ts.createSourceFile(
    "emits.ts",
    emitsSource,
    ts.ScriptTarget.Latest,
    true
  );

  // CLI flag: keep comments in the output string
  const keepComments = process.argv.includes("--keep-comments");

  // Collect all declarations (exported and internal)
  const allTypes = collectAllTypes([helpersFile, emitsFile]);

  // Transform files
  let transformedHelpers = transformSourceFile(helpersFile, allTypes, "$V_", keepComments);
  let transformedEmits = transformSourceFile(emitsFile, allTypes, "$V_", keepComments);

  // Remove imports from emits
  transformedEmits = transformedEmits.replace(/import\s*{[^}]+}\s*from\s*["'][^"']+["'];?\s*/g, "");

  // Combine
  const combined = transformedHelpers + "\n\n" + transformedEmits;

  // Create output
  const output = `// Generated file - do not edit directly
const typeHelpersSource = \`${combined.replace(/`/g, "\\`").replace(/\$/g, "\\$")}\`;
export default typeHelpersSource;
`;

  // Ensure dist exists
  if (!fs.existsSync(distDir)) {
    fs.mkdirSync(distDir, { recursive: true });
  }

  // Write files
  fs.writeFileSync(path.join(distDir, "string-export.js"), output);
  fs.writeFileSync(
    path.join(distDir, "string-export.d.ts"),
    `declare const typeHelpersSource: string;\nexport default typeHelpersSource;\n`
  );

  console.log("✓ Built string export successfully");
}

build();
