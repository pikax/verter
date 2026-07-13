<script>
	import Child from './Child.svelte';
</script>

<Child>
	<div id=&#98;&#x62;>x</div>
</Child>

<style>
	:global(#bb) {
		color: red;
	}
</style>
