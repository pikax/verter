<script lang="ts">
  // @ts-expect-error legacy TypeScript handlers remain uncontextual
  function handlePointer(domEvent) {
    return domEvent.notADomEventMember;
  }
</script>

<button onclick={handlePointer}>legacy TypeScript boundary</button>
