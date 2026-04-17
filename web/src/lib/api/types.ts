export type ProviderKind = 'kugou_lite';

export type AccountStatus = 'active' | 'pending' | 'expired' | 'disabled' | 'login_failed';

export type LoginSessionStatus =
  | 'pending'
  | 'waiting_scan'
  | 'waiting_confirm'
  | 'authorized'
  | 'bound'
  | 'expired'
  | 'failed';

export type SourceSummary = {
  source_id: string;
  account_id: string;
  provider: ProviderKind;
  enabled: boolean;
  script_url: string;
  userid: string;
  vip_active: boolean;
  updated_at: number;
};

export type SourceDetail = {
  source_id: string;
  account_id: string;
  provider: ProviderKind;
  enabled: boolean;
  script_url: string;
  runtime_token_preview: string;
  account: {
    userid: string;
    vip_active: boolean;
    vip_type: number;
    status: AccountStatus;
    last_refresh_at: number | null;
    last_error: string | null;
  };
};

export type QrLoginStartResponse = {
  session_id: string;
  status: Extract<LoginSessionStatus, 'waiting_scan'>;
  qr_url: string;
  qr_base64: string | null;
  expires_at: number;
};

export type QrLoginPollResponse =
  | {
      session_id: string;
      status: 'pending' | 'waiting_scan' | 'waiting_confirm' | 'authorized';
    }
  | {
      session_id: string;
      status: 'bound';
      account: {
        account_id: string;
        userid: string;
        vip_active: boolean;
        vip_type: number;
      };
      source: {
        source_id: string;
        name: string;
        script_url: string;
      };
    }
  | {
      session_id: string;
      status: 'expired' | 'failed';
      message?: string;
    };

export type RefreshSourceResponse = {
  ok: true;
  source_id: string;
  vip_active: boolean;
  updated_at: number;
};

export type SessionConfig = {
  serverBaseUrl: string;
  adminToken: string;
};

export type ApiErrorPayload = {
  code?: string;
  message?: string;
};
