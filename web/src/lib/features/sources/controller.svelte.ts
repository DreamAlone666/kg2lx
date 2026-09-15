import { adminSession, handleAuthError } from '../../state/admin-session.svelte';
import type { SourceSummary, SourceDetail } from '../../api/types';

class SourcesController {
  sources = $state<SourceSummary[]>([]);
  currentSource = $state<SourceDetail | null>(null);
  loading = $state(false);
  error = $state<string | null>(null);
  refreshing = $state<Record<string, boolean>>({});

  private abortController: AbortController | null = null;

  async loadSources() {
    this.stop();
    this.abortController = new AbortController();
    this.loading = true;
    this.error = null;
    try {
      const res = await adminSession.api.listSources(this.abortController.signal);
      this.sources = res.items;
    } catch (e: any) {
      if (e.name === 'AbortError') return;
      if (handleAuthError(e)) return;
      this.error = e.message;
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
      this.error = e.message;
    } finally {
      this.loading = false;
    }
  }

  /**
   * 强制刷新账号状态。
   * @returns 是否成功，供调用方决定给出成功还是失败反馈。
   */
  async refreshSource(sourceId: string): Promise<boolean> {
    if (this.refreshing[sourceId]) return false;
    this.refreshing[sourceId] = true;
    try {
      await adminSession.api.refreshSource(sourceId);
      await this.loadSource(sourceId);
      await this.loadSources();
      return true;
    } catch (e: any) {
      if (!handleAuthError(e)) this.error = e.message;
      return false;
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
