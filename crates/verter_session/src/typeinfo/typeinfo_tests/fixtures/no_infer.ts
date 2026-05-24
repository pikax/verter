// @ai-generated - Synthetic NoInfer<T> typeinfo fixture.

export declare function pickValue<T>(value: T, fallback: NoInfer<T>): T;

// Call site where T is fixed by the first argument; fallback is checked but
// does NOT contribute to T inference. With "ok" as const, T is inferred to
// "ok"; the second argument is also "ok" so the call succeeds. The result is
// the literal "ok".
export function noInferFixedLiteralCall() {
  return pickValue("ok" as const, "ok");
}
export type NoInferFixedLiteralResult = ReturnType<typeof noInferFixedLiteralCall>;

// Component-like default pattern: T is locked by the props arg, and the
// defaults parameter cannot widen T.
export type ComponentProps<TVariant extends string> = {
  variant: TVariant;
  label: string;
};
export declare function makeComponent<TVariant extends string>(
  props: ComponentProps<TVariant>,
  defaults: NoInfer<Partial<ComponentProps<TVariant>>>,
): ComponentProps<TVariant>;

export function noInferComponentCall() {
  return makeComponent({ variant: "primary" as const, label: "Save" }, { label: "Save" });
}
export type NoInferComponentResult = ReturnType<typeof noInferComponentCall>;
