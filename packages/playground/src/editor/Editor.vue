<script setup lang="ts">
import { ref, watch, onMounted, onUnmounted, shallowRef } from 'vue'
import * as monaco from 'monaco-editor-core'
import { IMPORT_MAP_FILENAME, type Store } from '../core/store'

const props = defineProps<{
  store: Store
}>()

const editorContainer = ref<HTMLElement>()
const editor = shallowRef<monaco.editor.IStandaloneCodeEditor>()
const pendingCode = ref<string | null>(null)

function getLanguage(filename: string): string {
  if (filename.endsWith('.vue')) return 'vue'
  if (filename.endsWith('.ts')) return 'typescript'
  if (filename.endsWith('.js')) return 'javascript'
  if (filename.endsWith('.css')) return 'css'
  if (filename.endsWith('.json')) return 'json'
  return 'plaintext'
}

function saveAndCompile() {
  const value = editor.value?.getValue()
  if (value !== undefined) {
    props.store.updateCode(value)
    props.store.recompile()
    pendingCode.value = null
  }
}

onMounted(() => {
  if (!editorContainer.value) return

  editor.value = monaco.editor.create(editorContainer.value, {
    value: props.store.activeFile?.code ?? '',
    language: getLanguage(props.store.activeFilename),
    theme: props.store.darkMode ? 'vs-dark' : 'vs',
    minimap: { enabled: false },
    fontSize: 14,
    lineNumbers: 'on',
    renderLineHighlight: 'line',
    scrollBeyondLastLine: false,
    automaticLayout: true,
    tabSize: 2,
    wordWrap: 'on',
  })

  // Add Ctrl+S / Cmd+S keybinding
  editor.value.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS, () => {
    saveAndCompile()
  })

  editor.value.onDidChangeModelContent(() => {
    const value = editor.value?.getValue()
    if (value !== undefined) {
      if (props.store.activeFilename === IMPORT_MAP_FILENAME) {
        props.store.updateImportMap(value)
      } else if (props.store.autoSave) {
        props.store.updateCode(value)
      } else {
        // Store pending changes but don't compile
        pendingCode.value = value
        // Still update the file code for display, but compilation won't auto-trigger
        props.store.updateCode(value)
      }
    }
  })

  watch(
    () => props.store.activeFilename,
    (filename) => {
      const file = props.store.activeFile
      if (file && editor.value) {
        const model = monaco.editor.createModel(
          file.code,
          getLanguage(filename)
        )
        editor.value.setModel(model)
        pendingCode.value = null
      }
    }
  )

  watch(
    () => props.store.darkMode,
    (dark) => {
      monaco.editor.setTheme(dark ? 'vs-dark' : 'vs')
    }
  )
})

onUnmounted(() => {
  editor.value?.dispose()
})
</script>

<template>
  <div class="editor-wrapper">
    <div ref="editorContainer" class="editor-container" />
  </div>
</template>

<style scoped>
.editor-wrapper {
  height: 100%;
  width: 100%;
  display: flex;
  flex-direction: column;
}

.editor-container {
  flex: 1;
  min-height: 0;
}
</style>
