<script>
	let base = $state(10);
</script>

{#if base > 0}
	{@const tripled = base * 3}
	<button onclick={() => base++}>{tripled}</button>
{/if}
