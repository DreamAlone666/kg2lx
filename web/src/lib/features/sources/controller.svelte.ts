import { adminSession } from '../../state/admin-session.svelte';
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
      this.error = e.message;
    } finally {
      this.loading = false;
    }
  }

  async refreshSource(sourceId: string) {
    if (this.refreshing[sourceId]) return;
    this.refreshing[sourceId] = true;
    try {
      await adminSession.api.refreshSource(sourceId);
      await this.loadSource(sourceId);
      await this.loadSources();
    } catch (e: any) {
      this.error = e.message;
    } finally {
      this.refreshing[sourceId] = false;
    }
  }

  stop() {
    if (this.abortController) {
      this.abortController.abort();
      this.abortController = null;
    }
  }
}

export const sourcesController = new SourcesController();
