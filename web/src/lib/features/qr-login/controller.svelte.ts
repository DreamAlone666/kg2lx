import { adminSession } from '../../state/admin-session.svelte';
import type { QrLoginStartResponse, QrLoginPollResponse, LoginSessionStatus } from '../../api/types';

class QrLoginController {
  sessionId = $state<string | null>(null);
  status = $state<LoginSessionStatus>('pending');
  qrBase64 = $state<string | null>(null);
  qrUrl = $state<string | null>(null);
  expiresAt = $state<number | null>(null);
  message = $state<string | null>(null);
  
  boundSource = $state<QrLoginPollResponse & { status: 'bound' } | null>(null);

  private pollInterval: any = null;
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

  private beginPolling() {
    if (!this.sessionId) return;
    
    this.pollInterval = setInterval(async () => {
      if (!this.sessionId) return;
      if (!this.abortController) this.abortController = new AbortController();
      
      try {
        const res = await adminSession.api.pollQrLogin(this.sessionId, this.abortController.signal);
        this.status = res.status;
        
        if (res.status === 'bound') {
          this.boundSource = res;
          this.stop();
        } else if (res.status === 'expired' || res.status === 'failed') {
          this.message = (res as any).message || '会话已终止';
          this.stop();
        }
      } catch (e: any) {
        if (e.name === 'AbortError') return;
        if (e.status === 404) {
          this.status = 'failed';
          this.message = '会话未找到';
          this.stop();
        }
      }
    }, 2000);
  }
}

export const qrLoginController = new QrLoginController();
