<script>export let obj;</script>
{#await obj.m() then v}<p>{v}</p>{/await}
{#key obj.k()}<p>a</p>{/key}
