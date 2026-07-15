<script>
	let open = $state(true);
	let { value } = $props();
</script>

{#if open}
	<div class="{value}">x</div>
{/if}

<style>
	.a\26 b\&c {
		color: red;
	}
</style>
