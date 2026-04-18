<script lang="ts">
  import { adminSession } from '$lib/state/admin-session.svelte';
  import { goto } from '$app/navigation';
  import { AdminApi } from '$lib/api/admin';
  import { HttpClient } from '$lib/api/http';
  import { dev } from '$app/environment';
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

      await api.healthz();
      await api.listSources();

      adminSession.setSession(serverBaseUrl, adminToken);
      goto('/sources');
    } catch (e: any) {
      error = e.message || '连接失败';
    } finally {
      loading = false;
    }
  }
</script>

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
