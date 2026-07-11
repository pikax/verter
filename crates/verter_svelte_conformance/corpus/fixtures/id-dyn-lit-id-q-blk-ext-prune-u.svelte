<script>
	let open = $state(true);
	let { value } = $props();
</script>

{#if open}
	<div id="{value}">x</div>
{/if}

<style>
	#b {
		color: red;
	}

	.unused-prune {
		color: blue;
	}
</style>
