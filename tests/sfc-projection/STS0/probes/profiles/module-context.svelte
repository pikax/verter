<script module lang="ts">
  export const moduleAnswer: number = 42;
</script>

<script lang="ts">
  import { moduleAnswer } from "./module-context.svelte";
  const shown: number = moduleAnswer;
</script>

<p>{shown}</p>
