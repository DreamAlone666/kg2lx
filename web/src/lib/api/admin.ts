import { HttpClient } from './http';
import type {
  SourceSummary,
  SourceDetail,
  QrLoginStartResponse,
  QrLoginPollResponse,
  RefreshSourceResponse,
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
