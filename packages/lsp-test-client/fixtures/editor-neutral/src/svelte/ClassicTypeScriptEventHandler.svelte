<script lang="ts">
  // @ts-expect-error legacy-mode named handlers intentionally retain implicit-any parameters
  function myClick(e) {
    return e.pointerId;
  }
</script>

<button on:pointerdown={myClick}>Click</button>
