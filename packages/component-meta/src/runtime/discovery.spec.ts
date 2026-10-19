import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { parseTsconfig } from "./discovery.js";
import { hashTsconfigConfig } from "./tsconfig.js";
import type { CheckerWorkspace } from "../compat/checker.js";

describe("tsconfig JSONC discovery", () => {
  it("preserves URLs, alias globs and escaped quotes while accepting comments and trailing commas", async () => {
    const config = {
      $schema: "https://json.schemastore.org/tsconfig",
      compilerOptions: { paths: { "@/*": ["src/*"] } },
      include: ["src/**/*.vue"],
      description: 'quote: " // still a string',
    };
    const raw = `/* configuration */\n${JSON.stringify(config).slice(0, -1)}, /* trailing */ }`;
    const ws: Pick<CheckerWorkspace, "readFile"> = { readFile: async () => raw };
    expect((await parseTsconfig("/project/tsconfig.json", ws))?.config).toEqual(config);
  });

  it("tracks an inherited input reached again at a shallower depth", () => {
    const root = mkdtempSync(join(tmpdir(), "verter-config-identity-"));
    try {
      const config = join(root, "tsconfig.json");
      writeFileSync(config, JSON.stringify({ extends: ["./deep0.json", "./base.json"] }));
      for (let i = 0; i < 7; i++) {
        writeFileSync(
          join(root, `deep${i}.json`),
          JSON.stringify({ extends: i === 6 ? "./base.json" : `./deep${i + 1}.json` }),
        );
      }
      writeFileSync(join(root, "base.json"), '{ "extends": "./roots.json" }');
      writeFileSync(join(root, "roots.json"), '{ "extends": "./members.json" }');
      writeFileSync(join(root, "members.json"), '{ "files": ["a.ts", "b.ts"] }');
      const before = hashTsconfigConfig(config);
      writeFileSync(join(root, "unrelated.ts"), "export const value = 1;");
      expect(hashTsconfigConfig(config)).toBe(before);
      writeFileSync(join(root, "members.json"), '{ "files": ["b.ts", "a.ts"] }');
      expect(hashTsconfigConfig(config)).not.toBe(before);
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });
});
