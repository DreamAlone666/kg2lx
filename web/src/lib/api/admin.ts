import { HttpClient } from './http';
import type {
  AccountItem,
  RenewalStatus,
  QrLoginStartResponse,
  QrLoginPollResponse,
  RefreshTokenResponse,
  SyncEntitlementsResponse,
  GlobalLogsResponse,
  EventType,
  LogView,
  UpdateSourceResponse,
  RotateTokensResponse,
  DeleteAccountResponse,
} from './types';

export class AdminApi {
  constructor(private http: HttpClient) {}

  async healthz(signal?: AbortSignal): Promise<{ ok: boolean }> {
    return this.http.get('/healthz', { signal });
  }

  /**
   * 修改音源的可配置字段。**未出现在 `patch` 里的键一律不改**——这是后端 PATCH 的
   * 正式语义，也意味着「不改标题」与「把标题重置为默认」是两条不同的路：
   *
   *   `{ quality_fallback: true }`  只动开关，标题原样保留
   *   `{ name: null }`              重置标题（`null` 是**有意义的值**，不得改写为 `''`）
   *   `{ name: 'x' }`               改标题
   *
   * 因此本方法**逐键判断后组装 body**，而不是把 `patch` 原样丢出去：后者一旦出现
   * `name: undefined` 这类写法，就会在「省略键」与「显式传值」之间制造一个靠
   * JSON.stringify 的丢键行为兜底的静默地带，而这两者在后端是不同语义。
   *
   * body **永远是对象**（哪怕 `{}`）：HttpClient.patch 在 body 为假值时不发 body，
   * 而 axum 的 `Json` 提取器要求请求声明 `Content-Type: application/json`。
   *
   * 净化后的实际落盘值以响应为准回显，不要假定等于提交值。
   */
  async updateSource(
    sourceId: string,
    patch: { name?: string | null; quality_fallback?: boolean },
    signal?: AbortSignal,
  ): Promise<UpdateSourceResponse> {
    const body: { name?: string | null; quality_fallback?: boolean } = {};
    if (patch.name !== undefined) body.name = patch.name;
    if (patch.quality_fallback !== undefined) body.quality_fallback = patch.quality_fallback;
    return this.http.patch(`/api/v1/admin/sources/${encodeURIComponent(sourceId)}`, body, { signal });
  }

  async rotateSourceTokens(sourceId: string, signal?: AbortSignal): Promise<RotateTokensResponse> {
    return this.http.post(
      `/api/v1/admin/sources/${encodeURIComponent(sourceId)}/rotate-tokens`,
      null,
      { signal },
    );
  }

  async startQrLogin(signal?: AbortSignal): Promise<QrLoginStartResponse> {
    return this.http.post('/api/v1/admin/providers/kugou-lite/login/qr', null, { signal });
  }

  async pollQrLogin(sessionId: string, signal?: AbortSignal): Promise<QrLoginPollResponse> {
    return this.http.get(`/api/v1/admin/providers/kugou-lite/login/qr/${encodeURIComponent(sessionId)}`, { signal });
  }

  async refreshToken(accountId: string, signal?: AbortSignal): Promise<RefreshTokenResponse> {
    return this.http.post(`/api/v1/admin/accounts/${encodeURIComponent(accountId)}/refresh-token`, null, { signal });
  }

  async syncEntitlements(accountId: string, signal?: AbortSignal): Promise<SyncEntitlementsResponse> {
    return this.http.post(`/api/v1/admin/accounts/${encodeURIComponent(accountId)}/entitlements/sync`, null, { signal });
  }

  async listAccounts(signal?: AbortSignal): Promise<{ items: AccountItem[] }> {
    return this.http.get('/api/v1/admin/accounts', { signal });
  }

  async getAccount(accountId: string, signal?: AbortSignal): Promise<AccountItem> {
    return this.http.get(`/api/v1/admin/accounts/${encodeURIComponent(accountId)}`, { signal });
  }

  async deleteAccount(accountId: string, signal?: AbortSignal): Promise<DeleteAccountResponse> {
    return this.http.delete(`/api/v1/admin/accounts/${encodeURIComponent(accountId)}`, { signal });
  }

  async updateRenewal(
    accountId: string,
    enabled: boolean,
    signal?: AbortSignal,
  ): Promise<RenewalStatus> {
    return this.http.patch(
      `/api/v1/admin/accounts/${encodeURIComponent(accountId)}/entitlements/renewal`,
      { enabled },
      { signal },
    );
  }

  async claimRenewal(accountId: string, signal?: AbortSignal): Promise<RenewalStatus> {
    return this.http.post(
      `/api/v1/admin/accounts/${encodeURIComponent(accountId)}/entitlements/renewal/claim`,
      null,
      { signal },
    );
  }

  async listLogs(
    query?: {
      limit?: number;
      view?: LogView;
      eventType?: EventType;
      accountId?: string;
      sourceId?: string;
      correlationId?: string;
      beforeSeq?: number;
      afterSeq?: number;
    },
    signal?: AbortSignal,
  ): Promise<GlobalLogsResponse> {
    const params = new URLSearchParams();
    if (query?.limit !== undefined) params.set('limit', query.limit.toString());
    if (query?.view) params.set('view', query.view);
    if (query?.eventType) params.set('event_type', query.eventType);
    if (query?.accountId) params.set('account_id', query.accountId);
    if (query?.sourceId) params.set('source_id', query.sourceId);
    if (query?.correlationId) params.set('correlation_id', query.correlationId);
    if (query?.beforeSeq !== undefined) params.set('before_seq', query.beforeSeq.toString());
    if (query?.afterSeq !== undefined) params.set('after_seq', query.afterSeq.toString());
    const queryString = params.toString();
    return this.http.get(`/api/v1/admin/logs${queryString ? `?${queryString}` : ''}`, {
      signal,
    });
  }
}
