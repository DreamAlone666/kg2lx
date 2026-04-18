<script lang="ts">
  import { onMount } from 'svelte';
  import { page } from '$app/state';
  import { adminSession } from '$lib/state/admin-session.svelte';
  import { sourcesController } from '$lib/features/sources/controller.svelte';
  import { goto } from '$app/navigation';
  import { Button, Badge, Card, CardHeader, CardContent } from '$lib/components/ui';
  import LogsPanel from '$lib/components/source-logs/LogsPanel.svelte';

  let sourceId = $derived(page.params.sourceId ?? '');

  onMount(() => {
    if (!adminSession.adminToken) {
      goto('/connect');
    }
  });

  $effect(() => {
    const sourceId = page.params.sourceId;
    if (!adminSession.adminToken || !sourceId) return;
    sourcesController.loadSource(sourceId);
  });

  async function handleRefresh() {
    if (!sourceId) return;
    await sourcesController.refreshSource(sourceId);
  }

  function copyToClipboard(text: string) {
    navigator.clipboard.writeText(text);
    // In a real app we'd use toast
    alert('已复制到剪贴板！');
  }

  const accountStatusMap: Record<string, string> = {
    active: '正常',
    pending: '待处理',
    expired: '已过期',
    disabled: '已禁用',
    login_failed: '登录失败'
  };
  const providerMap: Record<string, string> = {
    kugou_lite: '酷狗概念版'
  };
</script>

<div class="mx-auto max-w-3xl p-6">
  <div class="mb-6 flex items-center gap-4">
    <Button variant="ghost" size="sm" href="/sources">
      &larr; 返回音源列表
    </Button>
  </div>

  {#if sourcesController.loading && !sourcesController.currentSource}
    <div class="flex h-64 items-center justify-center">
      <div class="animate-spin rounded-full h-8 w-8 border-b-2 border-primary"></div>
    </div>
  {:else if sourcesController.error}
    <div class="rounded-lg border border-destructive/50 bg-destructive/10 p-4 text-destructive">
      <p class="font-semibold">加载音源出错</p>
      <p class="text-sm">{sourcesController.error}</p>
    </div>
  {:else if sourcesController.currentSource}
    {@const source = sourcesController.currentSource}
    <Card>
      <CardHeader class="border-b">
        <div class="flex items-center justify-between">
          <div>
            <h1 class="text-2xl font-bold">{source.account.userid}</h1>
            <p class="text-sm text-muted-foreground">平台: {providerMap[source.provider] || source.provider}</p>
          </div>
          <Button
            variant="outline"
            onclick={handleRefresh}
            disabled={sourcesController.refreshing[sourceId]}
          >
            {sourcesController.refreshing[sourceId] ? '刷新中...' : '刷新 Token'}
          </Button>
        </div>
      </CardHeader>

      <CardContent class="p-6 space-y-8">
        <div class="grid grid-cols-2 gap-8 sm:grid-cols-4">
          <div class="space-y-1">
            <p class="text-xs font-medium text-muted-foreground uppercase">状态</p>
            <Badge variant={source.account.status === 'active' ? 'default' : 'destructive'}>
              {accountStatusMap[source.account.status] || source.account.status}
            </Badge>
          </div>
          <div class="space-y-1">
            <p class="text-xs font-medium text-muted-foreground uppercase">VIP 状态</p>
            <p class="text-sm font-semibold">{source.account.vip_active ? '已激活' : '未激活'}</p>
          </div>
          <div class="space-y-1 col-span-2">
            <p class="text-xs font-medium text-muted-foreground uppercase">最后刷新时间</p>
            <p class="text-sm">
              {source.account.last_refresh_at ? new Date(source.account.last_refresh_at * 1000).toLocaleString() : '从未'}
            </p>
          </div>
        </div>

        <div class="space-y-3">
          <h3 class="text-sm font-medium">脚本地址</h3>
          <p class="text-xs text-muted-foreground">将此 URL 导入洛雪音乐（LX Music）作为自定义音源。</p>
          <div class="flex items-center gap-2">
            <div class="flex-1 rounded-md border bg-muted px-3 py-2 font-mono text-xs truncate">
              {source.script_url}
            </div>
            <Button size="sm" onclick={() => copyToClipboard(source.script_url)}>
              复制
            </Button>
          </div>
        </div>

        {#if source.account.last_error}
          <div class="rounded-md border border-destructive/20 bg-destructive/5 p-4">
            <h3 class="text-xs font-bold uppercase tracking-wider text-destructive">最后一次错误</h3>
            <p class="mt-1 text-sm text-destructive">{source.account.last_error}</p>
          </div>
        {/if}
        
        <div class="space-y-1">
          <p class="text-xs font-medium text-muted-foreground uppercase">运行时 Token 预览</p>
          <p class="text-xs font-mono break-all text-muted-foreground/60">{source.runtime_token_preview}</p>
        </div>
      </CardContent>
    </Card>

    <LogsPanel {sourceId} />
  {/if}
</div>
