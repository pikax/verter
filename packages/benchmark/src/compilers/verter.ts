import { VerterHost } from '@verter/native'

export interface VerterCompileResult {
  code: string
  errors: string[]
}

/**
 * Create a VerterHost instance for benchmarking.
 * @param analysisLevel - "full" (default), "essential", or "none"
 */
export function createVerterHost(analysisLevel: 'full' | 'essential' | 'none' = 'full'): VerterHost {
  return new VerterHost({
    devMode: false,
    analysisLevel
  } as any)
}

const hostProfile = {
  sourceMap: false
}

/**
 * Compile a Vue SFC using VerterHost (new AST-based pipeline, stateful).
 * Forces recompilation by calling remove() before upsert() to defeat caching.
 * Uses camelCase field names as required by the NAPI runtime.
 */
export function compileVerterHost(host: VerterHost, source: string, filename: string = 'anonymous.vue'): VerterCompileResult {
  try {
    // Remove any cached version to force recompilation
    ;(host as any).remove(filename)

    const result = (host as any).upsert({
      inputId: filename,
      source,
      compileProfile: hostProfile
    })

    let code = ''
    const scriptFile = (host as any).getVirtualFile({
      canonicalId: result.canonicalId,
      nodeKind: { kind: 'script' },
      compileProfile: hostProfile
    })
    if (scriptFile) code += scriptFile.code

    const templateFile = (host as any).getVirtualFile({
      canonicalId: result.canonicalId,
      nodeKind: { kind: 'template' },
      compileProfile: hostProfile
    })
    if (templateFile) code += '\n\n' + templateFile.code

    return {
      code,
      errors: result.diagnostics?.diagnostics?.map((d: any) => d.message) || []
    }
  } catch (error) {
    return {
      code: '',
      errors: [error instanceof Error ? error.message : String(error)]
    }
  }
}
