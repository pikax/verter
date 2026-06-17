<script>
	let items = $state([{ qty: 2, price: 3 }, { qty: 1, price: 5 }]);
</script>

<ul>
	{#each items as item}
		{@const total = item.qty * item.price}
		<li>{total}</li>
	{/each}
</ul>
