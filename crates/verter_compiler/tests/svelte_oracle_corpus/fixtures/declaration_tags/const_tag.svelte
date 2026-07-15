<script>
	let { items } = $props();
</script>

<div>
	{#each items as item}
		{@const total = item.qty * item.price}
		<p>{total}</p>
	{/each}
</div>
