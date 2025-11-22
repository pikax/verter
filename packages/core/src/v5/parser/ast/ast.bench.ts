import { bench } from "vitest";
import { basename } from "node:path";
import {
  parseAcornLoose,
  parseOXC,
  parseAST,
  sanitisePosition,
  parseAcorn,
  parseBabel,
} from "./ast.js";
import { parse } from "oxc-parser";
import { MagicString, walk } from "@vue/compiler-sfc";
import { isFunctionType } from "@vue/compiler-core";

const validFiles = Object.entries(
  // @ts-expect-error not the correct flag on the tsconfig
  import.meta.glob("./__bench__/*.ts", {
    query: "?raw",
    eager: true,
    import: "default",
  }) as Record<string, string>
).reduce((c, [k, v]) => {
  c[basename(k)] = v;
  return c;
}, {} as Record<string, string>);

/**
 * This is to benchmark acorn and OXC
 * It seems in real usage acornloose is faster
 */
describe("ast bench", () => {
  describe.each(Object.keys(validFiles))("single %s", (x) => {
    const file = validFiles[x];
    bench("oxc async", async () => {
      await parse("index.ts", file);
    });
    bench("oxc", () => {
      parseOXC(file);
    });

    bench("acornLoose", () => {
      parseAcornLoose(file);
    });

    bench("acorn", () => {
      parseAcorn(file);
    });

    bench("parseAST", () => {
      parseAST(file);
    });

    bench("babel", () => {
      parseBabel(file);
    });
  });

  describe("multi", () => {
    bench("oxc async", async () => {
      await Promise.all(
        Object.values(validFiles).map((x) => parse("index.ts", x))
      );
    });

    bench("oxc", () => {
      Object.values(validFiles).map((x) => parseOXC(x));
    });

    bench("acornLoose", () => {
      Object.values(validFiles).map((x) => parseAcornLoose(x));
    });

    bench("acorn", () => {
      Object.values(validFiles).map((x) => parseAcorn(x));
    });

    bench("babel", () => {
      Object.values(validFiles).map((x) => parseBabel(x));
    });

    bench("parseAST", () => {
      Object.values(validFiles).map((x) => parseAST(x));
    });
  });

  describe("magicstring", () => {
    function makeChanges(s: MagicString, ast: any) {
      walk(ast, {
        enter: (n: any, p: any) => {
          if (isFunctionType(n)) {
            s.prependLeft(p.start, "VERTER_");
          }
        },
      });
      return s.toString();
    }

    describe("single", () => {
      describe.each(Object.keys(validFiles))("single %s", (x) => {
        const source = validFiles[x];

        // bench("oxc", () => {
        //   const parsed = parseOXC(source);
        //   makeChanges(parsed.magicString as any, parsed.program);
        // });

        bench("oxc + magicstring", () => {
          const parsed = parseOXC(source);

          const s = new MagicString(source);
          makeChanges(s, parsed.program);
        });

        bench("sanitised oxc + magicstring", () => {
          const parsed = parseOXC(sanitisePosition(source));

          const s = new MagicString(source);
          makeChanges(s, parsed.program);
        });

        bench("acornLoose + magicstring", () => {
          const parsed = parseAcornLoose(source);

          const s = new MagicString(source);

          makeChanges(s, parsed);
        });

        bench("babel + magicstring", () => {
          const parsed = parseBabel(source);

          const s = new MagicString(source);

          makeChanges(s, parsed);
        });

        bench("acorn + magicstring", () => {
          const parsed = parseAcorn(source);

          const s = new MagicString(source);

          makeChanges(s, parsed);
        });
      });
    });
    describe("multi", () => {
      const sources = Object.values(validFiles);

      // bench("oxc", () => {
      //   sources.map((source) => {
      //     const parsed = parseOXC(source);

      //     makeChanges(parsed.magicString as any, parsed.program);
      //   });
      // });

      bench("oxc + magicstring", () => {
        sources.map((source) => {
          const parsed = parseOXC(source);

          const s = new MagicString(source);
          makeChanges(s, parsed.program);
        });
      });

      bench("sanitised oxc + magicstring", () => {
        sources.map((source) => {
          const parsed = parseOXC(sanitisePosition(source));

          const s = new MagicString(source);
          makeChanges(s, parsed.program);
        });
      });

      bench("acornLoose + magicstring", () => {
        sources.map((source) => {
          const parsed = parseAcornLoose(source);

          const s = new MagicString(source);

          makeChanges(s, parsed);
        });
      });

      bench("babel + magicstring", () => {
        sources.map((source) => {
          const parsed = parseBabel(source);

          const s = new MagicString(source);

          makeChanges(s, parsed);
        });
      });

      bench("acorn + magicstring", () => {
        sources.map((source) => {
          const parsed = parseAcorn(source);

          const s = new MagicString(source);

          makeChanges(s, parsed);
        });
      });

      // bench("async oxc", async () => {
      //   await Promise.all(
      //     sources.map(async (source) => {
      //       const parsed = await parseAsync("test.ts", source);
      //       makeChanges(parsed.magicString as any, parsed.program);
      //     })
      //   );
      // });

      bench("async oxc + magicstring", async () => {
        await Promise.all(
          sources.map(async (source) => {
            const parsed = await parseAsync("test.ts", source);
            const s = new MagicString(source);
            makeChanges(s, parsed.program);
          })
        );
      });

      bench("async sanitised oxc + magicstring", async () => {
        await Promise.all(
          sources.map(async (source) => {
            const parsed = await parseAsync("test.ts", sanitisePosition(source));
            const s = new MagicString(source);
            makeChanges(s, parsed.program);
          })
        );
      });
    });
  });
});
