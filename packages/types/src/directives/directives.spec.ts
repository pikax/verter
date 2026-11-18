/**
 * `runCustomDirective` must carry the FOURTH type parameter of Vue's
 * `Directive<HostElement, Value, Modifiers, Arg>` into the `arg` parameter of
 * the call signature it hands back.
 *
 * This is the PUBLISHED `@verter/types` surface — what a user installs. If the
 * declaration hardcodes `arg: string | undefined` the directive's `Arg`
 * constraint is discarded and `v-color:badarg` type-checks clean in the editor.
 */
import { assertType, describe, it } from "vitest";
import { shallowRef, type ComponentPublicInstance, type Directive } from "vue";
import { shallowUnwrapRef } from "../vue/index.js";
import {
  runCustomDirective,
  retrieveSetupDirectives,
  type ExtractLeafElement,
} from "./directives.js";

declare module "vue" {
  interface GlobalDirectives {
    vPaintGlobal: Directive<SVGCircleElement, string, "lazy", "fg">;
  }
}

declare const host: HTMLDivElement;

describe("runCustomDirective arg typing", () => {
  it("accepts ordinary Vue instances without losing their directive binding contract", () => {
    const component = null! as ComponentPublicInstance;
    const vPaint = null! as Directive<HTMLDivElement, string, "lazy", "fg">;
    const apply = runCustomDirective(component, vPaint);
    apply(component, "red", "fg", { lazy: true });
    // @ts-expect-error an unknown component root does not erase the value type
    apply(component, 123, "fg", {});
    // @ts-expect-error an unknown component root does not erase the argument type
    apply(component, "red", "bg", {});
    // @ts-expect-error an unknown component root does not erase the modifier type
    apply(component, "red", "fg", { eager: true });
  });

  it("checks bindings on functional component witnesses without a declared root", () => {
    // The shared component adapter's instantiated functional surface.
    const component = null! as {
      readonly $props: { label: string };
      readonly $slots: {};
      $emit: (event: "change") => void;
    };
    const vPaint = null! as Directive<HTMLDivElement, string, "lazy", "fg">;
    const apply = runCustomDirective(component, vPaint);
    apply(component, "red", "fg", { lazy: true });
    // @ts-expect-error a functional witness retains the directive's value type
    apply(component, 123, "fg", {});
    // @ts-expect-error a functional witness retains the directive's argument type
    apply(component, "red", "bg", {});
    // @ts-expect-error a functional witness retains the directive's modifier type
    apply(component, "red", "fg", { eager: true });
    const precise = null! as typeof component & { $el: SVGCircleElement };
    // @ts-expect-error a known component root still rejects an incompatible directive
    runCustomDirective(precise, vPaint);
  });

  it("preserves SVG and MathML leaves and checks their complete binding", () => {
    const circle = null! as SVGCircleElement;
    const math = null! as MathMLElement;
    const vPaint = null! as Directive<SVGCircleElement, string, "lazy", "fg">;
    const apply = runCustomDirective(circle, vPaint);
    apply(circle, "red", "fg", { lazy: true });
    // @ts-expect-error the directive requires a string value
    apply(circle, 123, "fg", {});
    // @ts-expect-error the directive declares only the fg argument
    apply(circle, "red", "bg", {});
    // @ts-expect-error undeclared modifiers remain errors
    apply(circle, "red", "fg", { eager: true });
    // @ts-expect-error an HTML host cannot satisfy an SVG directive
    runCustomDirective(host, vPaint);
    const vMath = null! as Directive<MathMLElement, number>;
    runCustomDirective(math, vMath)(math, 1, undefined, {});
    const component = null! as { $el: SVGCircleElement };
    runCustomDirective(component, vPaint)(component, "red", "fg", {});
    assertType<SVGCircleElement>(null! as ExtractLeafElement<new () => typeof component>);
    assertType<MathMLElement>(null! as ExtractLeafElement<{ $el: MathMLElement }>);
    assertType<SVGCircleElement>(
      null! as (SVGElementTagNameMap & Record<string, SVGElement>)["circle"],
    );
    const discard = null! as (SVGElementTagNameMap & Record<string, SVGElement>)["discard"];
    const vSvg = null! as Directive<SVGElement, string>;
    runCustomDirective(discard, vSvg)(discard, "red", undefined, {});
    // @ts-expect-error a tag absent from the DOM map retains only its namespace base type
    assertType<SVGCircleElement>(discard);
  });

  it("keeps augmented global bindings exact and lets setup bindings shadow them", () => {
    const circle = null! as SVGCircleElement;
    const globals = retrieveSetupDirectives({});
    runCustomDirective(circle, globals.vPaintGlobal)(circle, "red", "fg", {});
    // @ts-expect-error the augmented global directive retains its value type
    runCustomDirective(circle, globals.vPaintGlobal)(circle, 123, "fg", {});
    const vPaintGlobal = null! as Directive<SVGCircleElement, number>;
    const locals = retrieveSetupDirectives({ vPaintGlobal });
    runCustomDirective(circle, locals.vPaintGlobal)(circle, 123, undefined, {});
    // @ts-expect-error the local override requires a number
    runCustomDirective(circle, locals.vPaintGlobal)(circle, "red", undefined, {});
  });
  it("checks the unwrapped local binding even when a same-named global exists", () => {
    const circle = null! as SVGCircleElement;
    const vPaintGlobal = shallowRef<Directive<SVGCircleElement, number>>(null!);
    const local = shallowUnwrapRef({ vPaintGlobal }).vPaintGlobal;
    runCustomDirective(circle, local)(circle, 123, undefined, {});
    // @ts-expect-error the local numeric directive shadows the string-valued global
    runCustomDirective(circle, local)(circle, "red", "fg", {});
    const invalid = shallowUnwrapRef({ vPaintGlobal: shallowRef(123) }).vPaintGlobal;
    // @ts-expect-error a non-directive local cannot select the augmented global instead
    runCustomDirective(circle, invalid);
  });
  it("constrains `arg` to the directive's Arg type parameter", () => {
    const vColor = null as unknown as Directive<HTMLDivElement, string, "lazy", "fg" | "bg">;
    const apply = runCustomDirective(host, vColor);
    apply(host, "red", "fg", {});
    apply(host, "red", "bg", { lazy: true });
    apply(host, "red", undefined, {});
    // @ts-expect-error "badarg" is not an allowed argument for this directive
    apply(host, "red", "badarg", {});
  });

  it("still accepts any argument when the directive leaves Arg at Vue's default", () => {
    const vLoose = null as unknown as Directive<HTMLDivElement, string, "lazy">;
    const apply = runCustomDirective(host, vLoose);
    apply(host, "red", "anything-at-all", {});
    apply(host, "red", undefined, { lazy: true });
  });
});
