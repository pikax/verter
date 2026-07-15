<svelte:options customElement="x-tmpl-host" />

<button onfocus={() => $host().dispatchEvent(new CustomEvent('boop'))}>go</button>
