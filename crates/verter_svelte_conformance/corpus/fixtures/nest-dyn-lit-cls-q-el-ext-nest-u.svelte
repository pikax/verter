<script>
	let { value } = $props();
</script>

<div class="{value}">x</div>

<style>
	.b {
		& {
			color: red;
		}
	}
</style>
