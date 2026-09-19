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
   * `name` 原样透传：`null` 是有意义的值（恢复默认标题），不得改写为 `''` 或省略键。
   * 净化后的实际落盘值以响应里的 `name` 为准。
   */
  async updateSourceName(
    sourceId: string,
    name: string | null,
    signal?: AbortSignal,
  ): Promise<UpdateSourceResponse> {
    return this.http.patch(`/api/v1/admin/sources/${sourceId}`, { name }, { signal });
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
