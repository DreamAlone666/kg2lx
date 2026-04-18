<script lang="ts">
  import { untrack } from 'svelte';
  import { sourceLogsController } from '../../features/source-logs/controller.svelte';
  import { Button, Badge, Card, CardHeader, CardContent } from '../ui';

  let { sourceId } = $props<{ sourceId: string }>();

  $effect(() => {
    if (!sourceId) return;
    untrack(() => sourceLogsController.startPolling(sourceId));
    return () => untrack(() => sourceLogsController.stopPolling());
  });

  function formatTime(timestamp: number) {
    return new Date(timestamp * 1000).toLocaleTimeString([], {
      hour: '2-digit',
      minute: '2-digit',
      second: '2-digit',
      hour12: false
    });
  }

  function getQualityVariant(quality: string) {
    if (quality.includes('128k')) return 'outline';
    if (quality.includes('320k')) return 'default';
    if (quality.includes('flac')) return 'default';
    return 'secondary';
  }

  const stageMap: Record<string, string> = {
    precheck: '预检',
    ensure_dfid: 'DFID',
    refresh_login: '刷新登录',
    fetch_music_url: '取链'
  };
</script>

<Card class="mt-8">
  <CardHeader class="border-b">
    <div class="flex items-center justify-between">
      <div class="flex items-center gap-4">
        <h2 class="text-lg font-semibold">运行时日志</h2>
        <div class="flex bg-muted rounded-md p-0.5">
          <button
            class="px-3 py-1 text-xs rounded-sm transition-colors {sourceLogsController.view === 'all' ? 'bg-background shadow-sm' : 'hover:text-foreground/80 text-muted-foreground'}"
            onclick={() => sourceLogsController.setView('all')}
          >
            全部
          </button>
          <button
            class="px-3 py-1 text-xs rounded-sm transition-colors {sourceLogsController.view === 'errors' ? 'bg-background shadow-sm' : 'hover:text-foreground/80 text-muted-foreground'}"
            onclick={() => sourceLogsController.setView('errors')}
          >
            错误
          </button>
        </div>
      </div>
      <Button
        variant="ghost"
        size="sm"
        onclick={() => sourceLogsController.refresh()}
        disabled={sourceLogsController.loading}
      >
        <span class={sourceLogsController.loading ? 'animate-spin' : ''}>
          {sourceLogsController.loading ? '...' : '刷新'}
        </span>
      </Button>
    </div>
  </CardHeader>

  <CardContent class="p-0">
    {#if sourceLogsController.error}
      <div class="p-8 text-center">
        <p class="text-sm text-destructive mb-4">加载日志失败: {sourceLogsController.error}</p>
        <Button size="sm" onclick={() => sourceLogsController.refresh()}>重试</Button>
      </div>
    {:else if sourceLogsController.logs.length === 0}
      <div class="p-12 text-center text-muted-foreground">
        <p class="text-sm">暂无运行时流量记录</p>
      </div>
    {:else}
      <div class="overflow-x-auto">
        <table class="w-full text-xs text-left border-collapse min-w-[800px]">
          <thead>
            <tr class="border-b bg-muted/50 text-muted-foreground font-medium">
              <th class="px-4 py-2">歌曲信息</th>
              <th class="px-4 py-2 w-20">音质</th>
              <th class="px-4 py-2 w-24">状态</th>
              <th class="px-4 py-2 w-20">时间</th>
              <th class="px-4 py-2 w-20">延迟</th>
              <th class="px-4 py-2">诊断信息</th>
            </tr>
          </thead>
          <tbody class="divide-y">
            {#each sourceLogsController.logs as log (log.log_id)}
              <tr class="hover:bg-muted/30 transition-colors">
                <td class="px-4 py-2.5">
                  <div class="flex flex-col gap-0.5">
                    {#if log.track_title}
                      <span class="font-medium text-foreground">{log.track_title}</span>
                      <span class="text-muted-foreground">
                        {log.artist_name || '未知歌手'}
                        {#if log.album_name}
                          <span class="opacity-60 ml-1">· {log.album_name}</span>
                        {/if}
                      </span>
                    {:else}
                      <span class="font-mono text-muted-foreground italic">无歌曲元数据</span>
                      {#if log.album_audio_id}
                        <span class="text-[10px] text-muted-foreground font-mono">{log.album_audio_id}</span>
                      {/if}
                    {/if}
                  </div>
                </td>
                <td class="px-4 py-2.5">
                  <Badge variant={getQualityVariant(log.requested_quality)} class="px-1.5 py-0 font-mono">
                    {log.requested_quality}
                  </Badge>
                </td>
                <td class="px-4 py-2.5">
                  <div class="flex flex-col gap-1">
                    <Badge variant={log.ok ? 'default' : 'destructive'} class="px-1.5 py-0 w-fit">
                      {log.ok ? 'OK' : log.error_code || 'ERR'}
                    </Badge>
                    {#if !log.ok && log.error}
                      <span class="text-[10px] text-destructive truncate max-w-[120px]" title={log.error}>
                        {log.error}
                      </span>
                    {/if}
                  </div>
                </td>
                <td class="px-4 py-2.5 whitespace-nowrap text-muted-foreground">
                  {formatTime(log.created_at)}
                </td>
                <td class="px-4 py-2.5 whitespace-nowrap font-mono">
                  {log.latency_ms}ms
                </td>
                <td class="px-4 py-2.5">
                  <div class="flex flex-wrap items-center gap-x-3 gap-y-1 text-muted-foreground/80">
                    <span class="bg-muted px-1 rounded text-[10px]">{stageMap[log.stage] || log.stage}</span>
                    <span class="font-mono text-[10px]">#{log.request_hash.slice(0, 7)}</span>
                    {#if log.refresh_attempted}
                      <span class="text-[10px] text-amber-600 font-medium">已刷新Token</span>
                    {/if}
                    {#if log.retry_count > 0}
                      <span class="text-[10px] text-amber-600 font-medium">重试:{log.retry_count}</span>
                    {/if}
                  </div>
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  </CardContent>
</Card>
