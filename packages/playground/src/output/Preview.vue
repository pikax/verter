<script setup lang="ts">
import { ref, watch, onMounted, computed } from 'vue'
import type { Store } from '../core/store'
import srcdocTemplate from './srcdoc.html?raw'

const props = defineProps<{
  store: Store
}>()

const iframe = ref<HTMLIFrameElement>()
const runtimeError = ref<string>('')

const allCss = computed(() => {
  return Object.values(props.store.files)
    .map((f) => f.compiled.css)
    .filter(Boolean)
    .join('\n')
})

// Import maps must be present before any module scripts load (can't be added dynamically)
const srcdoc = computed(() => {
  const importMapScript = `<script type="importmap">${JSON.stringify(props.store.importMap)}<\/script>`
  return srcdocTemplate.replace('</head>', `${importMapScript}\n  </head>`)
})

// Transform 'as' to ':' for destructuring (import uses 'as', destructuring uses ':')
function transformImportList(imports: string): string {
  return imports.replace(/(\w+)\s+as\s+(\w+)/g, '$1: $2')
}

// Transform compiled code to work in preview iframe
function transformForPreview(code: string, moduleName: string): string {
  let transformed = code

  // Transform: import { x, y as z } from 'vue' -> const { x, y: z } = window.Vue
  transformed = transformed.replace(
    /import\s+\{([^}]+)\}\s+from\s+['"]vue['"]/g,
    (_, imports) => `const {${transformImportList(imports)}} = window.Vue`
  )

  // Transform: import x from 'vue' -> const x = window.Vue
  transformed = transformed.replace(
    /import\s+(\w+)\s+from\s+['"]vue['"]/g,
    (_, name) => `const ${name} = window.Vue`
  )

  // Transform: import { x } from './File.vue' -> const { x } = window.__modules__['./File.js']
  transformed = transformed.replace(
    /import\s+\{([^}]+)\}\s+from\s+['"]\.\/([^'"]+)['"]/g,
    (_, imports, path) => {
      const modulePath = './' + path.replace(/\.(vue|ts)$/, '.js')
      return `const {${transformImportList(imports)}} = window.__modules__["${modulePath}"]`
    }
  )

  // Transform: import X from './File.vue' -> const X = window.__modules__['./File.js'].default
  transformed = transformed.replace(
    /import\s+(\w+)\s+from\s+['"]\.\/([^'"]+)['"]/g,
    (_, name, path) => {
      const modulePath = './' + path.replace(/\.(vue|ts)$/, '.js')
      return `const ${name} = window.__modules__["${modulePath}"].default`
    }
  )

  // Transform: export default X -> window.__modules__['moduleName'].default = X
  transformed = transformed.replace(
    /export\s+default\s+/g,
    `window.__modules__["${moduleName}"].default = `
  )

  // Transform: export function X -> window.__modules__['moduleName'].X = function X
  transformed = transformed.replace(
    /export\s+function\s+(\w+)/g,
    (_, name) => `window.__modules__["${moduleName}"].${name} = function ${name}`
  )

  // Note: standalone `function render(...)` is NOT transformed here.
  // The mergeRenderIntoComponent step in compiler.ts attaches render to the component
  // via `__sfc__.render = render`, so the function declaration must remain as-is.

  // Transform: export const/let/var X = -> window.__modules__['moduleName'].X =
  transformed = transformed.replace(
    /export\s+(const|let|var)\s+(\w+)\s*=/g,
    (_, _keyword, name) => `window.__modules__["${moduleName}"].${name} =`
  )

  // Transform: export { x, y } -> Object.assign(window.__modules__['moduleName'], { x, y })
  transformed = transformed.replace(
    /export\s+\{([^}]+)\}/g,
    (_, exports) => {
      const items = exports.split(',').map((e: string) => {
        const parts = e.trim().split(/\s+as\s+/)
        const name = parts[0]
        const alias = parts[1] || name
        return `${alias}: ${name}`
      }).join(', ')
      return `Object.assign(window.__modules__["${moduleName}"], { ${items} })`
    }
  )

  return transformed
}

function updatePreview() {
  if (!iframe.value?.contentWindow) return

  const mainFile = props.store.files[props.store.mainFile]
  if (!mainFile?.compiled.js) return

  runtimeError.value = ''

  const scripts: string[] = []

  // Add all compiled JS as modules (transformed to work without ES module imports)
  for (const [filename, file] of Object.entries(props.store.files)) {
    if (file.compiled.js) {
      const moduleName = './' + filename.replace(/\.(vue|ts)$/, '.js')
      const transformed = transformForPreview(file.compiled.js, moduleName)
      scripts.push(`
        window.__modules__["${moduleName}"] = {}
        ${transformed}
      `)
    }
  }

  // Mount the app
  const mainModule = './' + props.store.mainFile.replace(/\.(vue|ts)$/, '.js')
  scripts.push(`
    const { createApp } = window.Vue
    const Component = window.__modules__["${mainModule}"]?.default
    if (Component) {
      const app = createApp(Component)
      app.mount('#app')
    }
  `)

  iframe.value.contentWindow.postMessage(
    {
      action: 'eval',
      scripts,
      css: allCss.value,
      darkMode: props.store.darkMode,
    },
    '*'
  )
}

onMounted(() => {
  window.addEventListener('message', (e) => {
    if (e.data.action === 'error') {
      runtimeError.value = e.data.message
    } else if (e.data.action === 'console') {
      console[e.data.method as 'log']('[preview]', ...e.data.args)
    }
    // Note: 'ready' message is just informational, don't trigger updatePreview to avoid loops
  })
})

// Trigger preview when iframe loads
function onIframeLoad() {
  // Small delay to ensure iframe's script has initialized
  setTimeout(() => updatePreview(), 100)
}

watch(
  () => [
    props.store.activeFile?.compiled.js,
    props.store.darkMode,
    allCss.value,
  ],
  () => {
    updatePreview()
  },
  { deep: true }
)
</script>

<template>
  <div class="preview-container">
    <iframe ref="iframe" class="preview-iframe" :srcdoc="srcdoc" sandbox="allow-scripts allow-same-origin" @load="onIframeLoad" />
    <div v-if="runtimeError" class="runtime-error">
      <strong>Runtime Error:</strong>
      <pre>{{ runtimeError }}</pre>
    </div>
  </div>
</template>

<style scoped>
.preview-container {
  height: 100%;
  width: 100%;
  display: flex;
  flex-direction: column;
  background: var(--bg-primary);
}

.preview-iframe {
  flex: 1;
  border: none;
  width: 100%;
  background: white;
}

html.dark .preview-iframe {
  background: #1a1a1a;
}

.runtime-error {
  padding: 12px;
  background: #fff0f0;
  border-top: 2px solid var(--error-color);
  color: var(--error-color);
  font-size: 13px;
}

html.dark .runtime-error {
  background: #2a1a1a;
}

.runtime-error pre {
  margin-top: 8px;
  font-size: 12px;
  white-space: pre-wrap;
  word-break: break-word;
}
</style>
