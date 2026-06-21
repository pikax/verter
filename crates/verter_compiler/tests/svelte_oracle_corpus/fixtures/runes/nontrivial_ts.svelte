<script lang="ts">
	interface Box { n: number }
	function id<T>(x: T): T { return x; }
	let count: number = $state(0);
	let labeled: Box = { n: count as number };
</script>
<button onclick={() => count++}>{count}</button>
