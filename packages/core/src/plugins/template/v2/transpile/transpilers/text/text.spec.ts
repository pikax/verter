import { fromTranspiler } from "../spec.helpers";
import Text from "./";

describe("text element", () => {
  function transpile(
    source: string,
    options?: {
      webComponents: string[];
    }
  ) {
    return fromTranspiler(Text, source, [], options);
  }

  it("should handle text", () => {
    const { result } = transpile(`test`);
    expect(result).toMatchInlineSnapshot(`"{ "test" }"`);
  });

  it("should not handle if starts < ", () => {
    const { result } = transpile(`<`);
    expect(result).toMatchInlineSnapshot(`"<"`);
  });

  it("should parse text if has <", () => {
    const { result } = transpile(`2 < 1`);
    expect(result).toMatchInlineSnapshot(`"{ "2 < 1" }"`);
  });

  it('should escape " in text', () => {
    const { result } = transpile(`"`);
    expect(result).toMatchInlineSnapshot(`"{ "\\"" }"`);
  });
});
