import { adminSession, handleAuthError } from '../../state/admin-session.svelte';
import type { SourceSummary, SourceDetail } from '../../api/types';
import { describeApiError } from '../../domain';

class SourcesController {
  sources = $state<SourceSummary[]>([]);
  currentSource = $state<SourceDetail | null>(null);
  loading = $state(false);
  error = $state<string | null>(null);
  refreshing = $state<Record<string, boolean>>({});
  /**
   * 列表最近一次成功拉取的时刻（Unix 秒）。
   *
   * 存在的理由：刷新列表此前唯一的反馈是按钮降到 50% 透明度，而列表数据没变时
   * 画面完全静止，用户无从判断操作是否生效——反馈必须落在动作旁边。
   */
  lastLoadedAt = $state<number | null>(null);

  private abortController: AbortController | null = null;

  async loadSources() {
    this.stop();
    this.abortController = new AbortController();
    this.loading = true;
    this.error = null;
    try {
      const res = await adminSession.api.listSources(this.abortController.signal);
      this.sources = res.items;
      this.lastLoadedAt = Math.floor(Date.now() / 1000);
    } catch (e: any) {
      if (e.name === 'AbortError') return;
      if (handleAuthError(e)) return;
      this.error = describeApiError(e);
    } finally {
      this.loading = false;
    }
  }

  async loadSource(sourceId: string) {
    this.stop();
    this.abortController = new AbortController();
    this.loading = true;
    this.error = null;
    try {
      this.currentSource = await adminSession.api.getSource(sourceId, this.abortController.signal);
    } catch (e: any) {
      if (e.name === 'AbortError') return;
      if (handleAuthError(e)) return;
      this.error = describeApiError(e);
    } finally {
      this.loading = false;
    }
  }

  /**
   * 强制刷新账号状态。
   *
   * 返回三态而不是布尔值：「上一次刷新还没结束」与「刷新失败」是两件事，
   * 用同一个 false 表达会让调用方把前者提示成「刷新失败，请稍后重试」——
   * 用户被指责了一次根本没有发生的失败。
   */
  async refreshSource(sourceId: string): Promise<'ok' | 'failed' | 'busy'> {
    if (this.refreshing[sourceId]) return 'busy';
    this.refreshing[sourceId] = true;
    try {
      await adminSession.api.refreshSource(sourceId);
      await this.loadSource(sourceId);
      await this.loadSources();
      return 'ok';
    } catch (e: any) {
      if (!handleAuthError(e)) this.error = describeApiError(e);
      return 'failed';
    } finally {
      this.refreshing[sourceId] = false;
    }
  }

  clearError() {
    this.error = null;
  }

  stop() {
    if (this.abortController) {
      this.abortController.abort();
      this.abortController = null;
    }
  }
}

export const sourcesController = new SourcesController();
