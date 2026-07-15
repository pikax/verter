<script>
	let { a, ...rest } = $props();
</script>

<div title={rest?.x}></div>
<div id={rest?.x.y}></div>
