<script>
	let { value } = $props();
</script>

<slot>
	<div class="wrap">
	<div data-x="{value}">x</div>
</div>
</slot>

<style>
	.wrap [data-x="a\26 b"] {
		color: red;
	}
</style>
