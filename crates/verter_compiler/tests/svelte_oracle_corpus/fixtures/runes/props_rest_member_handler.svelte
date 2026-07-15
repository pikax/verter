<script>
	let { a, ...rest } = $props();
	let sink = $state(0);
</script>

<button onclick={() => sink += rest.x}>x</button>
