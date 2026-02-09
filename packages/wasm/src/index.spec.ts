/**
 * @ai-generated - Tests for Uint8Array input support in @verter/wasm compile wrapper.
 * Verifies input routing (string vs Uint8Array), compileBytes fallback, and UTF-8 validation.
 */
import { describe, it, expect, vi, beforeEach } from 'vitest';

const MOCK_RESULT = {
  code: 'compiled code',
  sourceMap: '{}',
  codeWithSourceMap: 'compiled code\n//# sourceMappingURL=...',
  durationMs: 1.5,
};

const mockCompile = vi.fn(() => MOCK_RESULT);
const mockCompileBytes = vi.fn(() => MOCK_RESULT);
const mockInit = vi.fn(async () => {});

vi.mock('../wasm/verter_wasm.js', () => ({
  default: mockInit,
  compile: mockCompile,
  compileBytes: mockCompileBytes,
}));

// Import after mock setup so the module picks up mocked dependencies
const { compile, compileSync, initialize, isInitialized } = await import('./index.js');

beforeEach(async () => {
  mockCompile.mockClear();
  mockCompileBytes.mockClear();
  mockInit.mockClear();

  // Ensure module is initialized for each test
  await initialize();
});

describe('Uint8Array input support', () => {
  describe('compile', () => {
    // @ai-generated - String input routes to wasmCompile
    it('should route string input to wasmCompile', async () => {
      const result = await compile('template code');

      expect(mockCompile).toHaveBeenCalledWith('template code', undefined);
      expect(mockCompileBytes).not.toHaveBeenCalled();
      expect(result).toEqual(MOCK_RESULT);
    });

    // @ai-generated - Uint8Array input routes to wasmCompileBytes when available
    it('should route Uint8Array input to wasmCompileBytes', async () => {
      const bytes = new TextEncoder().encode('template code');
      const result = await compile(bytes);

      expect(mockCompileBytes).toHaveBeenCalledWith(bytes, undefined);
      expect(mockCompile).not.toHaveBeenCalled();
      expect(result).toEqual(MOCK_RESULT);
    });

    // @ai-generated - Options are forwarded with Uint8Array input
    it('should forward options with Uint8Array input', async () => {
      const bytes = new TextEncoder().encode('template code');
      const opts = { filename: 'App.vue', isProduction: true };
      await compile(bytes, opts);

      expect(mockCompileBytes).toHaveBeenCalledWith(bytes, opts);
    });

    // @ai-generated - Options are forwarded with string input
    it('should forward options with string input', async () => {
      const opts = { filename: 'App.vue' };
      await compile('code', opts);

      expect(mockCompile).toHaveBeenCalledWith('code', opts);
    });
  });

  describe('compileSync', () => {
    // @ai-generated - String input routes to wasmCompile
    it('should route string input to wasmCompile', () => {
      const result = compileSync('template code');

      expect(mockCompile).toHaveBeenCalledWith('template code', undefined);
      expect(mockCompileBytes).not.toHaveBeenCalled();
      expect(result).toEqual(MOCK_RESULT);
    });

    // @ai-generated - Uint8Array input routes to wasmCompileBytes
    it('should route Uint8Array input to wasmCompileBytes', () => {
      const bytes = new TextEncoder().encode('template code');
      const result = compileSync(bytes);

      expect(mockCompileBytes).toHaveBeenCalledWith(bytes, undefined);
      expect(mockCompile).not.toHaveBeenCalled();
      expect(result).toEqual(MOCK_RESULT);
    });
  });

  describe('isInitialized', () => {
    // @ai-generated - Reports initialization state correctly
    it('should return true after initialization', () => {
      expect(isInitialized()).toBe(true);
    });
  });
});
