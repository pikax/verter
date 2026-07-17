<script lang="ts">
  let lastPointerId = $state(0);

  function myClick(e) {
    lastPointerId = e.pointerId;
  }
</script>

<button onpointerdown={myClick}>Click</button>
<output>{lastPointerId}</output>
