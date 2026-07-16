<script lang="ts">
  const load = Promise.resolve({ label: "ready" });
</script>

{#await load}
  <p>loading</p>
{:then result}
  <p>{result.label}</p>
{:catch err}
  <p>{String(err)}</p>
{/await}
