import { adminSession, handleAuthError } from '$lib/state/admin-session.svelte';
import { toast } from '$lib/state/toast.svelte';
import { createVisibilityAwareTimer } from '$lib/utils/polling';
import { describeApiError, sortEventsAscending } from '$lib/domain';
import type { AccountItem, EventType, LogView, StoredEvent } from '$lib/api/types';

const PAGE_SIZE = 30;
const INCREMENTAL_LIMIT = 100;
const POLL_INTERVAL_MS = 3000;
const NEW_ROW_MARK_MS = 1000;

export interface LogFilters {
	view: LogView;
	eventType: EventType | undefined;
	accountId: string | undefined;
}

/**
 * 全站事件日志与因果链路控制器。
 *
 * - 支持多维筛选（事件类别 / 账号归属 / 仅错误）
 * - 支持游标分页（before_seq = next_before_seq 加载更早记录）与增量轮询（after_seq 合并新事件并保留已展开的更早页）
 * - 支持按 correlation_id 拉取完整正序因果链路供检查器展示
 */
class LogsController {
	logs = $state<StoredEvent[]>([]);
	accounts = $state<AccountItem[]>([]);
	accountsLoaded = $state(false);
	view = $state<LogView>('all');
	eventType = $state<EventType | undefined>(undefined);
	accountId = $state<string | undefined>(undefined);
	selectedLogId = $state<string | null>(null);

	loading = $state(false);
	loadingMore = $state(false);
	hasMore = $state(false);
	nextBeforeSeq = $state<number | null>(null);
	error = $state<string | null>(null);
	newLogIds = $state<Set<string>>(new Set());
	lastUpdatedAt = $state<number | null>(null);
	paused = $state(false);

	correlationId = $state<string | null>(null);
	correlationLogs = $state<StoredEvent[]>([]);
	loadingCorrelation = $state(false);
	correlationError = $state<string | null>(null);

	private requestId = 0;
	private correlationRequestId = 0;
	private accountsRequestId = 0;
	private active = false;
	private skipNextHighlight = false;
	private newRowTimer: ReturnType<typeof setTimeout> | null = null;
	private pending = new Set<number>();
	private onStaleSelection: (() => void) | null = null;

	private timer = createVisibilityAwareTimer({
		intervalMs: POLL_INTERVAL_MS,
		onTick: () => {
			if (!this.active) return;
			if (this.pending.size > 0) return;
			void this.loadLogs(true);
		},
	});

	get selectedEvent(): StoredEvent | null {
		if (!this.selectedLogId) return null;
		return (
			this.logs.find((item) => item.log_id === this.selectedLogId) ??
			this.correlationLogs.find((item) => item.log_id === this.selectedLogId) ??
			null
		);
	}

	start(
		initial: LogFilters & { selectedLogId: string | null },
		options?: { onStaleSelection?: () => void }
	) {
		this.stop();
		this.active = true;
		this.view = initial.view;
		this.eventType = initial.eventType;
		this.accountId = initial.accountId;
		this.selectedLogId = initial.selectedLogId;
		this.skipNextHighlight = true;
		this.onStaleSelection = options?.onStaleSelection ?? null;

		void this.loadAccounts();
		void this.loadLogs(false);
		this.timer.start();
	}

	stop() {
		this.active = false;
		this.timer.stop();
		this.requestId++;
		this.correlationRequestId++;
		this.accountsRequestId++;
		this.pending.clear();
		this.clearNewRowTimer();
		this.onStaleSelection = null;

		this.logs = [];
		this.accountsLoaded = false;
		this.hasMore = false;
		this.nextBeforeSeq = null;
		this.error = null;
		this.newLogIds = new Set();
		this.lastUpdatedAt = null;
		this.paused = false;
		this.loading = false;
		this.loadingMore = false;
		this.selectedLogId = null;
		this.clearCorrelation();
	}

	syncFromUrl(next: LogFilters & { selectedLogId: string | null }) {
		if (!this.active) return;
		const filtersChanged =
			this.view !== next.view ||
			this.eventType !== next.eventType ||
			this.accountId !== next.accountId;

		this.selectedLogId = next.selectedLogId;

		if (filtersChanged) {
			this.applyFilters(next);
		} else {
			this.ensureCorrelationForSelected();
		}
	}

