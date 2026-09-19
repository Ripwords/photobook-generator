<script setup lang="ts">
const { check } = useUpdater();

// Not awaited, so the check never sits on the first-paint path.
onMounted(() => void check(true));
</script>

<template>
  <UApp>
    <NuxtPage />
  </UApp>
</template>
