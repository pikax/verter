<script>
	let c = $state('x');
</script>

<div style="a{String(c)}b" onclick={() => c += '!'}></div>
