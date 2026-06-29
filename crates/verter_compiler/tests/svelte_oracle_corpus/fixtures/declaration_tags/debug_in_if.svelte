<script>
	let a = $state(0);
</script>

{#if a}
	<button onclick={() => a++}>x</button>
	{@debug a}
{/if}
