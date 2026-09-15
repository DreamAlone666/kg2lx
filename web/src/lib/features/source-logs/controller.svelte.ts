import { adminSession, handleAuthError } from '../../state/admin-session.svelte';
import { createVisibilityAwareTimer } from '../../utils/polling';
import type { RuntimeLogListItem, RuntimeLogView } from '../../api/types';

/** 轮询间隔。取链是低频事件，5 秒足够及时且不会打扰后端。 */
const POLL_INTERVAL_MS = 5000;

/** 新行高亮的总时长，略长于关键帧动画，确保动画播完再撤掉标记。 */
const NEW_ROW_MARK_MS = 1000;

/**
 * 运行时日志拉取与轮询。
 *
 * 竞态防护只需一个单调递增的 requestId：每次发起请求分配新序号，
 * 回包时序号不是最新的就直接丢弃。切换音源 / 切换视图 / 手动刷新的旧响应全部自然失效。
 *
 * 节奏交给 createVisibilityAwareTimer：页面隐藏时暂停，恢复可见时立即补拉，
 * 避免用户在后台空转请求、切回来又先看到过期数据。
 */
class SourceLogsController {
	logs = $state<RuntimeLogListItem[]>([]);
	view = $state<RuntimeLogView>('all');
	loading = $state(false);
	error = $state<string | null>(null);
	/** 本次拉取中新出现的 log_id。仅用于让新行播放进入动画，旧行不重播。 */
	newLogIds = $state<Set<string>>(new Set());
	/**
	 * 最近一次**成功**拉取的时刻（Unix 秒）。
	 *
	 * 失败时 logs 刻意保留不清空（拉取失败不等于已有数据失效），因此界面必须能
	 * 说清「你看到的这批是哪个时刻的」——否则保留数据会退化成「显示过期数据而不自知」。
	 */
	lastUpdatedAt = $state<number | null>(null);

	private requestId = 0;
	private currentSourceId: string | null = null;
	private newRowTimer: ReturnType<typeof setTimeout> | null = null;
	/** 视图切换会整表替换，不应让整屏都播放「新增」动画，故跳过下一次标记。 */
	private skipNextHighlight = false;

	private timer = createVisibilityAwareTimer({
		intervalMs: POLL_INTERVAL_MS,
		onTick: () => {
			if (this.currentSourceId) this.loadLogs(this.currentSourceId, true);
		}
	});

	async loadLogs(sourceId: string, silent = false) {
		const requestId = ++this.requestId;
		if (!silent) this.loading = true;

		try {
			const res = await adminSession.api.listSourceLogs(sourceId, {
				limit: 20,
				view: this.view
			});
			if (requestId !== this.requestId || this.currentSourceId !== sourceId) return;

			const knownIds = new Set(this.logs.map((log) => log.log_id));
			const hadRows = this.logs.length > 0;
			this.logs = res.items;
			this.lastUpdatedAt = Math.floor(Date.now() / 1000);
			this.markNewRows(res.items, knownIds, hadRows);
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
		this.timer.start();
	}

	stopPolling() {
		this.timer.stop();
		this.requestId++; // 使在途请求的回包失效
		this.currentSourceId = null;
		this.clearNewRowTimer();
		this.logs = [];
		this.newLogIds = new Set();
		this.error = null;
		this.lastUpdatedAt = null;
		this.loading = false;
	}

	setView(view: RuntimeLogView) {
		if (this.view === view) return;
		this.view = view;
		this.skipNextHighlight = true;
		if (this.currentSourceId) this.loadLogs(this.currentSourceId, true);
	}

	refresh() {
		if (this.currentSourceId) this.loadLogs(this.currentSourceId);
	}

	/**
	 * 标出本次新出现的行。首次加载不算「新增」，否则整屏都会播放动画，
	 * 用户将无法分辨哪些是真正刚产生的日志。
	 */
	private markNewRows(
		items: RuntimeLogListItem[],
		knownIds: Set<string>,
		hadRows: boolean
	) {
		if (!hadRows || this.skipNextHighlight) {
			this.skipNextHighlight = false;
			this.clearNewRowTimer();
			this.newLogIds = new Set();
			return;
		}

		const fresh = items.filter((item) => !knownIds.has(item.log_id));
		this.clearNewRowTimer();

		if (fresh.length === 0) {
			this.newLogIds = new Set();
			return;
		}

		this.newLogIds = new Set(fresh.map((item) => item.log_id));
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

export const sourceLogsController = new SourceLogsController();
