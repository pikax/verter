<script lang="ts">
  // Intentionally incomplete identifier — completion/auto-import of a local or ambient
  // is exercised by E2E (Svelte does not use vue's `computed`; use a missing local).
  let base = $state(1);
  // AUTO_IMPORT_SYMBOL_SITE: type prefix of a known global/helper if offered
  let doubled = bas;
</script>

<div>{doubled}</div>
