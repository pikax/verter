import { TSTypeAliasDeclaration, VerterASTNode } from "../../ast";
import { parseOXC } from "../../ast/ast";

export type GenericInfo = {
  /**
   * Source of the generic
   *
   * @value
   *  ```ts
   *  T extends 'foo' | 'bar'
   * ```
   * @example
   * ```vue
   * <script generic="T extends 'foo' | 'bar'" setup lang="ts">
   * ```
   */
  source: string;

  /**
   * Used to prefix sanitised names
   * @default '__VERTER__TS__'
   */
  sanitisePrefix?: string;

  /**
   * The generic array names
   * @example
   * ```vue
   * <script generic="T extends 'foo' | 'bar', Comp" setup lang="ts">
   * ```
   * The names will be `['T', 'Comp']`
   */
  names: string[];

  /**
   * The sanitised names, prefixed with `sanitisePrefix`
   * @example
   * ```vue
   * <script generic="T extends 'foo' | 'bar', Comp" setup lang="ts">
   * ```
   * The names will be `['__VERTER__TS__T', '__VERTER__TS__Comp']`
   */
  sanitisedNames: string[];

  /**
   * Declaration generic, it contains the same constraints and default as `source`
   * but the names are sanitised and if no default provided it will default to `any`
   *
   * @example
   * ```vue
   * <script generic="T extends 'foo' | 'bar', Comp" setup lang="ts">
   * ```
   * The declaration will be the content
   * ```ts
   * <__VERTER__TS__T extends 'foo' | 'bar' = any, Comp = any>()=>{}
   * ```
   */
  declaration: string;

  /**
   * Position of the generic in the document
   */
  position: {
    start: number;
    end: number;
  };
};

export function parseGeneric(
  genericStr: string,
  offset: number = 0,
  prefix: string = "__VERTER__TS__"
): GenericInfo | null {
  if (!genericStr) return null;
  const genericCode = `type __GENERIC__<${genericStr}> = {};`;
  const ast = parseOXC(genericCode, { lang: "ts", sourceType: "script" });

  const body = ast.program.body[0] as TSTypeAliasDeclaration;

  // TODO handle if generic is invalid, maybe do a parse with REGEX
  if (!body || !("typeParameters" in body) || !body.typeParameters) {
    // todo parse with acorn
    return null;
  }
  const params = body?.typeParameters?.params ?? [];

  const items = params.map((param, index) => ({
    name: param.name.name,
    content: genericCode.slice(param.start, param.end),
    constraint: param.constraint
      ? genericCode.slice(param.constraint.start, param.constraint.end)
      : undefined,
    default: param.default
      ? genericCode.slice(param.default.start, param.default.end)
      : undefined,
    index,
  }));

  function getGenericComponentName(name: string) {
    return prefix + name;
  }

  function replaceComponentNameUsage(name: string, content: string) {
    const regex = new RegExp(`\\b${name}\\b`, "g");
    return content.replace(regex, getGenericComponentName(name));
  }
  function sanitiseGenericNames(content: string | null | undefined): string {
    if (!content) return content ?? "";
    return names
      ? names.reduce((prev, cur) => {
          return replaceComponentNameUsage(cur, prev);
        }, content)
      : content;
  }

  const names = items.map((x) => x.name);
  const sanitisedNames = names.map(sanitiseGenericNames);

  const declaration = items
    .map((x) => {
      const name = getGenericComponentName(x.name);
      const constraint = sanitiseGenericNames(x.constraint);
      const defaultType = sanitiseGenericNames(x.default);

      return [
        name,
        constraint ? `extends ${constraint}` : undefined,
        `= ${defaultType || "any"}`,
      ]
        .filter(Boolean)
        .join(" ");
    })
    .join(", ");

  return {
    source: genericStr,
    names,
    sanitisedNames,
    declaration,
    position: {
      start: offset,
      end: offset + genericStr.length,
    },
  };
}
