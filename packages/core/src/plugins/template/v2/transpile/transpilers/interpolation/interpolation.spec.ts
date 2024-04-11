import { fromTranspiler } from "../spec.helpers";
import Interpolation from "./";

describe("tranpiler element", () => {
  function transpile(
    source: string,
    options?: {
      webComponents: string[];
    }
  ) {
    return fromTranspiler(Interpolation, source, [], options);
  }

  it("should handle interpolation", () => {
    const { result } = transpile(`{{ test }}`);
    expect(result).toMatchInlineSnapshot(`"{ ___VERTER___ctx.test }"`);
  });
});
