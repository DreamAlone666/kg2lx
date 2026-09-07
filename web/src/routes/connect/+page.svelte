<script lang="ts">
  import { adminSession } from '$lib/state/admin-session.svelte';
  import { goto } from '$app/navigation';
  import { AdminApi } from '$lib/api/admin';
  import { AdminApiError } from '$lib/api/errors';
  import { HttpClient } from '$lib/api/http';
  import { Button, Input, Card, CardHeader, CardContent } from '$lib/components/ui';

  let baseUrl = $state(adminSession.serverBaseUrl || '');
  let token = $state(adminSession.adminToken);
  let loading = $state(false);
  let error = $state<string | null>(null);

  async function handleConnect(e: Event) {
    e.preventDefault();
    loading = true;
    error = null;

    try {
      const serverBaseUrl = baseUrl.trim().replace(/\/$/, '');
      const adminToken = token.trim();
      const api = new AdminApi(new HttpClient({ baseUrl: serverBaseUrl, adminToken }));
      const signal = AbortSignal.timeout(8000);

      await api.healthz(signal);
      await api.listSources(signal);

      adminSession.setSession(serverBaseUrl, adminToken);
      goto('/sources');
    } catch (e: any) {
      error = connectErrorMessage(e);
    } finally {
      loading = false;
    }
  }

  function connectErrorMessage(e: unknown): string {
    if (e instanceof DOMException && (e.name === 'TimeoutError' || e.name === 'AbortError')) {
      return '连接超时，请确认后端服务正在运行。';
    }
    if (e instanceof AdminApiError && e.status === 401) {
      return '管理员 Token 不正确（401）。';
    }
    if (e instanceof TypeError) {
      // fetch 网络层失败统一抛 TypeError, 原始信息 (如 "Failed to fetch") 对用户无意义
      return '无法连接到服务器：请确认地址以 http(s):// 开头、后端已启动，且浏览器可以访问（跨域 / 网络问题也会导致此错误）。';
    }
    return (e as Error)?.message || '连接失败';
  }
</script>

<svelte:head>
  <title>连接后端 | kg2lx</title>
</svelte:head>

<div class="flex min-h-screen flex-col items-center justify-center p-4 bg-slate-50">
  <Card class="w-full max-w-md">
    <CardHeader>
      <h1 class="text-2xl font-bold">连接至后端</h1>
      <p class="text-sm text-muted-foreground">输入后端服务器地址和管理员 Token。</p>
    </CardHeader>

    <CardContent>
      <form onsubmit={handleConnect} class="space-y-4">
        <div class="space-y-2">
          <label for="baseUrl" class="text-sm font-medium">服务器地址</label>
          <Input
            id="baseUrl"
            type="text"
            bind:value={baseUrl}
            placeholder="留空使用当前域名 (Same Origin)"
          />
          <p class="text-xs text-muted-foreground">
            留空将使用当前域名（Same Origin）。开发环境可填 <code>/backend</code> 快速连接本地服务。
          </p>
        </div>

        <div class="space-y-2">
          <label for="token" class="text-sm font-medium">管理员 Token</label>
          <Input
            id="token"
            type="password"
            bind:value={token}
            placeholder="你的管理员密钥"
            required
          />
        </div>

        {#if error}
          <div class="rounded-md bg-destructive/15 p-3 text-sm text-destructive">
            {error}
          </div>
        {/if}

        <Button type="submit" class="w-full" disabled={loading}>
          {loading ? '连接中...' : '连接'}
        </Button>
      </form>
    </CardContent>
  </Card>
</div>
