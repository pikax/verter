<script>
	let color = $state('red');
	let x = $state('1');
</script>

<button onclick={() => color = 'blue'} style="font-weight:bold" style:color={color}></button>
<button onclick={() => x += '1'} style="display:block" style:--x={x} style:color|important={color}></button>
