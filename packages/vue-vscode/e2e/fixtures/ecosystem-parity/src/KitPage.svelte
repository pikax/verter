<script lang="ts">
  import { libHello } from "$lib/helper";
  const msg = libHello("svelte");
</script>

<main>
  <p>{msg}</p>
</main>
