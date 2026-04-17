import { AdminApiError } from './errors';
import type { ApiErrorPayload } from './types';

export interface HttpClientOptions {
  baseUrl: string;
  adminToken?: string;
  signal?: AbortSignal;
}

export class HttpClient {
  private baseUrl: string;
  private adminToken?: string;
  private signal?: AbortSignal;

  constructor(options: HttpClientOptions) {
    this.baseUrl = options.baseUrl.replace(/\/$/, '');
    this.adminToken = options.adminToken;
    this.signal = options.signal;
  }

  private async request<T>(path: string, options: RequestInit = {}): Promise<T> {
    const url = `${this.baseUrl}${path}`;
    const headers = new Headers(options.headers);

    if (this.adminToken && !path.startsWith('/healthz')) {
      headers.set('Authorization', `Bearer ${this.adminToken}`);
    }

    if (options.body && !(options.body instanceof FormData)) {
      headers.set('Content-Type', 'application/json');
    }

    const response = await fetch(url, {
      ...options,
      headers,
      signal: options.signal || this.signal,
    });

    if (!response.ok) {
      let payload: ApiErrorPayload = {};
      try {
        payload = await response.json();
      } catch {
        payload = { message: response.statusText };
      }
      throw new AdminApiError(response.status, payload);
    }

    if (response.status === 204) {
      return {} as T;
    }

    return response.json();
  }

  get<T>(path: string, options: RequestInit = {}): Promise<T> {
    return this.request<T>(path, { ...options, method: 'GET' });
  }

  post<T>(path: string, body?: any, options: RequestInit = {}): Promise<T> {
    return this.request<T>(path, {
      ...options,
      method: 'POST',
      body: body ? JSON.stringify(body) : undefined,
    });
  }
}
