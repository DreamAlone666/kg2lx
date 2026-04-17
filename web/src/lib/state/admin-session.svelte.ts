import { HttpClient } from '../api/http';
import { AdminApi } from '../api/admin';
import { getLocal, setLocal, getSession, setSession, clearSession } from '../utils/browser-storage';

class AdminSession {
  serverBaseUrl = $state(getLocal('serverBaseUrl') || '');
  adminToken = $state(getSession('adminToken') || '');
  isValidated = $state(false);

  api = $derived.by(() => {
    const http = new HttpClient({
      baseUrl: this.serverBaseUrl,
      adminToken: this.adminToken,
    });
    return new AdminApi(http);
  });

  constructor() {}

  setSession(baseUrl: string, token: string) {
    this.serverBaseUrl = baseUrl.replace(/\/$/, '');
    this.adminToken = token;
    this.isValidated = true;
    
    setLocal('serverBaseUrl', this.serverBaseUrl);
    setSession('adminToken', this.adminToken);
  }

  logout() {
    this.adminToken = '';
    this.isValidated = false;
    clearSession('adminToken');
  }
}

export const adminSession = new AdminSession();
