<script>
	let c = $state('x');
	let s = $state('red');
</script>

<button onclick={() => { c += '!'; s = 'blue'; }} class="a&amp;b-${c}-end" style="q:'&quot;'" style:color={s}>go</button>
