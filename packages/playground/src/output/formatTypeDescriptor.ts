import type { TypeDescriptor } from "@verter/component-meta/browser";

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
    case "unknown":
      return td.rawType;
  }
}
