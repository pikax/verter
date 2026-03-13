<script setup lang="ts">
import { computed } from "vue";
import type { Store } from "../core/store";
import type { ComponentMeta } from "@verter/component-meta/browser";
import { snapshotToMeta } from "@verter/component-meta/browser";
import { formatTypeDescriptor } from "./formatTypeDescriptor";

const props = defineProps<{
  store: Store;
}>();

const meta = computed<ComponentMeta | null>(() => {
  const file = props.store.activeFile;
  if (!file?.compiled.analysis) return null;
  return snapshotToMeta(file.compiled.analysis, file.filename);
});

const activeFlagChips = computed<string[]>(() => {
  if (!meta.value) return [];
  const f = meta.value.flags;
  const chips: string[] = [];
  if (f.asyncSetup) chips.push("Async Setup");
  if (f.hasReactiveState) chips.push("Reactive State");
  if (f.hasComputed) chips.push("Computed");
  if (f.hasWatchers) chips.push("Watchers");
  if (f.hasLifecycleHooks) chips.push("Lifecycle Hooks");
  if (f.hasProvide) chips.push("Provide");
  if (f.hasInject) chips.push("Inject");
  if (f.hasInheritAttrsFalse) chips.push("inheritAttrs: false");
  if (f.hasStoreUsage) chips.push("Store Usage");
  return chips;
});
</script>

