import { adminSession, handleAuthError } from '../../state/admin-session.svelte';
import type { RuntimeLogListItem, RuntimeLogView } from '../../api/types';

/**
 * 运行时日志拉取与轮询。
 *
 * 竞态防护只需一个单调递增的 requestId:
 * 每次发起请求分配新序号,回包时序号不是最新的就直接丢弃。
 * 切换音源 / 切换视图 / 手动刷新的旧响应全部自然失效。
 */
class SourceLogsController {
  logs = $state<RuntimeLogListItem[]>([]);
  view = $state<RuntimeLogView>('all');
  loading = $state(false);
  error = $state<string | null>(null);

  private requestId = 0;
  private intervalId: ReturnType<typeof setInterval> | null = null;
  private currentSourceId: string | null = null;

  async loadLogs(sourceId: string, silent = false) {
    const requestId = ++this.requestId;
    if (!silent) this.loading = true;

    try {
      const res = await adminSession.api.listSourceLogs(sourceId, {
        limit: 20,
        view: this.view,
      });
      if (requestId !== this.requestId || this.currentSourceId !== sourceId) return;
      this.logs = res.items;
      this.error = null;
    } catch (e: any) {
      if (handleAuthError(e)) return;
      if (requestId !== this.requestId || this.currentSourceId !== sourceId) return;
      this.error = e.message;
    } finally {
      if (!silent && requestId === this.requestId) this.loading = false;
    }
  }

  startPolling(sourceId: string) {
    this.stopPolling();
    this.currentSourceId = sourceId;
    this.loadLogs(sourceId, true);
    this.intervalId = setInterval(() => {
      if (this.currentSourceId === sourceId) this.loadLogs(sourceId, true);
    }, 5000);
  }

  stopPolling() {
    if (this.intervalId) {
      clearInterval(this.intervalId);
      this.intervalId = null;
    }
    this.requestId++; // 使在途请求的回包失效
    this.currentSourceId = null;
    this.logs = [];
    this.error = null;
    this.loading = false;
  }

  setView(view: RuntimeLogView) {
    if (this.view === view) return;
    this.view = view;
    if (this.currentSourceId) this.loadLogs(this.currentSourceId, true);
  }

  refresh() {
    if (this.currentSourceId) this.loadLogs(this.currentSourceId);
  }
}

export const sourceLogsController = new SourceLogsController();
