<script setup lang="ts">
import { ref, watch, onMounted, computed } from "vue";
import type { Store } from "../core/store";
import srcdocTemplate from "./srcdoc.html?raw";
import {
  collectSvelteRuntimeFlags,
  transformForPreview,
  orderScriptsByDependency,
  type SvelteRuntimeFlag,
} from "./previewTransforms";
import { detectFrameworkId } from "../core/frameworks";
import { buildPreviewMountScript, type PreviewRuntimeFramework } from "../core/previewRuntime";

const props = defineProps<{
  store: Store;
}>();

const iframe = ref<HTMLIFrameElement>();
const runtimeError = ref<string>("");

const allCss = computed(() => {
  return Object.values(props.store.files)
    .map((f) => f.compiled.css)
    .filter(Boolean)
    .join("\n");
});

// Import maps must be present before any module scripts load (can't be added dynamically)
const srcdoc = computed(() => {
  const importMapScript = `<script type="importmap">${JSON.stringify(props.store.importMap)}<\/script>`;
  return srcdocTemplate.replace("</head>", `${importMapScript}\n  </head>`);
});

function updatePreview() {
  if (!iframe.value?.contentWindow) return;

  const mainFile = props.store.files[props.store.mainFile];
  if (!mainFile?.compiled.js) return;

  runtimeError.value = "";

  const scripts: string[] = [];
  const svelteRuntimeFlags = new Set<SvelteRuntimeFlag>();

  // Build transformed JS map, then topologically sort so dependencies evaluate first
  const transformedFiles: Record<string, string> = {};
  for (const [filename, file] of Object.entries(props.store.files)) {
    if (file.compiled.js) {
      for (const flag of collectSvelteRuntimeFlags(file.compiled.js)) {
        svelteRuntimeFlags.add(flag);
      }
      const moduleName = "./" + filename.replace(/\.(vue|svelte|ts)$/, ".js");
      transformedFiles[filename] = transformForPreview(file.compiled.js, moduleName);
    }
  }

  const ordered = orderScriptsByDependency(transformedFiles, props.store.mainFile);

  for (const filename of ordered) {
    const moduleName = "./" + filename.replace(/\.(vue|svelte|ts)$/, ".js");
    scripts.push(`
        window.__modules__["${moduleName}"] = {}
        ${transformedFiles[filename]}
      `);
  }

  // Mount through the framework-owned public runtime protocol.
  const mainModule = "./" + props.store.mainFile.replace(/\.(vue|svelte|ts)$/, ".js");
  const frameworkId: PreviewRuntimeFramework =
    detectFrameworkId(props.store.mainFile) === "svelte" ? "svelte" : "vue";
  scripts.push(buildPreviewMountScript(frameworkId, mainModule));

  iframe.value.contentWindow.postMessage(
    {
      action: "eval",
      scripts,
      css: allCss.value,
      frameworkId,
      svelteRuntimeFlags: [...svelteRuntimeFlags],
    },
    "*",
  );
}

onMounted(() => {
  window.addEventListener("message", (e) => {
    if (e.data.action === "error") {
      runtimeError.value = e.data.message;
    } else if (e.data.action === "console") {
      console[e.data.method as "log"]("[preview]", ...e.data.args);
    }
    // Note: 'ready' message is just informational, don't trigger updatePreview to avoid loops
  });
});

// Trigger preview when iframe loads
function onIframeLoad() {
  // Small delay to ensure iframe's script has initialized
  setTimeout(() => updatePreview(), 100);
}

watch(
  () => [
    // Watch all files' compiled JS (not just active file) so multi-file changes trigger preview
    ...Object.values(props.store.files).map((f) => f.compiled.js),
    allCss.value,
  ],
  () => {
    updatePreview();
  },
  { deep: true },
);
</script>

<template>
  <div class="preview-container">
    <iframe
      ref="iframe"
      class="preview-iframe"
      :srcdoc="srcdoc"
      sandbox="allow-scripts allow-same-origin"
      @load="onIframeLoad"
    />
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
