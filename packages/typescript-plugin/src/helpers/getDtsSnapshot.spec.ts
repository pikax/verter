/**
 * @ai-generated - Tests for getDtsSnapshot using VerterHost (Rust-backed SFC compilation).
 */
import { describe, it, expect, beforeEach, vi } from "vitest";
import { parseFile } from "./getDtsSnapshot";

const mockLogger = {
  info: vi.fn(),
  msg: vi.fn(),
} as any;

beforeEach(() => {
  vi.clearAllMocks();
});

describe("parseFile", () => {
  it("compiles a simple SFC with script setup", () => {
    const sfc = `
<script setup lang="ts">
const msg = "hello";
</script>

<template>
  <div>{{ msg }}</div>
</template>
`;
    const result = parseFile("/test/Simple.vue", sfc, mockLogger);

    expect(result).not.toBe("export default {} as any");
    expect(result).toContain("msg");
  });

  it("compiles defineProps with types", () => {
    const sfc = `
<script setup lang="ts">
const props = defineProps<{ title: string; count?: number }>();
</script>

<template>
  <h1>{{ props.title }}</h1>
</template>
`;
    const result = parseFile("/test/Props.vue", sfc, mockLogger);

    expect(result).not.toBe("export default {} as any");
    expect(result).toContain("title");
  });

  it("returns fallback stub for SFC with no script block", () => {
    const sfc = `
<template>
  <div>static content</div>
</template>
`;
    const result = parseFile("/test/NoScript.vue", sfc, mockLogger);

    // Should still produce output (template-only component)
    expect(typeof result).toBe("string");
    expect(result.length).toBeGreaterThan(0);
  });

  it("returns fallback stub for empty input", () => {
    const result = parseFile("/test/Empty.vue", "", mockLogger);

    expect(typeof result).toBe("string");
    expect(result.length).toBeGreaterThan(0);
  });

  it("returns consistent output for same content", () => {
    const sfc = `
<script setup lang="ts">
const x = 1;
</script>

<template>
  <span>{{ x }}</span>
</template>
`;
    const result1 = parseFile("/test/Cache.vue", sfc, mockLogger);
    const result2 = parseFile("/test/Cache.vue", sfc, mockLogger);

    expect(result1).toBe(result2);
  });

  it("compiles SFC with defineEmits", () => {
    const sfc = `
<script setup lang="ts">
const emit = defineEmits<{ change: [value: string] }>();
</script>

<template>
  <button @click="emit('change', 'hi')">Click</button>
</template>
`;
    const result = parseFile("/test/Emits.vue", sfc, mockLogger);

    expect(result).not.toBe("export default {} as any");
    expect(result).toContain("emit");
  });

  // @ai-generated - Verifies generated TSX imports @verter/types (the module we need to resolve)
  it("generated TSX contains @verter/types import", () => {
    const sfc = `
<script setup lang="ts">
const props = defineProps<{ title: string }>();
</script>

<template>
  <h1>{{ props.title }}</h1>
</template>
`;
    const result = parseFile("/test/VerterTypes.vue", sfc, mockLogger);

    expect(result).toContain('@verter/types');
    expect(result).not.toContain('$verter/types$');
  });
});
