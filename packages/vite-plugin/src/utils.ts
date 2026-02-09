import type { ViteCodegenResult } from '@verter/native';

/**
 * Query parameters for Vue virtual modules
 */
export interface VueQuery {
  vue: boolean;
  type?: 'script' | 'template' | 'style';
  index?: number;
  scoped?: boolean;
  lang?: string;
}

/**
 * Parsed Vue request
 */
export interface ParsedVueRequest {
  filename: string;
  query: VueQuery;
}

/**
 * Parse a Vue request ID into filename and query parameters
 */
export function parseVueRequest(id: string): ParsedVueRequest {
  const [filename, queryString] = id.split('?', 2);

  if (!queryString) {
    return { filename, query: { vue: false } };
  }

  const params = new URLSearchParams(queryString);

  const query: VueQuery = {
    vue: params.has('vue'),
    type: params.get('type') as VueQuery['type'],
    index: params.has('index') ? parseInt(params.get('index')!, 10) : undefined,
    scoped: params.has('scoped'),
    lang: params.get('lang') || undefined,
  };

  return { filename, query };
}

/**
 * Cache for compiled SFC results
 */
const cache = new Map<string, ViteCodegenResult>();

/**
 * Get cached compilation result
 */
export function getDescriptor(filename: string): ViteCodegenResult | undefined {
  return cache.get(filename);
}

/**
 * Set cached compilation result
 */
export function setDescriptor(filename: string, result: ViteCodegenResult): void {
  cache.set(filename, result);
}

/**
 * Delete cached compilation result
 */
export function deleteDescriptor(filename: string): void {
  cache.delete(filename);
}

/**
 * Clear all cached results
 */
export function clearCache(): void {
  cache.clear();
}
