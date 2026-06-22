<script>
	let c = $state('x');
</script>

<div class="a{String(c)}b" onclick={() => c += '!'}></div>
