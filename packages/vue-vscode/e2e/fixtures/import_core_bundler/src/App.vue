<script setup lang="ts">
import { ref } from "vue";
// Direct relative default import.
import DirectComp from "./DirectComp.vue";
// Deep relative default import.
import DeepComp from "./nested/deep/DeepComp.vue";
// baseUrl-relative default import (resolved via baseUrl ".").
import BaseUrlComp from "src/widgets/BaseUrlComp.vue";
// `@/` alias default import.
import AliasAtComp from "@/AliasAtComp.vue";
// `~/` alias default import (configured in tsconfig paths).
import AliasTildeComp from "~/AliasTildeComp.vue";
// custom `#util/` alias default import.
import CustomAliasComp from "#util/CustomAliasComp.vue";
// Named import from the top barrel (`export { default as NamedFromBarrel }`).
import { NamedFromBarrel } from "./index";
// Named import reached through a barrel-of-barrels (`export *` -> widgets).
import { WidgetFromStar } from "./index";
// Namespace import of the widgets barrel — used as `<widgets.WidgetFromStar>`.
import * as widgets from "./widgets";
// Type-only import: must NOT register a template component value binding.
import type { OnlyAType } from "./types";

const count = ref(0);
const typed: OnlyAType = { typeFieldOnly: 1 };
</script>
<template>
  <div>{{ count }} {{ typed.typeFieldOnly }}</div>
  <DirectComp directOnly="a" />
  <DeepComp deepOnly="b" />
  <BaseUrlComp :baseUrlOnly="1" />
  <AliasAtComp aliasAtOnly="c" />
  <AliasTildeComp :aliasTildeOnly="true" />
  <CustomAliasComp customAliasOnly="d" />
  <NamedFromBarrel namedBarrelOnly="e" />
  <WidgetFromStar :baseUrlOnly="2" />
  <widgets.WidgetFromStar :baseUrlOnly="3" />
  <!-- Intentional misuse: `OnlyAType` is a `import type`, not a value. Rendering
       it as a tag gives the script-imports carrier-linkage map a name to match,
       so the type-only carrier-linkage fact is forced rather than vacuous (see
       the type-only block + the `type_only_import_is_not_carrier_linked_*`
       tracked-gap companion in import_matrix.rs). -->
  <OnlyAType :typeFieldOnly="1" />
</template>
