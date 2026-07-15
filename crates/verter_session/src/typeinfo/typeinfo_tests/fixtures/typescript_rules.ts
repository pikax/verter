// @ai-generated - Synthetic TypeScript type-system rules fixture.

export type LiteralAndPrimitiveSurface = {
  stringLiteral: "ready";
  numberLiteral: 42;
  booleanLiteral: true;
  stringValue: string;
  numberValue: number;
  booleanValue: boolean;
  symbolValue: symbol;
  bigintValue: bigint;
  nullValue: null;
  undefinedValue: undefined;
  unknownValue: unknown;
  anyValue: any;
  neverValue: never;
};

export type MethodAndIndexSurface = {
  readonly id: string;
  label?: string;
  method?: (input: string, count?: number) => boolean;
  [key: string]:
    | string
    | number
    | boolean
    | undefined
    | ((input: string, count?: number) => boolean);
};

export type TupleRules = [name: string, count?: number, ...flags: boolean[]];

export type ReadonlyTupleRules = readonly [mode: "view", values: readonly number[]];

export type FunctionRules = (
  item: { id: string },
  ...flags: boolean[]
) => { id: string; flags: boolean[] };

export type RecordLiteralKeys = Record<"alpha" | "beta", number>;

export type MappedModifierRules<T> = {
  readonly [K in keyof T]-?: T[K];
};

export type MappedModifierSurface = MappedModifierRules<{
  id?: string;
  count?: number;
}>;

export type UnionObjectRules =
  | { kind: "a"; a: string; shared: boolean }
  | { kind: "b"; b: number; shared: boolean };

export type IntersectionObjectRules = { id: string } & { count?: number } & {
  readonly ready: boolean;
};

export interface KeySource {
  id: string;
  count?: number;
  nested: {
    value: string;
  };
}

export type KeyOfRules = keyof KeySource;
export type IndexedRules = KeySource["nested"]["value"];

export type ConditionalDistributive<T> = T extends string ? { text: T } : { other: T };
export type ConditionalDistributedRules = ConditionalDistributive<"a" | 1>;

export type ConditionalNonDistributive<T> = [T] extends [string] ? { text: T } : { other: T };
export type ConditionalNonDistributedRules = ConditionalNonDistributive<"a" | 1>;

export type ConstructorLike = new (id: string) => { id: string; ready: boolean };
export type ConstructorParamsRules = ConstructorParameters<ConstructorLike>;
export type InstanceRules = InstanceType<ConstructorLike>;

export class ClassRules {
  id: string;
  constructor(id: string);
  method(count: number): string;
}
export type ClassInstanceRules = InstanceType<typeof ClassRules>;
export type ClassConstructorParamsRules = ConstructorParameters<typeof ClassRules>;

export const literalConfig = {
  mode: "view",
  nested: {
    value: 1,
  },
} as const;
export type TypeOfConstRules = typeof literalConfig;
export type TypeOfConstNestedValue = typeof literalConfig.nested.value;

export type AwaitedRules = Awaited<Promise<Promise<{ done: true }>>>;

export type TemplateIntrinsicRules = `on${Capitalize<"submit" | "cancel">}`;

export type KeyRemapExcludeRules<T> = {
  [K in keyof T as K extends "internal" ? never : `public:${K & string}`]: T[K];
};
export type KeyRemapExcludeSurface = KeyRemapExcludeRules<{
  id: string;
  internal: boolean;
  count: number;
}>;
