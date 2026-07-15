// Vendored fixture source bytes for the U2.MAPPED_TEMPLATE-era oracle lift
// rows (`mapped_template.ts` + `template_literal_inference.ts`), split out of
// `oracle_query_specs_vendored_sources.rs` to keep each vendored-sources file
// under the production line-size guard. `include!`'d by `oracle_query_specs.rs`
// immediately after the JSX vendored-sources file (the registry is the
// source-byte authority; the guard
// `inlined_registry_source_is_byte_identical_to_fixture_files` pins each const
// byte-identical to its on-disk `fixtures/*.ts` sibling).

/// Vendored source bytes of `/fixtures/mapped_template.ts` (the registry is the
/// source-byte authority). Inlined verbatim (PURE owned `&'static str`); the
/// guard `inlined_registry_source_is_byte_identical_to_fixture_files` asserts
/// byte-identity with `fixtures/mapped_template.ts`. NOTE the on-disk fixture
/// name uses an underscore but the rows upsert it at the hyphenated canonical
/// path `/fixtures/mapped-template.ts`.
#[allow(dead_code)]
pub(crate) const MAPPED_TEMPLATE_SOURCE: &str = r#"// @ai-generated - Synthetic mapped and template-literal key typeinfo fixture.

export type VNode = unknown;

export interface SlotSource {
  root?: { id: string };
  item?: { value: number };
  empty?: { reason: "none" | "filtered" };
}

export type PlainSlotMap = {
  [K in keyof SlotSource]-?: SlotSource[K];
};

export type RemappedSlotMap = {
  [K in keyof SlotSource as K extends string ? `slot:${K}` : never]-?: (
    payload: NonNullable<SlotSource[K]>,
  ) => VNode[];
};

export type StaticTemplateSlots = {
  "cell:name": (payload: { value: string; column: "name" }) => VNode[];
  "cell:count": (payload: { value: number; column: "count" }) => VNode[];
};

export type TemplateLiteralCellName = `cell:${"name"}`;
export type NameCellRenderer = StaticTemplateSlots[TemplateLiteralCellName];
export type RootRemappedSlot = RemappedSlotMap["slot:root"];
export type ItemRemappedSlot = RemappedSlotMap["slot:item"];

export type RecordTemplateSlots = Record<
  `slot:${"root" | "item"}`,
  (payload: { name: "root" | "item" }) => VNode[]
>;
export type RecordTemplateRootSlot = RecordTemplateSlots["slot:root"];
export type StaticTemplateSlotUnion = StaticTemplateSlots[`cell:${"name" | "count"}`];
"#;

/// Vendored source bytes of `/fixtures/template_literal_inference.ts` (the
/// registry is the source-byte authority). Inlined verbatim (PURE owned
/// `&'static str`); the guard
/// `inlined_registry_source_is_byte_identical_to_fixture_files` asserts
/// byte-identity with `fixtures/template_literal_inference.ts`.
#[allow(dead_code)]
pub(crate) const TEMPLATE_LITERAL_INFERENCE_SOURCE: &str = r#"// @ai-generated - Synthetic template-literal pattern-matching fixture.

export type SplitOn<S extends string, D extends string> = S extends `${infer H}${D}${infer T}`
  ? [H, ...SplitOn<T, D>]
  : [S];

export type DotSplitAbc = SplitOn<"a.b.c", ".">;

export type StripPrefix<S extends string, P extends string> = S extends `${P}${infer Rest}`
  ? Rest
  : S;
export type StripOnPrefix<S> = S extends `on${infer Rest}` ? Uncapitalize<Rest> : S;
export type StripOnClick = StripOnPrefix<"onClick">;
export type StripOnUnused = StripOnPrefix<"submit">;

export type EventHandlers<T extends string> = {
  [K in T as `on${Capitalize<K>}`]: (payload: K) => void;
};
export type CounterHandlers = EventHandlers<"inc" | "dec">;

export type StaticDigit<S extends string> = S extends `${infer D extends number}` ? D : never;
export type Digit42 = StaticDigit<"42">;
"#;
