<script>
	import Child from './Child.svelte';
</script>

<Child>
	<div class=a&amp;b&#38;c>x</div>
</Child>

<style>
	[data-x="\7a \z"] {
		color: red;
	}
</style>
