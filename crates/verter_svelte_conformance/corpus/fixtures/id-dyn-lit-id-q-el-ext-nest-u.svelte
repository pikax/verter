<script>
	let { value } = $props();
</script>

<div id="{value}">x</div>

<style>
	#b {
		color: red;

		&:hover {
			color: blue;
		}
	}
</style>
