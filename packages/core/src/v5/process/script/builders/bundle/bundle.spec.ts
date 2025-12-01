/**
 * @ai-generated - This test file was generated with AI assistance.
 * Tests buildBundle and ResolveBundleFilename functions:
 * - Bundle file generation with correct imports
 * - Component name resolution from filename
 * - Integration with ResolveOptionsFilename
 */
import { MagicString } from "@vue/compiler-sfc";
import { parser } from "../../../../parser/parser.js";
import { buildBundle, ResolveBundleFilename } from "./index.js";
import { ParsedBlockScript } from "../../../../parser/types.js";

describe("process script builders bundle", () => {
  function parse(
    content: string,
    wrapper: string | false = false,
    lang = "ts",
    filename = "test.vue"
  ) {
    const prepend = `<script ${
      wrapper === false ? "setup" : ""
    } lang="${lang}">`;
    const source = `${prepend}${content}</script>`;
    const parsed = parser(source);

    const s = new MagicString(source);

    const scriptBlock = parsed.blocks.find(
      (x) => x.type === "script"
    ) as ParsedBlockScript;

    return buildBundle(scriptBlock.result.items, {
      isAsync: scriptBlock.result.isAsync,
      s,
      filename,
      blocks: parsed.blocks,
      block: scriptBlock,
      blockNameResolver: (name) => name,
    });
  }

  describe("ResolveBundleFilename", () => {
    it("returns bundle.ts filename", () => {
      const result = ResolveBundleFilename({
        blockNameResolver: (name) => name,
      });
      expect(result).toBe("bundle.ts");
    });

    it("uses blockNameResolver to transform filename", () => {
      const result = ResolveBundleFilename({
        blockNameResolver: (name) => `prefix/${name}`,
      });
      expect(result).toBe("prefix/bundle.ts");
    });
  });

  describe("buildBundle", () => {
    it("generates correct bundle output", () => {
      const { result } = parse(`let a = 0; import { a } from "b";`);
      // Check key parts of the output
      expect(result).toContain(`import { ___VERTER___Component }`);
      expect(result).toContain(`from "./options"`);
      expect(result).toContain(
        `declare const Test: typeof ___VERTER___Component`
      );
      expect(result).toContain(`export default Test;`);
    });

    it("removes original script content", () => {
      const { result } = parse(`let a = 0; import { a } from "b";`);
      // Original content should be removed
      expect(result).not.toContain(`let a = 0`);
      expect(result).not.toContain(`import { a } from "b"`);
    });

    describe("component name resolution", () => {
      it("capitalizes and camelizes filename for component name", () => {
        const { result } = parse(`const foo = 1`, false, "ts", "my-component.vue");
        expect(result).toContain(`declare const MyComponent: typeof ___VERTER___Component`);
        expect(result).toContain(`export default MyComponent;`);
      });

      it("handles simple filename", () => {
        const { result } = parse(`const foo = 1`, false, "ts", "app.vue");
        expect(result).toContain(`declare const App: typeof ___VERTER___Component`);
        expect(result).toContain(`export default App;`);
      });

      it("handles filename with path", () => {
        const { result } = parse(`const foo = 1`, false, "ts", "src/components/user-profile.vue");
        expect(result).toContain(`declare const UserProfile: typeof ___VERTER___Component`);
        expect(result).toContain(`export default UserProfile;`);
      });

      it("handles filename with multiple dots", () => {
        const { result } = parse(`const foo = 1`, false, "ts", "my-comp.setup.vue");
        expect(result).toContain(`declare const MyComp: typeof ___VERTER___Component`);
        expect(result).toContain(`export default MyComp;`);
      });

      it("handles empty filename edge case", () => {
        // When filename is empty, the component name falls back to "Component"
        const { result } = parse(`const foo = 1`, false, "ts", "");
        expect(result).toContain(`import { ___VERTER___Component }`);
        expect(result).toContain(`declare const Component: typeof ___VERTER___Component`);
        expect(result).toContain(`export default Component;`);
      });
    });

    describe("import generation", () => {
      it("imports component from options file", () => {
        const { result } = parse(`const foo = 1`);
        expect(result).toContain(`import { ___VERTER___Component } from "./options"`);
      });

      it("uses custom blockNameResolver for options path", () => {
        const prepend = `<script setup lang="ts">`;
        const content = `const foo = 1`;
        const source = `${prepend}${content}</script>`;
        const parsed = parser(source);

        const s = new MagicString(source);

        const scriptBlock = parsed.blocks.find(
          (x) => x.type === "script"
        ) as ParsedBlockScript;

        const { result } = buildBundle(scriptBlock.result.items, {
          isAsync: scriptBlock.result.isAsync,
          s,
          filename: "test.vue",
          blocks: parsed.blocks,
          block: scriptBlock,
          blockNameResolver: (name) => `virtual/${name}.tsx`,
        });

        // The import should use the resolved options filename
        expect(result).toContain(`from "./options.tsx"`);
      });
    });

    describe("with different script content", () => {
      it("handles empty script", () => {
        const { result } = parse(``);
        expect(result).toContain(`import { ___VERTER___Component }`);
        expect(result).toContain(`declare const Test: typeof ___VERTER___Component`);
        expect(result).toContain(`export default Test;`);
      });

      it("handles script with defineProps", () => {
        const { result } = parse(`const props = defineProps<{ msg: string }>()`);
        // Original content should be removed
        expect(result).not.toContain(`defineProps`);
        expect(result).toContain(`import { ___VERTER___Component }`);
      });

      it("handles script with async setup", () => {
        const { result } = parse(`const data = await fetch('/api')`);
        // Original async content should be removed
        expect(result).not.toContain(`await`);
        expect(result).not.toContain(`fetch`);
        expect(result).toContain(`import { ___VERTER___Component }`);
      });

      it("handles script with imports", () => {
        const { result } = parse(`import { ref } from 'vue'; const count = ref(0)`);
        // Original imports should be removed
        expect(result).not.toContain(`import { ref }`);
        expect(result).not.toContain(`ref(0)`);
        expect(result).toContain(`import { ___VERTER___Component }`);
      });
    });
  });
});