	setFilters(next: LogFilters) {
		if (
			this.view === next.view &&
			this.eventType === next.eventType &&
			this.accountId === next.accountId
		) {
			return;
		}
		this.applyFilters(next);
	}

	selectEvent(logId: string | null) {
		this.selectedLogId = logId;
		if (!logId) {
			this.clearCorrelation();
			return;
		}
		this.ensureCorrelationForSelected();
	}

	setPaused(paused: boolean) {
		if (this.paused === paused) return;
		this.paused = paused;
		if (paused) {
			this.timer.pause();
			return;
		}
		this.skipNextHighlight = true;
		this.timer.resume();
	}

	refresh() {
		if (!this.active || this.loading) return;
		void this.loadLogs(false);
		void this.loadAccounts();
		const selected = this.selectedEvent;
		if (selected) {
			void this.loadCorrelation(selected.correlation_id, true);
		}
	}

	async loadMore() {
		if (!this.active || this.loadingMore || !this.hasMore || this.nextBeforeSeq === null) return;
		const requestId = this.requestId;
		const beforeSeq = this.nextBeforeSeq;
		this.loadingMore = true;

		try {
			const res = await adminSession.api.listLogs({
				limit: PAGE_SIZE,
				view: this.view,
				eventType: this.eventType,
				accountId: this.accountId,
				beforeSeq,
			});
			if (!this.active || requestId !== this.requestId) return;

			const seen = new Set(this.logs.map((item) => item.log_id));
			const appended = res.items.filter((item) => !seen.has(item.log_id));
			this.logs = [...this.logs, ...appended];
			this.hasMore = res.has_more;
			this.nextBeforeSeq = res.next_before_seq;
			this.error = null;
		} catch (e: unknown) {
			if (handleAuthError(e)) return;
			if (!this.active || requestId !== this.requestId) return;
			this.error = describeApiError(e);
		} finally {
			if (this.active && requestId === this.requestId) this.loadingMore = false;
		}
	}

	async loadCorrelation(correlationId: string, force = false) {
		if (
			!force &&
			this.correlationId === correlationId &&
			this.correlationLogs.length > 0 &&
			!this.correlationError
		) {
			return;
		}
		const reqId = ++this.correlationRequestId;
		const sameCorrelation = this.correlationLogs[0]?.correlation_id === correlationId;
		this.correlationId = correlationId;
		this.loadingCorrelation = !sameCorrelation || !force;
		this.correlationError = null;
		if (!sameCorrelation) {
			this.correlationLogs = [];
		}

		try {
			const res = await adminSession.api.listLogs({
				correlationId,
				limit: 100,
			});
			if (!this.active || reqId !== this.correlationRequestId) return;
			this.correlationLogs = sortEventsAscending(res.items);
			this.correlationError = null;
		} catch (e: unknown) {
			if (handleAuthError(e)) return;
			if (!this.active || reqId !== this.correlationRequestId) return;
			this.correlationError = describeApiError(e);
		} finally {
			if (this.active && reqId === this.correlationRequestId) {
				this.loadingCorrelation = false;
			}
		}
	}

	accountDisplayName(accountId: string | null): string {
		if (!accountId) return '扫码登录';
		const found = this.accounts.find((a) => a.account_id === accountId);
		if (!found) return accountId.slice(0, 8);
		return found.sources[0]?.name || found.display_name || `账号 ${found.userid}`;
	}

	sourceDisplayName(accountId: string | null, sourceId: string | null): string | null {
		if (!sourceId) return null;
		const accountsToSearch = accountId
			? this.accounts.filter((a) => a.account_id === accountId)
			: this.accounts;
		for (const acc of accountsToSearch) {
			const src = acc.sources.find((s) => s.source_id === sourceId);
			if (src) return src.name;
		}
		return sourceId;
	}

	private applyFilters(next: LogFilters) {
		this.view = next.view;
		this.eventType = next.eventType;
		this.accountId = next.accountId;
		this.selectedLogId = null;
		this.clearCorrelation();
		this.logs = [];
		this.hasMore = false;
		this.nextBeforeSeq = null;
		this.error = null;
		this.skipNextHighlight = true;
		void this.loadLogs(false);
	}

	private async loadAccounts() {
		const reqId = ++this.accountsRequestId;
		try {
			const res = await adminSession.api.listAccounts();
			if (!this.active || reqId !== this.accountsRequestId) return;
			this.accounts = res.items;
		} catch (e: unknown) {
			if (handleAuthError(e)) return;
			// 账号列表用于筛选条与名称映射，失败时不阻塞日志主体展示
		} finally {
			if (this.active && reqId === this.accountsRequestId) {
				this.accountsLoaded = true;
			}
		}
	}

