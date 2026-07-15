<template>
  <section>{{ heading }} · {{ echo }} · {{ ownProps.heading }}</section>
</template>

<script setup lang="ts">
import { makeConfig, type PanelConfig } from "./helpers";

const props = defineProps<{ heading: string }>();

// Go-to-definition on the imported factory must land on its `helpers.ts`
// declaration line, never on line 0.
const factory = makeConfig; // @dx-anchor def.importedUse

const config: PanelConfig = factory(props.heading);

const heading = config.title;

// Go-to-definition on the local binding must reach its own declaration above.
const echo = heading; // @dx-anchor def.localUse

// Go-to-definition on the props binding must reach the `defineProps` macro site.
const ownProps = props; // @dx-anchor def.propsRef
</script>
