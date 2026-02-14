import { compile } from '@verter/native'

export interface VerterCompileResult {
  code: string
  errors: string[]
  durationMs: number
}

/**
 * Compile a Vue SFC using Verter
 * Single-pass compilation with built-in timing
 */
export function compileVerter(source: string, filename: string = 'anonymous.vue'): VerterCompileResult {
  const errors: string[] = []
  
  try {
    const result = compile(source, {
      filename,
      skipSourceMap: true
    })
    
    // Verter returns duration_ms from Rust layer
    return {
      code: result.code || '',
      errors: result.errors || [],
      durationMs: result.duration_ms || 0
    }
    
  } catch (error) {
    errors.push(error instanceof Error ? error.message : String(error))
    return {
      code: '',
      errors,
      durationMs: 0
    }
  }
}
