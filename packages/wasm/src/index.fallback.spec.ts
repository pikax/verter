/**
 * @ai-generated - Tests for the Uint8Array fallback path when compileBytes is not available.
 * Simulates an older WASM build that doesn't export compileBytes.
 */
import { describe, it, expect, vi, beforeEach } from 'vitest';

const MOCK_RESULT = {
  code: 'compiled code',
  sourceMap: '{}',
  codeWithSourceMap: 'compiled code\n//# sourceMappingURL=...',
  durationMs: 1.5,
};

const mockCompile = vi.fn(() => MOCK_RESULT);
const mockInit = vi.fn(async () => {});

// Mock without compileBytes to test the fallback path
vi.mock('../wasm/verter_wasm.js', () => ({
  default: mockInit,
  compile: mockCompile,
  compileBytes: undefined, // Explicitly absent to simulate older WASM build
}));

const { compile, compileSync, initialize } = await import('./index.js');

beforeEach(async () => {
  mockCompile.mockClear();
  mockInit.mockClear();
  await initialize();
});

describe('Uint8Array fallback (no compileBytes)', () => {
  // @ai-generated - Falls back to decoding Uint8Array to string when compileBytes is unavailable
  it('should decode Uint8Array to string and call wasmCompile', async () => {
    const input = '<template><div>hello</div></template>';
    const bytes = new TextEncoder().encode(input);
    const result = await compile(bytes);

    expect(mockCompile).toHaveBeenCalledWith(input, undefined);
    expect(result).toEqual(MOCK_RESULT);
  });

  // @ai-generated - Fallback works with compileSync too
  it('should decode Uint8Array to string in compileSync', () => {
    const input = '<template><div>hello</div></template>';
    const bytes = new TextEncoder().encode(input);
    const result = compileSync(bytes);

    expect(mockCompile).toHaveBeenCalledWith(input, undefined);
    expect(result).toEqual(MOCK_RESULT);
  });

  // @ai-generated - Invalid UTF-8 bytes throw descriptive error via fallback
  it('should throw on invalid UTF-8 bytes', async () => {
    const invalidUtf8 = new Uint8Array([0x80, 0x81, 0x82]);

    await expect(compile(invalidUtf8)).rejects.toThrow();
  });

  // @ai-generated - Invalid UTF-8 bytes throw in compileSync via fallback
  it('should throw on invalid UTF-8 bytes in compileSync', () => {
    const invalidUtf8 = new Uint8Array([0x80, 0x81, 0x82]);

    expect(() => compileSync(invalidUtf8)).toThrow();
  });
});
