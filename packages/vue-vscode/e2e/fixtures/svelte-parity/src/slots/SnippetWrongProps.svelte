<script lang="ts">
  import SnippetTypedHost from "./SnippetTypedHost.svelte";
</script>

<!-- Live type errors on snippet parameter usage -->
<SnippetTypedHost>
  {#snippet header({ title, count })}
    <span>{title.toFixed(2)}</span>
    <span>{count.toUpperCase()}</span>
  {/snippet}
  {#snippet row({ body, flag })}
    <span>{body.toFixed(1)}</span>
    <span>{flag.toUpperCase()}</span>
  {/snippet}
</SnippetTypedHost>
