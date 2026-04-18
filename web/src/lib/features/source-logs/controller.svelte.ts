import { adminSession } from '../../state/admin-session.svelte';
import type { RuntimeLogListItem, RuntimeLogView } from '../../api/types';

class SourceLogsController {
  logs = $state<RuntimeLogListItem[]>([]);
  view = $state<RuntimeLogView>('all');
  loading = $state(false);
  error = $state<string | null>(null);

  private abortController: AbortController | null = null;
  private intervalId: any = null;
  private currentSourceId: string | null = null;
  private requestInFlight = false;

  async loadLogs(sourceId: string, silent = false) {
    if (this.requestInFlight) return;

    if (!silent) {
      this.loading = true;
      this.error = null;
    }

    const abortController = new AbortController();
    this.abortController = abortController;
    this.requestInFlight = true;

    try {
      const res = await adminSession.api.listSourceLogs(
        sourceId,
        { limit: 20, view: this.view },
        abortController.signal
      );
      this.logs = res.items;
      this.error = null;
    } catch (e: any) {
      if (e.name === 'AbortError') return;
      this.error = e.message;
    } finally {
      this.requestInFlight = false;
      if (this.abortController === abortController) this.abortController = null;
      if (!silent) {
        this.loading = false;
      }
    }
  }

  startPolling(sourceId: string) {
    this.stopPolling();
    this.currentSourceId = sourceId;
    this.loadLogs(sourceId);
    this.intervalId = setInterval(() => {
      this.loadLogs(sourceId, true);
    }, 5000);
  }

  stopPolling() {
    if (this.intervalId) {
      clearInterval(this.intervalId);
      this.intervalId = null;
    }
    if (this.abortController) {
      this.abortController.abort();
      this.abortController = null;
    }
    this.currentSourceId = null;
  }

  setView(view: RuntimeLogView) {
    if (this.view === view) return;
    this.view = view;
    if (this.currentSourceId) {
      this.loadLogs(this.currentSourceId);
    }
  }

  refresh() {
    if (this.currentSourceId) {
      this.loadLogs(this.currentSourceId);
    }
  }
}

export const sourceLogsController = new SourceLogsController();
