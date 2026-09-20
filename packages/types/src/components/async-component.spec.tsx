/**
 * The template-side normalization of a `defineAsyncComponent` result.
 */
import "../tsx/tsx";
import { describe, it, assertType } from "vitest";
import { defineAsyncComponent, defineComponent } from "vue";
import { asyncComponent } from "./components";

describe("asyncComponent", () => {
  it("renders a raw options loader that direct JSX rejects", () => {
    const Raw = defineAsyncComponent(async () => ({ template: "<span>async</span>" }));
    // @ts-expect-error the raw options object has no construct or call signature
    void (<Raw />);
    const Normalized = asyncComponent(Raw);
    void (<Normalized />);
  });

  it("derives the raw options contract from its actual props and emits", () => {
    const Raw = defineAsyncComponent(async () => ({
      props: { id: { type: String, required: true }, count: Number },
      emits: { change: (value: number) => typeof value === "number" },
      template: "<span/>",
    }));
    const Normalized = asyncComponent(Raw);
    void (<Normalized id="a" count={1} onChange={(value) => assertType<number>(value)} />);
    // @ts-expect-error a required prop stays required
    void (<Normalized count={1} />);
    // @ts-expect-error a declared prop keeps its type
    void (<Normalized id="a" count="one" />);

    const Listed = asyncComponent(
      defineAsyncComponent(async () => ({ props: ["label"] as const, template: "<i/>" })),
    );
    void (<Listed label={1} />);
  });

  it("preserves constructors exactly, generic ones included", () => {
    const Sfc = defineComponent({ props: { message: { type: String, required: true } } });
    const Loaded = defineAsyncComponent(() => Promise.resolve(Sfc));
    assertType<typeof Sfc>(asyncComponent(Loaded));
    // @ts-expect-error the constructor is not widened to an arbitrary type
    assertType<{ unrelated: true }>(asyncComponent(Loaded));
    void (<Loaded message="hi" />);
    const Normalized = asyncComponent(Loaded);
    // @ts-expect-error normalization keeps the required prop
    void (<Normalized />);

    const Generic = null! as new <T>(props: { value: T; onPick(value: T): void }) => {
      $props: { value: T; onPick(value: T): void };
    };
    const NormalizedGeneric = asyncComponent(Generic);
    assertType<typeof Generic>(NormalizedGeneric);
    void (<NormalizedGeneric value={1} onPick={(value) => assertType<number>(value)} />);
  });

  it("preserves functional components exactly, generic ones included", () => {
    const Functional = null! as <T>(props: { value: T; onPick(value: T): void }) => any;
    const Normalized = asyncComponent(Functional);
    assertType<typeof Functional>(Normalized);
    void (<Normalized value="a" onPick={(value) => assertType<string>(value)} />);
    // @ts-expect-error the generic link between props survives
    void (<Normalized value="a" onPick={(value: number) => value} />);
  });
});
