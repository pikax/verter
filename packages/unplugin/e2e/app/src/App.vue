<script setup lang="ts">
// Reactivity
import RefCounter from './components/reactivity/RefCounter.vue'
import ReactiveObject from './components/reactivity/ReactiveObject.vue'

// TypeScript
import TypedPropsEmits from './components/typescript/TypedPropsEmits.vue'
import WithDefaultsDemo from './components/typescript/WithDefaultsDemo.vue'
import DefineModelParent from './components/typescript/DefineModelParent.vue'
import ExposeParent from './components/typescript/ExposeParent.vue'
import GenericComponent from './components/typescript/GenericComponent.vue'

// Options API
import OptionsCounter from './components/options-api/OptionsCounter.vue'
import OptionsComponent from './components/options-api/OptionsComponent.vue'

// Directives
import ConditionalRendering from './components/directives/ConditionalRendering.vue'
import ListRendering from './components/directives/ListRendering.vue'
import FormInputs from './components/directives/FormInputs.vue'
import EventModifiers from './components/directives/EventModifiers.vue'
import DynamicBindings from './components/directives/DynamicBindings.vue'

// Slots
import SlotShowcase from './components/slots/SlotShowcase.vue'

// Styles
import ScopedCss from './components/styles/ScopedCss.vue'
import CssModules from './components/styles/CssModules.vue'
import ScssDemo from './components/styles/ScssDemo.vue'
import LessDemo from './components/styles/LessDemo.vue'
import CssVBind from './components/styles/CssVBind.vue'

// Composables
import ComposableDemo from './components/composables/ComposableDemo.vue'
import ProvideInject from './components/composables/ProvideInject.vue'

// Edge Cases
import FragmentRoots from './components/edge-cases/FragmentRoots.vue'
import MultiInstance from './components/edge-cases/MultiInstance.vue'
import ArrayMutations from './components/edge-cases/ArrayMutations.vue'
import AsyncUpdate from './components/edge-cases/AsyncUpdate.vue'
import TemplateRefs from './components/edge-cases/TemplateRefs.vue'
import DynamicComponent from './components/edge-cases/DynamicComponent.vue'
import DeepNested from './components/edge-cases/DeepNested.vue'
import ExportDefaultComment from './components/edge-cases/ExportDefaultComment.vue'
import RenderInComment from './components/edge-cases/RenderInComment.vue'
import MixedTextChildren from './components/edge-cases/MixedTextChildren.vue'

// Vapor
import VaporCounter from './components/vapor/VaporCounter.vue'
// TODO: Re-enable once Rust vapor codegen for v-if/v-for is fixed
// import VaporConditional from './components/vapor/VaporConditional.vue'
// import VaporList from './components/vapor/VaporList.vue'
import VaporBindings from './components/vapor/VaporBindings.vue'

// Integration
import TodoApp from './components/integration/TodoApp.vue'

import { ref } from 'vue'

const optionsReceived = ref('')
const genericItems = [
  { id: 1, label: 'Alpha' },
  { id: 2, label: 'Beta' },
  { id: 3, label: 'Gamma' },
]
</script>

<template>
  <div id="e2e-app">
    <h1 data-testid="app-title">Verter E2E Test App</h1>

    <section data-testid="section-reactivity">
      <h2>Reactivity</h2>
      <RefCounter />
      <ReactiveObject />
    </section>

    <section data-testid="section-typescript">
      <h2>TypeScript</h2>
      <TypedPropsEmits />
      <WithDefaultsDemo />
      <DefineModelParent />
      <ExposeParent />
      <GenericComponent :items="genericItems" />
    </section>

    <section data-testid="section-options-api">
      <h2>Options API</h2>
      <OptionsCounter />
      <OptionsComponent label="test-label" @action="(v: string) => optionsReceived = v" />
      <span data-testid="options-received">{{ optionsReceived }}</span>
    </section>

    <section data-testid="section-directives">
      <h2>Directives</h2>
      <ConditionalRendering />
      <ListRendering />
      <FormInputs />
      <EventModifiers />
      <DynamicBindings />
    </section>

    <section data-testid="section-slots">
      <h2>Slots</h2>
      <SlotShowcase />
    </section>

    <section data-testid="section-styles">
      <h2>Styles</h2>
      <ScopedCss />
      <CssModules />
      <ScssDemo />
      <LessDemo />
      <CssVBind />
    </section>

    <section data-testid="section-composables">
      <h2>Composables</h2>
      <ComposableDemo />
      <ProvideInject />
    </section>

    <section data-testid="section-edge-cases">
      <h2>Edge Cases</h2>
      <FragmentRoots />
      <div data-testid="multi-instance-wrapper">
        <MultiInstance />
        <MultiInstance />
        <MultiInstance />
      </div>
      <ArrayMutations />
      <AsyncUpdate />
      <TemplateRefs />
      <DynamicComponent />
      <DeepNested />
      <ExportDefaultComment />
      <RenderInComment />
      <MixedTextChildren />
    </section>

    <section data-testid="section-vapor">
      <h2>Vapor</h2>
      <VaporCounter />
      <!-- TODO: Re-enable once Rust vapor codegen for v-if/v-for is fixed -->
      <!-- <VaporConditional /> -->
      <!-- <VaporList /> -->
      <VaporBindings />
    </section>

    <section data-testid="section-integration">
      <h2>Integration</h2>
      <TodoApp />
    </section>
  </div>
</template>
