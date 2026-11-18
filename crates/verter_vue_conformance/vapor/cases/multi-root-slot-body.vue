<template>
  <div>
    <Panel><p>1</p><span>{{ count }}</span></Panel>
    <Panel><template #head>t<b :title="title" /></template><i /> <em>{{ note }}</em></Panel>
  </div>
</template>
