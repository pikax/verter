import { bench, describe } from "vitest";
import { ValidFiles } from "../utils.js";
import {
  parseStringVerter,
  parseVerterLoose,
  parseStringVolar,
  processVerterScript,
} from "./utils.js";

describe("process", () => {
  describe.each(Object.keys(ValidFiles))("single %s", (x) => {
    const file = ValidFiles[x];

    let verterParsed: ReturnType<typeof parseStringVerter>;
    let volarParsed: ReturnType<typeof parseStringVolar>;

    bench(
      "Verter",
      () => {
        processVerterScript(verterParsed);
      },
      {
        setup() {
          verterParsed = parseStringVerter(file);
        },
      }
    );

    bench(
      "Volar",
      () => {
        volarParsed.embeddedCodes.flatMap((x) => x.embeddedCodes);
      },
      {
        setup() {
          volarParsed = parseStringVolar(file);
        },
      }
    );
  });
});
