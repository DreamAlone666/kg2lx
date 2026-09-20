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

    // 2xx 也可能不是接口数据：地址指向静态站点、或命中了服务端的 SPA 回退时，
    // 拿到的是 HTML。此时 json() 抛的 SyntaxError 是引擎的英文解析报错，不能让它
    // 冒到界面上。按本仓既有约定抛「无 code 的 AdminApiError」——describeApiError
    // 正是用「没有 code」识别这一类（正常错误信封必带 code）。
    //
    // 只吞 SyntaxError：fetch 在收到响应头时就 resolve，所以超时（AbortSignal）与
    // 读体中途的连接失败也是在 json() 这一步 reject 的。一并吞掉会把「超时」误报成
    // 「响应体不对」，并让连接页那条 DOMException 分支再也走不到。
    try {
      return await response.json();
    } catch (e) {
      if (!(e instanceof SyntaxError)) throw e;
      throw new AdminApiError(response.status, {});
    }
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

  /**
   * 与 post 同样走 `body ? JSON.stringify(body) : undefined`：传 `null` 或省略
   * body 时既不带 body 也不带 `Content-Type`，而 axum 的 `Json` 提取器要求请求声明
   * `Content-Type: application/json`，会直接拒掉。因此调用方**永远传对象**，
   * 需要表达「无值」时传 `{ name: null }` 而不是 `null`。
   */
  patch<T>(path: string, body?: any, options: RequestInit = {}): Promise<T> {
    return this.request<T>(path, {
      ...options,
      method: 'PATCH',
      body: body ? JSON.stringify(body) : undefined,
    });
  }
}
