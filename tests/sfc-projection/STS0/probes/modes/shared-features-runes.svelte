<!--
  STS0 mode-paired compiler control: every mode-shared feature (store
  auto-subscription, transitions/actions/animate, snippets, event
  attributes, class:/style: directives) in a runes:true component.
  The pinned svelte@5.56.10 compiler must accept this with zero warnings.
-->
<script>
  import { writable } from "svelte/store";

  const store = writable(1);
  let value = $state(1);

  function act(node) {
    return { destroy() {} };
  }

  function fade(node, params) {
    return { duration: params?.duration ?? 1 };
  }

  function zoom(node, positions) {
    return { duration: Math.abs(positions.to.left - positions.from.left) || 1 };
  }
</script>

{#snippet label(text)}
  <span>{text}</span>
{/snippet}

{#each [value] as item (item)}
  <div
    class:on={value === 1}
    style:color="red"
    use:act={value}
    in:fade={{ duration: 1 }}
    out:fade
    animate:zoom
  >
    {@render label(item)}
  </div>
{/each}

<button onclick={() => (value += 1)}>{$store}</button>
