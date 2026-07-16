<script lang="ts">
  import SnippetCase from "./SnippetCase.svelte";
</script>

<SnippetCase>
  {#snippet row({ name })}
    <span>{name}</span>
  {/snippet}
</SnippetCase>
