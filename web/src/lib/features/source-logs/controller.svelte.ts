import { adminSession } from '../../state/admin-session.svelte';
import type { RuntimeLogListItem, RuntimeLogView } from '../../api/types';

class SourceLogsController {
  logs = $state<RuntimeLogListItem[]>([]);
  view = $state<RuntimeLogView>('all');
  loading = $state(false);
  error = $state<string | null>(null);

  private abortControllers = new Set<AbortController>();
  private intervalId: ReturnType<typeof setInterval> | null = null;
  private currentSourceId: string | null = null;
  private requestSeq = 0;
  private latestSettledRequestSeq = 0;
  private latestInteractiveRequestSeq = 0;

  async loadLogs(sourceId: string, silent = false) {
    const requestId = ++this.requestSeq;
    const requestedView = this.view;
    const abortController = new AbortController();

    this.abortControllers.add(abortController);

    if (!silent) {
      this.loading = true;
      this.error = null;
      this.latestInteractiveRequestSeq = requestId;
    }

    try {
      const res = await adminSession.api.listSourceLogs(
        sourceId,
        { limit: 20, view: requestedView },
        abortController.signal
      );

      if (!this.shouldApplyResult(requestId, sourceId, requestedView)) return;

      this.latestSettledRequestSeq = requestId;
      this.logs = res.items;
      this.error = null;
    } catch (e: any) {
      if (e.name === 'AbortError') return;
      if (!this.shouldApplyResult(requestId, sourceId, requestedView)) return;

      this.latestSettledRequestSeq = requestId;
      this.error = e.message;
    } finally {
      this.abortControllers.delete(abortController);

      if (!silent && this.latestInteractiveRequestSeq === requestId) {
        this.loading = false;
      }
    }
  }

  startPolling(sourceId: string) {
    this.stopPolling();
    this.currentSourceId = sourceId;
    this.loadLogs(sourceId, true);
    this.intervalId = setInterval(() => {
      if (this.currentSourceId === sourceId) {
        this.loadLogs(sourceId, true);
      }
    }, 5000);
  }

  stopPolling() {
    if (this.intervalId) {
      clearInterval(this.intervalId);
      this.intervalId = null;
    }

    for (const controller of this.abortControllers) {
      controller.abort();
    }
    this.abortControllers.clear();

    this.currentSourceId = null;
    this.loading = false;
    this.latestSettledRequestSeq = 0;
    this.latestInteractiveRequestSeq = 0;
  }

  setView(view: RuntimeLogView) {
    if (this.view === view) return;

    this.view = view;
    if (this.currentSourceId) {
      this.loadLogs(this.currentSourceId, true);
    }
  }

  refresh() {
    if (this.currentSourceId) {
      this.loadLogs(this.currentSourceId);
    }
  }

  private shouldApplyResult(requestId: number, sourceId: string, view: RuntimeLogView) {
    if (this.currentSourceId !== sourceId) return false;
    if (this.view !== view) return false;
    return requestId >= this.latestSettledRequestSeq;
  }
}

export const sourceLogsController = new SourceLogsController();
