<script lang="ts">
  import { page } from '$app/state';
  import { sourcesController } from '$lib/features/sources/controller.svelte';
  import { Button, Badge, Card, CardHeader, CardContent } from '$lib/components/ui';
  import LogsPanel from '$lib/components/source-logs/LogsPanel.svelte';

  let sourceId = $derived(page.params.sourceId ?? '');
  let pageTitle = $derived.by(() => {
    const userid = sourcesController.currentSource?.account.userid;
    return userid ? `${userid} | kg2lx` : '音源详情 | kg2lx';
  });

  // 认证守卫已上提到 (app)/+layout.ts，页面只负责取数
  $effect(() => {
    const id = page.params.sourceId;
    if (!id) return;
    sourcesController.loadSource(id);
  });

  async function handleRefresh() {
    if (!sourceId) return;
    await sourcesController.refreshSource(sourceId);
  }

  type CopyState = 'idle' | 'copied' | 'failed';
  let copyState = $state<CopyState>('idle');
  let copyTimer: ReturnType<typeof setTimeout> | null = null;

  async function copyToClipboard(text: string) {
    let ok = false;

    try {
      // Clipboard API 仅在 HTTPS/localhost 下可用
      await navigator.clipboard.writeText(text);
      ok = true;
    } catch {
      // 纯 HTTP 内网部署时降级到 execCommand
      const textarea = document.createElement('textarea');
      textarea.value = text;
      textarea.setAttribute('readonly', '');
      textarea.style.position = 'fixed';
      textarea.style.opacity = '0';
      document.body.appendChild(textarea);
      textarea.select();
      try {
        ok = document.execCommand('copy');
      } catch {
        ok = false;
      }
      textarea.remove();
    }

    copyState = ok ? 'copied' : 'failed';
    if (copyTimer) clearTimeout(copyTimer);
    copyTimer = setTimeout(() => (copyState = 'idle'), 2000);
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

<svelte:head>
  <title>{pageTitle}</title>
</svelte:head>

<div class="mx-auto max-w-3xl">
  <div class="mb-6 flex items-center gap-4">
    <Button variant="ghost" size="sm" href="/sources">
      &larr; 返回音源列表
    </Button>
  </div>

  {#if sourcesController.loading && !sourcesController.currentSource}
    <div class="flex h-64 items-center justify-center">
      <div class="animate-spin rounded-full h-8 w-8 border-b-2 border-primary"></div>
    </div>
  {:else if sourcesController.error && !sourcesController.currentSource}
    <div class="rounded-lg border border-destructive/50 bg-destructive/10 p-4 text-destructive">
      <p class="font-semibold">加载音源出错</p>
      <p class="text-sm">{sourcesController.error}</p>
    </div>
  {:else if sourcesController.currentSource}
    {@const source = sourcesController.currentSource}
    {#if sourcesController.error}
      <div class="mb-4 flex items-center justify-between gap-4 rounded-lg border border-destructive/50 bg-destructive/10 p-4 text-destructive">
        <p class="text-sm">{sourcesController.error}</p>
        <Button variant="ghost" size="sm" onclick={() => sourcesController.clearError()}>关闭</Button>
      </div>
    {/if}
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
            <Button
              size="sm"
              variant={copyState === 'failed' ? 'destructive' : 'outline'}
              onclick={() => copyToClipboard(source.script_url)}
            >
              {copyState === 'copied' ? '已复制 ✓' : copyState === 'failed' ? '复制失败,请手动复制' : '复制'}
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
