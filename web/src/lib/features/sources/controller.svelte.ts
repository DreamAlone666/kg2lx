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
   * 音质降级开关保存进行中。
   *
   * 与 `savingName` 分开，理由和当初不把它并进 `refreshing[sourceId]` 完全一样：
   * `savingName` 是**改名**这个动作的公开信号，详情页的「刷新 Token」按钮与
   * SourceTitle 的「保存」按钮都直接读它（前者据此禁用、后者据此显示 pending）。
   * 共用会让一次拨开关把那个按钮点亮成「保存中…」，并在开关自己毫无提示的情况下
   * 把刷新按钮锁住——两个动作的反馈再次串味。
   */
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
