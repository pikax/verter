<script lang="ts">
  let lastPointerId = $state(0);

  function myClick(e) {
    // @ts-expect-error inferred PointerEvent has no such member; any would leave this unused
    lastPointerId = e.__verterMissingPointerMember;
  }
</script>

<button onpointerdown={myClick}>Click</button>
<output>{lastPointerId}</output>
