import { goto } from '$app/navigation';
import { HttpClient } from '../api/http';
import { AdminApi } from '../api/admin';
import { AdminApiError } from '../api/errors';
import { getLocal, setLocal, getSession, setSession, clearSession } from '../utils/browser-storage';

class AdminSession {
  serverBaseUrl = $state(getLocal('serverBaseUrl') || '');
  adminToken = $state(getSession('adminToken') || '');

  api = $derived.by(() => {
    const http = new HttpClient({
      baseUrl: this.serverBaseUrl,
      adminToken: this.adminToken,
    });
    return new AdminApi(http);
  });

  setSession(baseUrl: string, token: string) {
    this.serverBaseUrl = baseUrl.replace(/\/$/, '');
    this.adminToken = token;

    setLocal('serverBaseUrl', this.serverBaseUrl);
    setSession('adminToken', this.adminToken);
  }

  logout() {
    this.adminToken = '';
    clearSession('adminToken');
  }
}

export const adminSession = new AdminSession();

/**
 * 401 统一收口: 清空会话并回到连接页。
 * 各 controller 的 catch 中优先调用。
 * @returns 该错误是否已被处理
 */
export function handleAuthError(error: unknown): boolean {
  if (error instanceof AdminApiError && error.status === 401) {
    adminSession.logout();
    goto('/connect');
    return true;
  }
  return false;
}
