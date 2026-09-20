import { HttpClient } from './http';
import type {
  SourceSummary,
  SourceDetail,
  QrLoginStartResponse,
  QrLoginPollResponse,
  RefreshSourceResponse,
  SourceLogsResponse,
  RuntimeLogView,
  UpdateSourceResponse,
} from './types';

export class AdminApi {
  constructor(private http: HttpClient) {}

  async healthz(signal?: AbortSignal): Promise<{ ok: boolean }> {
    return this.http.get('/healthz', { signal });
  }

  async listSources(signal?: AbortSignal): Promise<{ items: SourceSummary[] }> {
    return this.http.get('/api/v1/admin/sources', { signal });
  }

  async getSource(sourceId: string, signal?: AbortSignal): Promise<SourceDetail> {
    return this.http.get(`/api/v1/admin/sources/${sourceId}`, { signal });
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
    return this.http.patch(`/api/v1/admin/sources/${sourceId}`, body, { signal });
  }

  async listSourceLogs(
    sourceId: string,
    query?: { limit?: number; view?: RuntimeLogView },
    signal?: AbortSignal,
  ): Promise<SourceLogsResponse> {
    const params = new URLSearchParams();
    if (query?.limit) params.set('limit', query.limit.toString());
    if (query?.view) params.set('view', query.view);
    const queryString = params.toString();
    return this.http.get(`/api/v1/admin/sources/${sourceId}/logs${queryString ? `?${queryString}` : ''}`, { signal });
  }

  async startQrLogin(signal?: AbortSignal): Promise<QrLoginStartResponse> {
    return this.http.post('/api/v1/admin/providers/kugou-lite/login/qr', null, { signal });
  }

  async pollQrLogin(sessionId: string, signal?: AbortSignal): Promise<QrLoginPollResponse> {
    return this.http.get(`/api/v1/admin/providers/kugou-lite/login/qr/${sessionId}`, { signal });
  }

  async refreshSource(sourceId: string, signal?: AbortSignal): Promise<RefreshSourceResponse> {
    return this.http.post(`/api/v1/admin/sources/${sourceId}/refresh`, null, { signal });
  }
}