<template>
  <div class="analysis-panel">
    <div v-if="!meta" class="empty-state">
      Component meta not available
    </div>
    <div v-else class="analysis-content">
      <!-- Component Info -->
      <details class="analysis-section" open>
        <summary class="section-title">
          {{ meta.componentName }}
          <span v-if="meta.optionsApi" class="badge badge-kind">Options API</span>
        </summary>
        <div v-if="activeFlagChips.length > 0" class="flag-chips">
          <span v-for="chip in activeFlagChips" :key="chip" class="flag-chip">{{ chip }}</span>
        </div>
      </details>

      <!-- Props -->
      <details v-if="meta.props.length > 0" class="analysis-section" open>
        <summary class="section-title">Props ({{ meta.props.length }})</summary>
        <div class="import-list">
          <div v-for="(p, i) in meta.props" :key="i" class="import-item">
            <div class="binding-item">
              <code class="binding-name">{{ p.name }}</code>
              <span class="badge badge-type">{{ formatTypeDescriptor(p.type) }}</span>
              <span v-if="p.required" class="badge badge-reactive">required</span>
              <span v-if="p.hasDefault" class="badge badge-vue">default</span>
              <span v-if="p.rawType" class="binding-init">raw: {{ p.rawType }}</span>
            </div>
            <div v-if="p.description" class="jsdoc-description">{{ p.description }}</div>
            <div v-if="p.tags?.length" class="jsdoc-tags">
              <span v-for="(tag, j) in p.tags" :key="j" class="jsdoc-tag">@{{ tag.name }}<span v-if="tag.text"> {{ tag.text }}</span></span>
            </div>
          </div>
        </div>
      </details>

      <!-- Events -->
      <details v-if="meta.events.length > 0" class="analysis-section" open>
        <summary class="section-title">Events ({{ meta.events.length }})</summary>
        <div class="import-list">
          <div v-for="(e, i) in meta.events" :key="i" class="import-item">
            <div class="binding-item">
              <code class="binding-name">{{ e.name }}</code>
              <span class="badge badge-type">{{ formatTypeDescriptor(e.payload) }}</span>
              <span v-if="e.hasValidator" class="badge badge-reactive">validator</span>
              <span v-if="e.isDeclared" class="badge badge-vue">declared</span>
            </div>
            <div v-if="e.description" class="jsdoc-description">{{ e.description }}</div>
            <div v-if="e.tags?.length" class="jsdoc-tags">
              <span v-for="(tag, j) in e.tags" :key="j" class="jsdoc-tag">@{{ tag.name }}<span v-if="tag.text"> {{ tag.text }}</span></span>
            </div>
          </div>
        </div>
      </details>

      <!-- Slots -->
      <details v-if="meta.slots.length > 0" class="analysis-section" open>
        <summary class="section-title">Slots ({{ meta.slots.length }})</summary>
        <div class="import-list">
          <div v-for="(s, i) in meta.slots" :key="i" class="import-item">
            <div class="binding-item">
              <code class="binding-name">#{{ s.name }}</code>
              <span v-if="s.isScoped" class="badge badge-reactive">scoped</span>
              <span v-if="s.isRequired" class="badge badge-vue">required</span>
              <span v-if="s.hasFallbackContent" class="badge badge-kind">fallback</span>
            </div>
            <div v-if="s.description" class="jsdoc-description">{{ s.description }}</div>
            <div v-if="s.tags?.length" class="jsdoc-tags">
              <span v-for="(tag, j) in s.tags" :key="j" class="jsdoc-tag">@{{ tag.name }}<span v-if="tag.text"> {{ tag.text }}</span></span>
            </div>
            <div v-if="s.bindings.length > 0" class="import-bindings">
              <span v-for="(b, j) in s.bindings" :key="j" class="binding-tag">
                <code>{{ b.name }}</code>
                <span class="badge badge-type">{{ formatTypeDescriptor(b.type) }}</span>
              </span>
            </div>
          </div>
        </div>
      </details>

      <!-- Models -->
      <details v-if="meta.models.length > 0" class="analysis-section" open>
        <summary class="section-title">Models ({{ meta.models.length }})</summary>
        <div class="import-list">
          <div v-for="(m, i) in meta.models" :key="i" class="binding-item">
            <code class="binding-name">{{ m.name }}</code>
            <span class="badge badge-type">{{ formatTypeDescriptor(m.type) }}</span>
          </div>
        </div>
      </details>

      <!-- Exposed -->
      <details v-if="meta.exposed.length > 0" class="analysis-section" open>
        <summary class="section-title">Exposed ({{ meta.exposed.length }})</summary>
        <div class="import-list">
          <div v-for="(e, i) in meta.exposed" :key="i" class="binding-item">
            <code class="binding-name">{{ e.name }}</code>
            <span class="badge badge-type">{{ formatTypeDescriptor(e.type) }}</span>
          </div>
        </div>
      </details>

      <!-- Components -->
      <details v-if="meta.components.length > 0" class="analysis-section">
        <summary class="section-title">Components ({{ meta.components.length }})</summary>
        <div class="import-list">
          <div v-for="(comp, i) in meta.components" :key="i" class="import-item">
            <div class="binding-item">
              <code class="binding-name">{{ comp.name }}</code>
              <span v-if="comp.isDynamic" class="badge badge-reactive">dynamic</span>
              <span v-if="comp.importSource" class="binding-init">from {{ comp.importSource }}</span>
            </div>
            <div v-if="comp.props.length > 0" class="import-bindings">
              <span v-for="(p, j) in comp.props" :key="j" class="binding-tag">
                <code>{{ p.isBound ? ':' : '' }}{{ p.name }}</code>
                <span :class="['badge', p.constness === 'const' ? 'badge-vue' : p.constness === 'dynamic' ? 'badge-reactive' : 'badge-kind']">
                  {{ p.constness }}
                </span>
              </span>
            </div>
            <div v-if="comp.slotsUsed.length > 0" class="import-bindings">
              <span class="sub-label">slots:</span>
              <code v-for="(s, j) in comp.slotsUsed" :key="j" class="style-tag">{{ s }}</code>
            </div>
            <div v-if="comp.vModels.length > 0" class="import-bindings">
              <span class="sub-label">v-model:</span>
              <code v-for="(m, j) in comp.vModels" :key="j" class="style-tag">{{ m }}</code>
            </div>
          </div>
        </div>
      </details>

      <!-- Template Refs -->
      <details v-if="meta.templateRefs.length > 0" class="analysis-section">
        <summary class="section-title">Template Refs ({{ meta.templateRefs.length }})</summary>
        <div class="import-list">
          <div v-for="(ref, i) in meta.templateRefs" :key="i" class="binding-item">
            <code class="binding-name">{{ ref.name }}</code>
            <span class="badge badge-kind">{{ ref.targetTag }}</span>
            <span v-if="ref.isDynamic" class="badge badge-reactive">dynamic</span>
          </div>
        </div>
      </details>

      <!-- Vue API Calls -->
      <details v-if="meta.vueApiCalls.length > 0" class="analysis-section">
        <summary class="section-title">Vue API Calls ({{ meta.vueApiCalls.length }})</summary>
        <div class="import-list">
          <div v-for="(call, i) in meta.vueApiCalls" :key="i" class="binding-item">
            <code class="binding-name">{{ call.api }}</code>
            <span v-if="call.argValue" class="badge badge-kind">"{{ call.argValue }}"</span>
          </div>
        </div>
      </details>

      <!-- Styles -->
      <details v-if="meta.styles.length > 0" class="analysis-section">
        <summary class="section-title">Styles ({{ meta.styles.length }})</summary>
        <div v-for="(style, i) in meta.styles" :key="i" class="style-block">
          <div class="style-header">
            <span class="badge badge-kind">{{ style.lang }}</span>
            <span v-if="style.scoped" class="badge badge-reactive">scoped</span>
            <span v-if="style.isModule" class="badge badge-vue">module</span>
            <span v-if="style.moduleName" class="badge badge-kind">{{ style.moduleName }}</span>
          </div>
          <div v-if="style.classes.length > 0" class="style-sub">
            <span class="sub-label">Classes:</span>
            <code v-for="(c, j) in style.classes" :key="j" class="style-tag">.{{ c }}</code>
          </div>
          <div v-if="style.ids.length > 0" class="style-sub">
            <span class="sub-label">IDs:</span>
            <code v-for="(id, j) in style.ids" :key="j" class="style-tag">#{{ id }}</code>
          </div>
          <div v-if="style.customProperties.length > 0" class="style-sub">
            <span class="sub-label">Custom Props:</span>
            <code v-for="(cp, j) in style.customProperties" :key="j" class="style-tag">{{ cp }}</code>
          </div>
          <div v-if="style.selectors.length > 0" class="style-sub">
            <span class="sub-label">Selectors:</span>
            <span>{{ style.selectors.length }}</span>
          </div>
        </div>
      </details>
    </div>
  </div>
