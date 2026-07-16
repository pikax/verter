<script lang="ts">
  interface Person {
    name: string;
    age: number;
  }

  let person: Person | null = null;
  let flag = true;
  let items: Array<string | number> = ["a", 1];

  function setPerson() {
    person = { name: "Ada", age: 36 };
  }
</script>

<div>
  <button onclick={setPerson}>set</button>
  {#if person}
    <p>{person.name} — {person.age}</p>
  {:else}
    <p>no person</p>
  {/if}
  {#if flag}
    <p>flag-on</p>
  {/if}
  {#each items as item (String(item))}
    <span>{item}</span>
  {/each}
</div>
