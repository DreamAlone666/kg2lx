<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { page } from '$app/state';
  import { adminSession } from '$lib/state/admin-session.svelte';
  import { qrLoginController } from '$lib/features/qr-login/controller.svelte';
  import { goto } from '$app/navigation';
  import { Button, Badge, Card, CardHeader, CardContent } from '$lib/components/ui';

  let redirectTimeout: ReturnType<typeof setTimeout> | null = null;

  onMount(() => {
    if (!adminSession.adminToken) {
      goto('/connect');
      return;
    }

    const sessionId = page.url.searchParams.get('session_id');
    if (sessionId) {
      qrLoginController.resume(sessionId);
    }
  });

  onDestroy(() => {
    qrLoginController.stop();
    if (redirectTimeout) {
      clearTimeout(redirectTimeout);
      redirectTimeout = null;
    }
  });

  async function startNewLogin() {
    await qrLoginController.start();
    if (qrLoginController.sessionId) {
      const url = new URL(window.location.href);
      url.searchParams.set('session_id', qrLoginController.sessionId);
      window.history.replaceState({}, '', url);
    }
  }

  $effect(() => {
    const status = qrLoginController.status;
    const terminal = status === 'bound' || status === 'expired' || status === 'failed';

    if (!terminal) return;

    const url = new URL(window.location.href);
    if (url.searchParams.has('session_id')) {
      url.searchParams.delete('session_id');
      window.history.replaceState({}, '', url);
    }

    if (redirectTimeout) {
      clearTimeout(redirectTimeout);
      redirectTimeout = null;
    }

    if (status === 'bound' && qrLoginController.boundSource) {
      redirectTimeout = setTimeout(() => {
        const sourceId = qrLoginController.boundSource?.source.source_id;
        if (sourceId) goto(`/sources/${sourceId}`);
      }, 3000);

      return () => {
        if (redirectTimeout) {
          clearTimeout(redirectTimeout);
          redirectTimeout = null;
        }
      };
    }
  });

  const statusMap: Record<string, string> = {
    pending: '准备中',
    waiting_scan: '等待扫码',
    waiting_confirm: '等待确认',
    authorized: '已授权',
    bound: '绑定成功',
    expired: '已过期',
    failed: '失败'
  };
</script>

<div class="mx-auto max-w-md p-6">
  <div class="mb-6">
    <Button variant="ghost" size="sm" href="/sources">
      &larr; 返回音源列表
    </Button>
  </div>

  <Card class="text-center">
    <CardHeader>
      <h1 class="text-2xl font-bold">绑定酷狗账号</h1>
      <p class="text-muted-foreground text-sm">请使用酷狗概念版 App 扫码。</p>
    </CardHeader>

    <CardContent class="space-y-6">
      {#if qrLoginController.status === 'pending' && !qrLoginController.sessionId}
        <Button class="w-full" onclick={startNewLogin}>
          获取二维码
        </Button>
      {:else}
        <div class="flex flex-col items-center justify-center p-6 border rounded-xl bg-slate-50">
          {#if qrLoginController.qrBase64}
            <div class="bg-white p-4 rounded-lg shadow-sm">
              <img src={qrLoginController.qrBase64} alt="二维码" class="w-64 h-64" />
            </div>
          {:else if qrLoginController.qrUrl}
            <div class="w-64 h-64 flex items-center justify-center bg-muted rounded-lg text-xs break-all p-4">
              {qrLoginController.qrUrl}
            </div>
          {:else if qrLoginController.status === 'pending'}
             <div class="w-64 h-64 flex items-center justify-center">
               <div class="animate-spin rounded-full h-8 w-8 border-b-2 border-primary"></div>
             </div>
          {/if}
          
          <div class="mt-6">
            <Badge variant="secondary" class="px-3 py-1">
              状态: {statusMap[qrLoginController.status] || qrLoginController.status}
            </Badge>
          </div>
        </div>

        {#if qrLoginController.status === 'bound'}
          <div class="rounded-lg bg-green-50 p-4 text-green-800 border border-green-200">
            <p class="font-bold">绑定成功！</p>
            <p class="text-sm">正在跳转至音源详情...</p>
          </div>
        {/if}

        {#if qrLoginController.message}
          <div class="rounded-lg bg-destructive/10 p-4 text-destructive text-sm border border-destructive/20">
            {qrLoginController.message}
          </div>
        {/if}

        <Button variant="link" size="sm" onclick={startNewLogin} class="text-muted-foreground">
          重置并重试
        </Button>
      {/if}
    </CardContent>
  </Card>
</div>
