import { compile } from '@verter/native'

export interface VerterCompileResult {
  code: string
  errors: string[]
}

/**
 * Compile a Vue SFC using Verter
 * Single-pass compilation (timing handled by tinybench)
 */
export function compileVerter(source: string, filename: string = 'anonymous.vue'): VerterCompileResult {
  const errors: string[] = []

  try {
    const result = compile(source, {
      filename,
      skipSourceMap: true
    })

    return {
      code: result.code || '',
      errors: result.errors || []
    }

  } catch (error) {
    errors.push(error instanceof Error ? error.message : String(error))
    return {
      code: '',
      errors
    }
  }
}
