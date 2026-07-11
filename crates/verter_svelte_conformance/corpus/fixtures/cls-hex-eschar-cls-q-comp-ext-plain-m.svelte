<script>
	import Child from './Child.svelte';
</script>

<Child>
	<div class="a a&#x26;b">x</div>
</Child>

<style>
	.a\&b {
		color: red;
	}
</style>
