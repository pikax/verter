<script lang="ts">
  import EventChild from "./EventChild.svelte";
  let last = $state("");
  function onPick(v: string) {
    last = v;
  }
</script>

<EventChild label="child" onPick={onPick} />
<span>{last}</span>