	private async loadLogs(silent: boolean) {
		const requestId = ++this.requestId;
		this.pending.add(requestId);
		if (!silent) this.loading = true;

		const useIncremental = silent && this.logs.length > 0;

		try {
			const res = await adminSession.api.listLogs({
				limit: useIncremental ? INCREMENTAL_LIMIT : PAGE_SIZE,
				view: this.view,
				eventType: this.eventType,
				accountId: this.accountId,
				afterSeq: useIncremental ? this.logs[0].seq : undefined,
			});
			if (!this.active || requestId !== this.requestId) return;

			const knownIds = new Set(this.logs.map((log) => log.log_id));
			const hadRows = this.logs.length > 0;
			let newlyAdded: StoredEvent[] = [];

			if (useIncremental && res.has_more) {
				// 突发超过 100 条：回退做一次全量头部刷新
				const fullRes = await adminSession.api.listLogs({
					limit: PAGE_SIZE,
					view: this.view,
					eventType: this.eventType,
					accountId: this.accountId,
				});
				if (!this.active || requestId !== this.requestId) return;

				newlyAdded = fullRes.items.filter((item) => !knownIds.has(item.log_id));
				this.logs = fullRes.items;
				this.hasMore = fullRes.has_more;
				this.nextBeforeSeq = fullRes.next_before_seq;
			} else if (useIncremental) {
				if (res.items.length > 0) {
					newlyAdded = res.items.filter((item) => !knownIds.has(item.log_id));
					if (newlyAdded.length > 0) {
						this.logs = [...newlyAdded, ...this.logs];
					}
				}
			} else {
				newlyAdded = res.items.filter((item) => !knownIds.has(item.log_id));
				this.logs = res.items;
				this.hasMore = res.has_more;
				this.nextBeforeSeq = res.next_before_seq;
			}

			this.lastUpdatedAt = Math.floor(Date.now() / 1000);
			this.markNewRows(newlyAdded, knownIds, hadRows);
			this.error = null;

			if (!silent && this.selectedLogId && !this.loadingCorrelation && this.selectedEvent === null) {
				this.selectedLogId = null;
				this.clearCorrelation();
				toast.push({
					tone: 'info',
					message: '未在最近日志中找到指定记录，可尝试加载更早记录。',
				});
				this.onStaleSelection?.();
			} else {
				this.ensureCorrelationForSelected();
				const selected = this.selectedEvent;
				if (
					silent &&
					selected &&
					newlyAdded.some((item) => item.correlation_id === selected.correlation_id)
				) {
					void this.loadCorrelation(selected.correlation_id, true);
				}
			}
		} catch (e: unknown) {
			if (handleAuthError(e)) return;
			if (!this.active || requestId !== this.requestId) return;
			this.error = describeApiError(e);
		} finally {
			this.pending.delete(requestId);
			if (!silent && requestId === this.requestId) {
				this.loading = false;
			}
		}
	}

	private ensureCorrelationForSelected() {
		const selected = this.selectedEvent;
		if (!selected) return;
		if (this.correlationId !== selected.correlation_id) {
			void this.loadCorrelation(selected.correlation_id);
		}
	}

	private clearCorrelation() {
		this.correlationId = null;
		this.correlationLogs = [];
		this.loadingCorrelation = false;
		this.correlationError = null;
	}

	private markNewRows(items: StoredEvent[], knownIds: Set<string>, hadRows: boolean) {
		if (!hadRows || this.skipNextHighlight) {
			this.skipNextHighlight = false;
			this.newLogIds = new Set();
			return;
		}
		const added = new Set(
			items.filter((item) => !knownIds.has(item.log_id)).map((item) => item.log_id)
		);
		if (added.size === 0) return;

		this.newLogIds = added;
		this.clearNewRowTimer();
		this.newRowTimer = setTimeout(() => {
			this.newLogIds = new Set();
			this.newRowTimer = null;
		}, NEW_ROW_MARK_MS);
	}

	private clearNewRowTimer() {
		if (this.newRowTimer !== null) {
			clearTimeout(this.newRowTimer);
			this.newRowTimer = null;
		}
	}
}

export const logsController = new LogsController();
