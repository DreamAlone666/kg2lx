import { adminSession, handleAuthError } from '../../state/admin-session.svelte';
import type { SourceSummary, SourceDetail } from '../../api/types';
import { describeApiError } from '../../domain';

export type AccountAction = 'refresh' | 'sync';

class SourcesController {
  sources = $state<SourceSummary[]>([]);
  currentSource = $state<SourceDetail | null>(null);
  loading = $state(false);
  error = $state<string | null>(null);
  accountActions = $state<Partial<Record<string, AccountAction>>>({});
  savingName = $state(false);
  savingQualityFallback = $state(false);
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

  async runAccountAction(sourceId: string, accountId: string, action: AccountAction) {
    if (this.accountActions[accountId]) return { status: 'busy' } as const;
    this.accountActions[accountId] = action;
    try {
      let actionError: string | undefined;
      try {
        if (action === 'refresh') await adminSession.api.refreshToken(accountId);
        else await adminSession.api.syncEntitlements(accountId);
      } catch (error) {
        if (handleAuthError(error)) return { status: 'redirected' } as const;
        actionError = describeApiError(error);
      }

      // Failures are persisted too. Reload just the account projection so a slow action cannot
      // overwrite a title or quality preference saved while it was running.
      try {
        const { account } = await adminSession.api.getSource(sourceId);
        if (this.currentSource?.account_id === accountId) this.currentSource.account = account;
        this.sources = this.sources.map((source) =>
          source.account_id === accountId ? {
            ...source,
            account_status: account.status,
            last_refresh_at: account.last_refresh_at,
            entitlements: account.entitlements,
            entitlements_synced_at: account.entitlements_synced_at,
            entitlements_sync_failed: account.entitlements_sync_failed,
          } : source
        );
      } catch (error) {
        if (handleAuthError(error)) return { status: 'redirected' } as const;
        if (!actionError) return { status: 'unconfirmed' } as const;
      }
      return actionError
        ? { status: 'failed', message: actionError } as const
        : { status: 'ok' } as const;
    } finally {
      delete this.accountActions[accountId];
    }
  }

  /**
   * 修改标题。`name` 传 `null` 表示**恢复默认标题**——这是后端的正式语义，
   * 不能图省事改写成空串或干脆省略这个键：空白串与 `null` 落到同一分支（都是
   * 「要默认值」），而**省略键表示「本次不动标题」**，是另一回事。详见
   * api/admin.ts 的 updateSource。
   *
   * 成功后**就地打补丁**而不是重新 loadSource/loadSources：多一次往返会让标题
   * 在用户正盯着它的那一刻先跳回旧值再闪一下新值，而这正是本仓库
   * 「反馈必须落在动作旁边」要避免的观感——标题本身就是这次动作的反馈。
   *
   * 返回值刻意只有两态、没有 'unauthorized'：未授权由 handleAuthError 直接登出并跳转，
   * 此时 this.error 保持为 null，页面据此保持安静；再加第三态反而要多一条永远走不到的分支。
   */
  async renameSource(sourceId: string, name: string | null): Promise<'ok' | 'failed'> {
    this.savingName = true;
    this.error = null;
    try {
      const res = await adminSession.api.updateSource(sourceId, { name });

      // 只在详情仍停留在同一个音源时回写：保存期间用户可能已经切到别的音源，
      // 不校验会把 A 的新标题盖到 B 的页头上。
      if (this.currentSource?.source_id === sourceId) {
        this.currentSource = {
          ...this.currentSource,
          name: res.name,
          default_name: res.default_name,
        };
      }

      this.sources = this.sources.map((s) =>
        s.source_id === sourceId
          ? { ...s, name: res.name, default_name: res.default_name, updated_at: res.updated_at }
          : s
      );
      return 'ok';
    } catch (e: any) {
      if (!handleAuthError(e)) this.error = describeApiError(e);
      return 'failed';
    } finally {
      this.savingName = false;
    }
  }

  /**
   * 切换「音质自动降级」。
   *
   * 与 renameSource 同一套做法——包括**就地回写**而不是重新 loadSource/loadSources：
   * 开关自身就是这次动作的反馈，多一次往返会让它在用户正盯着的那一刻先跳回旧值
   * 再闪一下新值。
   *
   * 开关**不做乐观更新**：只有服务端确认落盘后才改 this.currentSource，失败时开关
   * 保持原值。理由是这个开关会持久改变播放与下载的行为，让界面显示一个并未生效的
   * 状态，比让它慢半拍危险得多。
   *
   * 列表卡片也读这个字段，所以 sources 里的同一条必须一并回写，否则列表与详情
   * 会各说各话——用户从详情返回列表会看到开关「自己变回去了」。
   */
  async setQualityFallback(sourceId: string, value: boolean): Promise<'ok' | 'failed'> {
    this.savingQualityFallback = true;
    this.error = null;
    try {
      const res = await adminSession.api.updateSource(sourceId, { quality_fallback: value });

      // 与改名同一处守卫：保存期间用户可能已经切到别的音源，
      // 不校验会把 A 的开关状态盖到 B 的页面上。
      if (this.currentSource?.source_id === sourceId) {
        this.currentSource = { ...this.currentSource, quality_fallback: res.quality_fallback };
      }

      this.sources = this.sources.map((s) =>
        s.source_id === sourceId
          ? { ...s, quality_fallback: res.quality_fallback, updated_at: res.updated_at }
          : s
      );
      return 'ok';
    } catch (e: any) {
      if (!handleAuthError(e)) this.error = describeApiError(e);
      return 'failed';
    } finally {
      this.savingQualityFallback = false;
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
