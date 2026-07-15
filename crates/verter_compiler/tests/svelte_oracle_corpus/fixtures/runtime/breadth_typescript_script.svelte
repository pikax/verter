<script lang="ts">
    interface Counter { value: number }
    let count: number = $state(0);
    const STEP: number = 2;
    function advance(delta: number = STEP): void { count += delta; }
    class Marker { value: number = 3; }
    if (new Marker().value < 0) advance(0);
</script>
<button onclick={() => count += STEP}>{count}</button>
