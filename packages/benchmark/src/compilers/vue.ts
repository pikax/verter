import { parse, compileScript, compileTemplate } from '@vue/compiler-sfc'

export interface VueCompileResult {
  code: string
  errors: string[]
}

/**
 * Compile a Vue SFC using the official @vue/compiler-sfc
 * Uses 2-phase compilation:
 * 1. Parse the SFC
 * 2. Compile script setup
 * 3. Compile template with binding metadata from script
 */
export function compileVue(source: string, filename: string = 'anonymous.vue'): VueCompileResult {
  const errors: string[] = []
  
  try {
    // Phase 1: Parse the SFC
    const { descriptor, errors: parseErrors } = parse(source, {
      filename
    })
    
    if (parseErrors.length > 0) {
      errors.push(...parseErrors.map(e => e.message))
      return { code: '', errors }
    }
    
    // Phase 2: Compile script (if present)
    let scriptCode = ''
    let bindingMetadata: any = undefined
    
    if (descriptor.script || descriptor.scriptSetup) {
      const scriptResult = compileScript(descriptor, {
        id: filename,
        inlineTemplate: false
      })
      scriptCode = scriptResult.content
      bindingMetadata = scriptResult.bindings
    }
    
    // Phase 3: Compile template with binding metadata
    let templateCode = ''
    
    if (descriptor.template) {
      const templateResult = compileTemplate({
        source: descriptor.template.content,
        filename,
        id: filename,
        scoped: descriptor.styles.some(s => s.scoped),
        compilerOptions: {
          mode: 'module',
          bindingMetadata
        }
      })
      
      if (templateResult.errors.length > 0) {
        errors.push(...templateResult.errors.map(e => typeof e === 'string' ? e : e.message))
      }
      
      templateCode = templateResult.code
    }
    
    // Combine script and template
    const code = [scriptCode, templateCode].filter(Boolean).join('\n\n')
    
    return { code, errors }
    
  } catch (error) {
    errors.push(error instanceof Error ? error.message : String(error))
    return { code: '', errors }
  }
}
