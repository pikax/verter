/**
 * `runCustomDirective` must carry the FOURTH type parameter of Vue's
 * `Directive<HostElement, Value, Modifiers, Arg>` into the `arg` parameter of
 * the call signature it hands back.
 *
 * This is the PUBLISHED `@verter/types` surface — what a user installs. If the
 * declaration hardcodes `arg: string | undefined` the directive's `Arg`
 * constraint is discarded and `v-color:badarg` type-checks clean in the editor.
 */
import { describe, it } from "vitest";
import type { Directive } from "vue";
import { runCustomDirective } from "./directives.js";

declare const host: HTMLDivElement;

describe("runCustomDirective arg typing", () => {
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
