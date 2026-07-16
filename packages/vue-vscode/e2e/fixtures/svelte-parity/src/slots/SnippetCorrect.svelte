<script lang="ts">
  import SnippetTypedHost from "./SnippetTypedHost.svelte";
</script>

<!-- Correct snippet usage: must stay diagnostic-clean -->
<SnippetTypedHost>
  {#snippet header({ title, count })}
    <h3>{title}:{count.toFixed(0)}</h3>
  {/snippet}
  {#snippet row({ body, flag })}
    <p>{body}:{flag}</p>
  {/snippet}
  {#snippet footer({ ok })}
    <span>{ok}</span>
  {/snippet}
</SnippetTypedHost>
