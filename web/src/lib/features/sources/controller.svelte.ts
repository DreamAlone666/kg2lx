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
   * 标题保存进行中。
   *
   * 不复用 `refreshing[sourceId]`：那张表是「这个音源正在刷新登录」的公开信号，
   * 列表页也读它来降透明度；把改名写进去会让一次改名在别处显示成「刷新中」，
   * 两个动作的反馈就此串味。
   */
  savingName = $state(false);
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

  /**
   * 修改标题。`name` 传 `null` 表示**恢复默认标题**——这是后端的正式语义，
   * 不能图省事改写成空串：空白串与省略键虽然落到同一分支，但 `null` 是唯一
   * 明确表达「我就是要默认值」的写法。
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
      const res = await adminSession.api.updateSourceName(sourceId, name);

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
