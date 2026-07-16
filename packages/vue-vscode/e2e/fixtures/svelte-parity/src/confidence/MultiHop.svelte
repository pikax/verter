<script lang="ts">
  import IdeSurfaceChild from "../ide/IdeSurfaceChild.svelte";
  import SnippetTypedHost from "../slots/SnippetTypedHost.svelte";
  import StrictChild from "../strict/StrictChild.svelte";

  let label = "multi";
  let count = 1;
  function onPick(v: string) {
    void v;
  }
</script>

<StrictChild {label} />
<IdeSurfaceChild {label} {count} {onPick}>
  {#snippet header({ title })}
    <span>{title}</span>
  {/snippet}
</IdeSurfaceChild>
<SnippetTypedHost>
  {#snippet header({ title, count: c })}
    <span>{title}{c}</span>
  {/snippet}
  {#snippet row({ body })}
    <span>{body}</span>
  {/snippet}
</SnippetTypedHost>
