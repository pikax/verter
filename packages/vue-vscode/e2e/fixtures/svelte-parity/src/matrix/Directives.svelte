<script lang="ts">
  let show = $state(true);
  let items = $state(["a", "b"]);
  let model = $state("");
  let klass = $state("active");
  let color = $state("red");
  let disabled = $state(false);
  let raw = $state("<b>x</b>");

  function onClick() {
    show = !show;
  }
</script>

<div>
  {#if show}
    <p class={klass} style:color={color}>on</p>
  {:else}
    <p>off</p>
  {/if}
  {#each items as item, index (item)}
    <li>{item}:{index}</li>
  {/each}
  <input bind:value={model} class={klass} disabled={disabled} />
  <button onclick={onClick}>toggle direct</button>
  <button on:click|preventDefault={onClick}>toggle legacy</button>
  {@html raw}
</div>
