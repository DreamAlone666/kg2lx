<script lang="ts">
  import { onMount } from 'svelte';
  import { adminSession } from '$lib/state/admin-session.svelte';
  import { sourcesController } from '$lib/features/sources/controller.svelte';
  import { goto } from '$app/navigation';
  import { Button, Badge, Card, CardHeader, CardContent } from '$lib/components/ui';

  onMount(() => {
    if (!adminSession.adminToken) {
      goto('/connect');
      return;
    }
    sourcesController.loadSources();
  });

  function formatDate(timestamp: number) {
    return new Date(timestamp * 1000).toLocaleString();
  }

  const providerMap: Record<string, string> = {
    kugou_lite: '酷狗概念版'
  };
</script>

<svelte:head>
  <title>音源管理 | kg2lx</title>
</svelte:head>

<div class="mx-auto max-w-5xl p-6">
  <div class="mb-8 flex flex-col gap-4 sm:flex-row sm:items-center sm:justify-between">
    <div>
      <h1 class="text-3xl font-bold tracking-tight">音源管理</h1>
      <p class="text-muted-foreground">管理已绑定的音乐账号。</p>
    </div>
    <div class="flex items-center gap-2">
      <Button variant="outline" onclick={() => sourcesController.loadSources()}>
        刷新列表
      </Button>
      <Button href="/bind">
        添加音源
      </Button>
    </div>
  </div>

  {#if sourcesController.loading && sourcesController.sources.length === 0}
    <div class="flex h-64 items-center justify-center">
      <div class="animate-spin rounded-full h-8 w-8 border-b-2 border-primary"></div>
    </div>
  {:else if sourcesController.error}
    <div class="rounded-lg border border-destructive/50 bg-destructive/10 p-4 text-destructive">
      <p class="font-semibold">加载音源出错</p>
      <p class="text-sm">{sourcesController.error}</p>
      <Button variant="outline" size="sm" class="mt-4" onclick={() => sourcesController.loadSources()}>
        重试
      </Button>
    </div>
  {:else if sourcesController.sources.length === 0}
    <Card class="flex h-64 flex-col items-center justify-center border-dashed">
      <p class="text-muted-foreground">未发现音源。</p>
      <Button variant="link" href="/bind">
        绑定你的第一个账号
      </Button>
    </Card>
  {:else}
    <div class="grid gap-4 md:grid-cols-2 lg:grid-cols-3">
      {#each sourcesController.sources as source (source.source_id)}
        <a href="/sources/{source.source_id}">
          <Card class="h-full transition-shadow hover:shadow-md">
            <CardHeader class="pb-2">
              <div class="flex items-center justify-between">
                <Badge variant="secondary">
                  {providerMap[source.provider] || source.provider}
                </Badge>
                {#if source.vip_active}
                  <Badge variant="default" class="bg-yellow-500 hover:bg-yellow-600">
                    VIP
                  </Badge>
                {/if}
              </div>
              <h2 class="mt-2 font-bold text-lg truncate">{source.userid}</h2>
            </CardHeader>
            <CardContent>
              <p class="text-xs font-mono text-muted-foreground truncate">ID: {source.source_id}</p>
              <div class="mt-4 text-xs text-muted-foreground">
                更新于: {formatDate(source.updated_at)}
              </div>
            </CardContent>
          </Card>
        </a>
      {/each}
    </div>
  {/if}
</div>
