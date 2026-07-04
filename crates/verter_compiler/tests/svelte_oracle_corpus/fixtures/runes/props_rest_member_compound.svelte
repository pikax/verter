<script>
	let { a, ...rest } = $props();
	let objS = $state({ p: 0 });
</script>

<button onclick={() => objS.p += rest.y}>x</button>
