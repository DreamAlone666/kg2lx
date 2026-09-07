import { adminSession, handleAuthError } from '../../state/admin-session.svelte';
import type { QrLoginStartResponse, QrLoginPollResponse, LoginSessionStatus } from '../../api/types';

class QrLoginController {
  sessionId = $state<string | null>(null);
  status = $state<LoginSessionStatus>('pending');
  qrBase64 = $state<string | null>(null);
  qrUrl = $state<string | null>(null);
  expiresAt = $state<number | null>(null);
  message = $state<string | null>(null);
  
  boundSource = $state<QrLoginPollResponse & { status: 'bound' } | null>(null);

  private pollInterval: ReturnType<typeof setInterval> | null = null;
  private pollInflight = false;
  private abortController: AbortController | null = null;

  async start() {
    this.stop();
    this.abortController = new AbortController();
    try {
      const res = await adminSession.api.startQrLogin(this.abortController.signal);
      this.sessionId = res.session_id;
      this.status = res.status;
      this.qrBase64 = res.qr_base64;
      this.qrUrl = res.qr_url;
      this.expiresAt = res.expires_at;
      this.message = null;
      this.boundSource = null;
      this.beginPolling();
    } catch (e: any) {
      if (e.name === 'AbortError') return;
      if (handleAuthError(e)) return;
      this.status = 'failed';
      this.message = e.message;
    }
  }

  resume(sessionId: string) {
    if (this.sessionId === sessionId) return;
    this.stop();
    this.sessionId = sessionId;
    this.status = 'pending';
    this.beginPolling();
  }

  stop() {
    if (this.pollInterval) {
      clearInterval(this.pollInterval);
      this.pollInterval = null;
    }
    if (this.abortController) {
      this.abortController.abort();
      this.abortController = null;
    }
  }

  /** 清空全部会话状态, 回到未开始的干净界面 */
  reset() {
    this.stop();
    this.sessionId = null;
    this.status = 'pending';
    this.qrBase64 = null;
    this.qrUrl = null;
    this.expiresAt = null;
    this.message = null;
    this.boundSource = null;
  }

  private beginPolling() {
    if (!this.sessionId) return;
    this.pollInterval = setInterval(() => this.poll(), 2000);
  }

  private async poll() {
    if (!this.sessionId || this.pollInflight) return;
    this.pollInflight = true;
    if (!this.abortController) this.abortController = new AbortController();

    try {
      const res = await adminSession.api.pollQrLogin(this.sessionId, this.abortController.signal);
      this.status = res.status;

      if (res.status === 'bound') {
        this.boundSource = res;
        this.stop();
      } else if (res.status === 'expired' || res.status === 'failed') {
        this.message = res.message || '会话已终止';
        this.stop();
      }
    } catch (e: any) {
      if (e.name === 'AbortError') return;
      if (handleAuthError(e)) return;
      if (e.status === 404) {
        this.status = 'failed';
        this.message = '会话未找到';
        this.stop();
      }
    } finally {
      this.pollInflight = false;
    }
  }
}

export const qrLoginController = new QrLoginController();
