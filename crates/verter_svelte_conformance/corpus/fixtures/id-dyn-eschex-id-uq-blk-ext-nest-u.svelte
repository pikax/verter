<script>
	let open = $state(true);
	let { value } = $props();
</script>

{#if open}
	<div id={value}>x</div>
{/if}

<style>
	#a\26 b {
		color: red;

		&:hover {
			color: blue;
		}
	}
</style>
