<script lang="ts">
  import DailyBinding from "./DailyBinding.svelte";
  import PropParent from "./components/PropParent.svelte";
  import RunesSurface from "./runes/RunesSurface.svelte";
  import ControlFlow from "./control/ControlFlow.svelte";
  import BadPropParent from "./diagnostics/BadPropParent.svelte";
  import BindValue from "./features/BindValue.svelte";
  import BindableParent from "./features/BindableParent.svelte";
  import AwaitCase from "./features/AwaitCase.svelte";
  import SnippetParent from "./features/SnippetParent.svelte";
  import EffectCase from "./features/EffectCase.svelte";
  import MappingCase from "./features/MappingCase.svelte";
  import Narrowing from "./features/Narrowing.svelte";
  import ScopedStyle from "./features/ScopedStyle.svelte";
  import JsRename from "./features/JsRename.svelte";
  import Directives from "./matrix/Directives.svelte";
  import EventsProps from "./matrix/EventsProps.svelte";
  import ModuleScript from "./matrix/ModuleScript.svelte";
  import JsSurface from "./matrix/JsSurface.svelte";
  import IntrinsicElements from "./features/IntrinsicElements.svelte";
  import ExposePublic from "./features/ExposePublic.svelte";
  import SnippetCorrect from "./slots/SnippetCorrect.svelte";
  import IdeSurfaceParent from "./ide/IdeSurfaceParent.svelte";
  import GenericInferGood from "./generics/GenericInferGood.svelte";
</script>

<main>
  <DailyBinding />
  <PropParent />
  <RunesSurface />
  <ControlFlow />
  <BadPropParent />
  <BindValue />
  <BindableParent />
  <AwaitCase />
  <SnippetParent />
  <EffectCase />
  <MappingCase />
  <Narrowing />
  <ScopedStyle />
  <JsRename />
  <Directives />
  <EventsProps />
  <ModuleScript />
  <JsSurface />
  <IntrinsicElements />
  <ExposePublic />
  <SnippetCorrect />
  <IdeSurfaceParent />
  <GenericInferGood />
</main>
