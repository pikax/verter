<script>
	let { a, b } = $props();
	let cond = $state(true);
</script>

{@render (cond ? a : b)()}
