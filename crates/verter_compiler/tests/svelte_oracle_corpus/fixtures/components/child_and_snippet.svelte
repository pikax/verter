<script>
	import Child from './Child.svelte';

	let rows = $state([1, 2, 3]);
</script>

{#snippet row(n)}
	<li>row {n}</li>
{/snippet}

<Child label="hi" />
<ul>
	{#each rows as n}
		{@render row(n)}
	{/each}
</ul>
