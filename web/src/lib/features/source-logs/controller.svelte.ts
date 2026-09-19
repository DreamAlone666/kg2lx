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
	/**
	 * 用户主动暂停了自动刷新。
	 *
	 * 与「页面隐藏」分开记录：页面隐藏是自动的、用户看不见的；主动暂停必须在
	 * 界面上明示，否则用户会以为数据还在实时更新（而它已经停了）。
	 * 手动「刷新」按钮不受此影响——用户按了就该拉一次。
	 */
	paused = $state(false);

	private requestId = 0;
	private currentSourceId: string | null = null;
	private newRowTimer: ReturnType<typeof setTimeout> | null = null;
	/** 视图切换会整表替换，不应让整屏都播放「新增」动画，故跳过下一次标记。 */
	private skipNextHighlight = false;
	/**
	 * 在途请求的序号集合。只用于轮询的**在途去重**（见 timer 的 onTick）。
	 *
	 * 用集合而不是布尔量：请求可能乱序返回，集合按序号精确增减，不受完成顺序影响。
	 */
	private pending = new Set<number>();

	private timer = createVisibilityAwareTimer({
		intervalMs: POLL_INTERVAL_MS,
		onTick: () => {
			if (!this.currentSourceId) return;
			// 在途去重：后端比轮询间隔还慢时，每个回包到达时都会被更新的 requestId
			// 判为过期而丢弃，表格于是**永远不更新**——实测后端延迟 6s、间隔 5s 时，
			// 14s 过后表格仍是 0 行（期间后端已回过两次）；把这一行注释掉再跑同一条件，
			// 立刻恢复成 20 行。同一时刻只放一个轮询请求在途，慢的时候就退化成
			// 「拉到一次更新一次」，而不是「一次都不更新」。
			if (this.pending.size > 0) return;
			this.loadLogs(this.currentSourceId, true);
		}
	});

	async loadLogs(sourceId: string, silent = false) {
		const requestId = ++this.requestId;
		this.pending.add(requestId);
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
			this.pending.delete(requestId);
			// 清 loading 刻意**不**再要求 `requestId === this.requestId`。
			//
			// 那条件曾让 loading 永久卡死，已实测复现：手动「刷新」发出的是非静默请求，
			// 只要它还没回来时轮询 tick（或一次视图切换）插进来，requestId 就被顶掉；
			// 此后该请求的 finally 因条件不成立而跳过，而插进来的那一个是静默的、
			// 根本不动 loading——结果是徽章永远停在「刷新中」，刷新按钮永远 disabled，
			// 且界面上没有任何路径能恢复（只能离开页面）。实测点击刷新后 18s、请求延迟
			// 只有 8s，状态仍是 badge=刷新中 / disabled=true。
			//
			// loading 的语义是「用户看得见的加载中」，它只由非静默请求拥有，所以也只该
			// 由非静默请求来清——与它是否最新无关。同一时刻不会有两个非静默请求重叠：
			// 设置 loading 后刷新按钮立即 disabled（Double click 分属两个任务，中间微任务
			// 队列已刷新，第二次不会触发）。
			if (!silent) this.loading = false;
		}
	}

	startPolling(sourceId: string) {
		this.stopPolling();
		this.currentSourceId = sourceId;
		// 首屏这次拉取显式发出且**非静默**：它是「加载中」而不是「后台刷新」，必须让
		// 骨架屏亮起来。此前它和轮询共用同一条静默路径，于是首屏整整一段时间在说
		// 「暂无运行时流量记录，播放音乐后这里会出现取链日志」——一个后端还没回话就
		// 被说出口的结论（骨架屏分支在首挂载时因此根本走不到）。
		//
		// timer.start() 紧接着也会立即 tick 一次，但会被上面的在途去重挡掉：loadLogs
		// 在第一个 await 之前就同步登记了 pending，所以那次 tick 必然看到 pending 非空。
		void this.loadLogs(sourceId);
		this.timer.start();
	}

	stopPolling() {
		this.timer.stop();
		this.requestId++; // 使在途请求的回包失效
		this.currentSourceId = null;
		this.pending.clear();
		this.clearNewRowTimer();
		this.logs = [];
		this.newLogIds = new Set();
		this.error = null;
		this.lastUpdatedAt = null;
		this.paused = false;
		this.loading = false;
	}

	setView(view: RuntimeLogView) {
		if (this.view === view) return;
		this.view = view;
		this.skipNextHighlight = true;
		if (this.currentSourceId) this.loadLogs(this.currentSourceId, true);
	}

	setPaused(paused: boolean) {
		if (this.paused === paused) return;
		this.paused = paused;

		if (paused) {
			this.timer.pause();
			return;
		}

		// 暂停期间可能已经累积了多行，一次性把 20 行全部标成「新增」是噪音而不是信号；
		// 增量高亮只留给轮询自然追加的场景（与首屏不标记的理由同源）。
		this.skipNextHighlight = true;
		this.timer.resume();
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
