import type { TypeDescriptor } from "@verter/component-meta/browser";

/**
 * Display spelling for a compiler-intrinsic wire token.
 *
 * Rendered LOCALLY on purpose: the type-only import above is erased at
 * runtime, so importing a value from `@verter/component-meta/browser` would
 * turn this module into a real runtime dependency on that package's build
 * output. An unknown op renders as its raw token rather than guessing.
 */
function intrinsicOpName(op: string): string {
  return op === "awaited" ? "Awaited" : op;
}

export function formatTypeDescriptor(td: TypeDescriptor): string {
  switch (td.kind) {
    case "primitive":
      return td.name;
    case "literal":
      return typeof td.value === "string" ? `"${td.value}"` : String(td.value);
    case "union":
      return td.types.map(formatTypeDescriptor).join(" | ");
    case "intersection":
      return td.types.map(formatTypeDescriptor).join(" & ");
    case "array": {
      const inner = formatTypeDescriptor(td.element);
      const needsParens = td.element.kind === "union" || td.element.kind === "intersection";
      return needsParens ? `(${inner})[]` : `${inner}[]`;
    }
    case "tuple":
      return `[${td.elements.map(formatTypeDescriptor).join(", ")}]`;
    case "object": {
      const props = td.properties.map(
        (p) => `${p.name}${p.optional ? "?" : ""}: ${formatTypeDescriptor(p.type)}`,
      );
      return `{ ${props.join("; ")} }`;
    }
    case "function": {
      const params = td.parameters.map(
        (p) => `${p.name}${p.optional ? "?" : ""}: ${formatTypeDescriptor(p.type)}`,
      );
      return `(${params.join(", ")}) => ${formatTypeDescriptor(td.returnType)}`;
    }
    case "enum":
      return td.name;
    case "ref": {
      if (td.typeArguments?.length) {
        return `${td.name}<${td.typeArguments.map(formatTypeDescriptor).join(", ")}>`;
      }
      return td.name;
    }
    case "intrinsicApplication": {
      if (td.arguments.length) {
        return `${intrinsicOpName(td.op)}<${td.arguments.map(formatTypeDescriptor).join(", ")}>`;
      }
      return intrinsicOpName(td.op);
    }
    case "unknown":
      return td.rawType;
  }
}
