<script>
	let n = $state(0);
</script>

{#if n === 0}
	<p>zero</p>
{:else if n === 1}
	<p>one</p>
{:else}
	<p>many</p>
{/if}
