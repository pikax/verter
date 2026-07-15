<script>
	let selected = $state(0);
	let count = $state(5);
</script>

<button onclick={() => count++}>inc</button>
{#key selected}
	<p>{count}</p>
{/key}
