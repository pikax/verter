/**
 * Type contracts of the template condition narrowing helpers: snapshot
 * re-narrowing inside a callback and the `v-for` frame alias types.
 */
import { describe, it, assertType } from "vitest";
import type { flowNarrow, flowExcluded, flowEach1, flowEach2, flowEach3 } from "./flow";

declare const narrow: typeof flowNarrow;
declare const excluded: typeof flowExcluded;
declare const each1: typeof flowEach1;
declare const each2: typeof flowEach2;
declare const each3: typeof flowEach3;

type U = { kind: "a"; a: string } | { kind: "b"; b: number };

describe("flow helpers", () => {
  it("re-narrow a mutable property inside a callback from its snapshot", () => {
    const state = { u: { kind: "a", a: "" } as U };
    if (state.u.kind === "a") {
      const o = state.u;
      const callback = () => {
        if (!narrow(state.u, o) || excluded(o)(state.u)) throw 0;
        return state.u.a;
      };
      assertType<() => string>(callback);
      const unguarded = () => {
        // @ts-expect-error a callback starts from the declared, un-narrowed type
        return state.u.a;
      };
      void unguarded;
    }
  });

  it("type v-for aliases like Vue's renderList", () => {
    assertType<string>(each1(["a"]));
    assertType<number>(each1(3));
    assertType<string>(each1("ab"));
    assertType<[string, number]>(each1(new Map<string, number>()));
    assertType<number>(each1({ a: 1 }));
    assertType<string>(each1(undefined as string[] | undefined));
    assertType<[boolean, number]>(each2([true]));
    assertType<[number, "a" | "b"]>(each2({ a: 1, b: 2 }));
    assertType<[number, "a", number]>(each3({ a: 1 }));
    // @ts-expect-error the alias is the element type, not any
    assertType<number>(each1(["a"]));
  });

  it("keep a generic array element", () => {
    function generic<T extends { name: string }, L extends T[]>(list: L) {
      assertType<string>(each1(list).name);
    }
    void generic;
  });
});
