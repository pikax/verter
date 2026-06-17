<script>
	let items = $state([{ id: 1, label: 'a' }, { id: 2, label: 'b' }]);
	let show = $state(true);
	let selected = $state(0);
</script>

{#if show}
	<ul>
		{#each items as item (item.id)}
			<li>{item.label}</li>
		{/each}
	</ul>
{:else}
	<p>nothing</p>
{/if}

{#key selected}
	<span>{selected}</span>
{/key}
