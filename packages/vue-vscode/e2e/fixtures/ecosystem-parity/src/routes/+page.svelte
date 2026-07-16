<script lang="ts">
  import { libHello } from "$lib/helper";

  // SvelteKit-shaped route page: $lib alias only (typed stub, no Official Kit LS).
  const msg = libHello("kit-page");
</script>

<main>
  <h1>{msg}</h1>
</main>
