import { fromTranspiler } from "../spec.helpers";
import Root from "./";

describe("tranpiler element", () => {
  function transpile(
    source: string,
    options?: {
      webComponents: string[];
    }
  ) {
    return fromTranspiler(Root, source, [], options, true);
  }
  test("no children", () => {
    const { result } = transpile(`<template></template>`);
    expect(result).toMatchInlineSnapshot(`"<></>"`);
  });

  it("should remove empty", () => {
    const { result } = transpile(`<template>test</template>`);
    expect(result).toMatchInlineSnapshot(`"<>test</>"`);
  });
});
