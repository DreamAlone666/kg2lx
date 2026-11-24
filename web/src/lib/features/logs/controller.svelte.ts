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

	private logRequest: AbortController | null = null;
	private olderRequest: AbortController | null = null;
	private correlationRequest: AbortController | null = null;
	private accountsRequest: AbortController | null = null;
	private active = false;
	private skipNextHighlight = false;
	private newRowTimer: ReturnType<typeof setTimeout> | null = null;
	private onStaleSelection: (() => void) | null = null;

	private timer = createVisibilityAwareTimer({
		intervalMs: POLL_INTERVAL_MS,
		onTick: () => {
			if (!this.active || this.logRequest) return;
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
		this.logRequest?.abort();
		this.logRequest = null;
		this.accountsRequest?.abort();
		this.accountsRequest = null;
		this.cancelMore();
		this.clearNewRowTimer();
		this.onStaleSelection = null;

		this.logs = [];
		this.accounts = [];
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
		if (!this.active || this.loading || this.loadingMore || !this.hasMore || this.nextBeforeSeq === null) return;
		const request = new AbortController();
		this.olderRequest = request;
		this.loadingMore = true;

		try {
			const res = await adminSession.api.listLogs({
				limit: PAGE_SIZE,
				view: this.view,
				eventType: this.eventType,
				accountId: this.accountId,
				beforeSeq: this.nextBeforeSeq,
			}, request.signal);
			if (request.signal.aborted) return;

			const seen = new Set(this.logs.map((item) => item.log_id));
			this.logs = [...this.logs, ...res.items.filter((item) => !seen.has(item.log_id))];
			this.hasMore = res.has_more;
			this.nextBeforeSeq = res.next_before_seq;
			this.error = null;
		} catch (error) {
			if (!request.signal.aborted && !handleAuthError(error)) {
				this.error = describeApiError(error);
			}
		} finally {
			if (request === this.olderRequest) {
				this.olderRequest = null;
				this.loadingMore = false;
			}
		}
	}

	private cancelMore() {
		this.olderRequest?.abort();
		this.olderRequest = null;
		this.loadingMore = false;
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
		this.correlationRequest?.abort();
		const request = new AbortController();
		this.correlationRequest = request;
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
			}, request.signal);
			if (request.signal.aborted) return;
			this.correlationLogs = sortEventsAscending(res.items);
			this.correlationError = null;
		} catch (e: unknown) {
			if (request.signal.aborted || handleAuthError(e)) return;
			this.correlationError = describeApiError(e);
		} finally {
			if (request === this.correlationRequest) {
				this.correlationRequest = null;
				this.loadingCorrelation = false;
			}
		}
	}

	accountDisplayName(accountId: string | null): string {
		if (!accountId) return '扫码登录';
		const found = this.accounts.find((a) => a.account_id === accountId);
		if (!found) return accountId.slice(0, 8);
		return found.sources[0]?.name || `账号 ${found.userid}`;
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
		this.accountsRequest?.abort();
		const request = new AbortController();
		this.accountsRequest = request;
		try {
			const res = await adminSession.api.listAccounts(request.signal);
			if (!request.signal.aborted) this.accounts = res.items;
		} catch (error) {
			if (!request.signal.aborted) handleAuthError(error);
		} finally {
			if (request === this.accountsRequest) {
				this.accountsRequest = null;
				this.accountsLoaded = true;
			}
		}
	}

	private async loadLogs(silent: boolean) {
		this.logRequest?.abort();
		const request = new AbortController();
		this.logRequest = request;
		if (!silent) this.cancelMore();
		this.loading = !silent;
		const afterSeq = silent ? this.logs[0]?.seq : undefined;
		const filters = { view: this.view, eventType: this.eventType, accountId: this.accountId };
		const api = adminSession.api;

		try {
			const response = await api.listLogs({
				...filters,
				limit: afterSeq === undefined ? PAGE_SIZE : INCREMENTAL_LIMIT,
				afterSeq,
			}, request.signal);
			if (request.signal.aborted) return;
			const items = [...response.items];

			// Bridge a large burst back to the previous head before moving the incremental cursor.
			if (afterSeq !== undefined && response.has_more) {
				let beforeSeq = items.at(-1)?.seq;
				while (beforeSeq !== undefined && beforeSeq > afterSeq) {
					const page = await api.listLogs({
						...filters, limit: INCREMENTAL_LIMIT, beforeSeq,
					}, request.signal);
					if (request.signal.aborted) return;
					items.push(...page.items.filter(item => item.seq > afterSeq));
					const next = page.next_before_seq;
					beforeSeq = next !== null && next < beforeSeq ? next : undefined;
				}
			}

			const knownIds = new Set(this.logs.map((log) => log.log_id));
			const hadRows = this.logs.length > 0;
			const newlyAdded = items.filter(item => !knownIds.has(item.log_id));
			if (afterSeq !== undefined) {
				if (newlyAdded.length > 0) this.logs = [...newlyAdded, ...this.logs];
			} else {
				this.logs = items;
				this.hasMore = response.has_more;
				this.nextBeforeSeq = response.next_before_seq;
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
			if (request.signal.aborted || handleAuthError(e)) return;
			this.error = describeApiError(e);
		} finally {
			if (request === this.logRequest) {
				this.logRequest = null;
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
		this.correlationRequest?.abort();
		this.correlationRequest = null;
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
adminSession.onReset(() => logsController.stop());
