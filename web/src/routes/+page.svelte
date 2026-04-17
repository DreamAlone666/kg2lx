<script lang="ts">
  import { onMount } from 'svelte';
  import { goto } from '$app/navigation';
  import { adminSession } from '$lib/state/admin-session.svelte';

  onMount(() => {
    if (adminSession.serverBaseUrl && adminSession.adminToken) {
      goto('/sources');
    } else {
      goto('/connect');
    }
  });
</script>

<div class="flex h-screen items-center justify-center">
  <p class="text-sm text-gray-500">正在跳转...</p>
</div>