</template>

<style scoped>
.analysis-panel {
  height: 100%;
  overflow-y: auto;
  background: var(--bg-primary);
  color: var(--text-primary);
  font-size: 13px;
  font-family: ui-monospace, "Cascadia Code", "Source Code Pro", Menlo, Consolas, monospace;
}

.empty-state {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 100%;
  color: var(--text-secondary);
  font-style: italic;
}

.analysis-content {
  padding: 12px;
}

.analysis-section {
  margin-bottom: 8px;
  border: 1px solid var(--border-color);
  border-radius: 4px;
}

.analysis-section[open] {
  padding-bottom: 8px;
}

.section-title {
  padding: 6px 10px;
  font-weight: 600;
  font-size: 12px;
  text-transform: uppercase;
  letter-spacing: 0.5px;
  color: var(--text-secondary);
  cursor: pointer;
  user-select: none;
}

.section-title:hover {
  color: var(--text-primary);
}

.flag-chips {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
  padding: 4px 10px;
}

.flag-chip {
  padding: 2px 8px;
  font-size: 11px;
  background: var(--bg-tertiary);
  border-radius: 10px;
  color: var(--text-secondary);
}

.import-list {
  padding: 4px 10px;
}

.import-item {
  padding: 3px 0;
}

.binding-item {
  padding: 3px 0;
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 4px;
}

.import-bindings {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
  margin-left: 12px;
}

.binding-tag {
  display: inline-flex;
  align-items: center;
  gap: 2px;
}

.badge {
  padding: 1px 5px;
  font-size: 10px;
  border-radius: 3px;
  font-weight: 500;
}

.badge-type {
  background: rgba(168, 85, 247, 0.15);
  color: #a855f7;
}

.badge-vue {
  background: rgba(66, 184, 131, 0.15);
  color: #42b883;
}

.badge-kind {
  background: var(--bg-tertiary);
  color: var(--text-secondary);
}

.badge-reactive {
  background: rgba(66, 153, 225, 0.15);
  color: var(--accent-color, #4299e1);
}

.binding-name {
  font-weight: 600;
}

.binding-init {
  color: var(--text-secondary);
  font-size: 12px;
}

.sub-label {
  font-size: 11px;
  color: var(--text-secondary);
  font-weight: 500;
}

.style-block {
  padding: 6px 10px;
  border-bottom: 1px solid var(--border-color);
}

.style-block:last-child {
  border-bottom: none;
}

.style-header {
  display: flex;
  gap: 4px;
  margin-bottom: 4px;
}

.style-sub {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 4px;
  padding: 2px 0;
}

.style-tag {
  font-size: 12px;
  padding: 1px 4px;
  background: var(--bg-tertiary);
  border-radius: 2px;
}

.jsdoc-description {
  padding: 2px 0 2px 12px;
  font-size: 12px;
  color: var(--text-secondary);
  font-style: italic;
}

.jsdoc-tags {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
  padding: 2px 0 2px 12px;
}

.jsdoc-tag {
  font-size: 11px;
  padding: 1px 5px;
  background: rgba(234, 179, 8, 0.12);
  color: #eab308;
  border-radius: 3px;
  font-weight: 500;
}

code {
  font-family: inherit;
}
</style>
