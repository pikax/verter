<script>
	let { value } = $props();
</script>

<slot>
	<div class="{value}">x</div>
</slot>

<style>
	.b {
		color: red;
	}
</style>
