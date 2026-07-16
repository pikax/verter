<script lang="ts">
  import StrictChild from "./StrictChild.svelte";
</script>

<!-- totallyFakeProp is not in $props → must error (Svelte typed strict) -->
<StrictChild label="ok" totallyFakeProp="nope" />
