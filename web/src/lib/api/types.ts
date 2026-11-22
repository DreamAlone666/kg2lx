export type ProviderKind = 'kugou_lite';

export type AccountStatus = 'active' | 'pending' | 'disabled' | 'login_failed';

export type LoginSessionStatus =
  | 'pending'
  | 'waiting_scan'
  | 'waiting_confirm'
  | 'authorized'
  | 'bound'
  | 'expired'
  | 'failed';

export type EntitlementSnapshot = {
  /** Omitted until the first confirmed snapshot. An empty array is a recorded result. */
  entitlements?: AccountEntitlement[];
  entitlements_synced_at: number | null;
  entitlements_sync_failed: boolean;
};

export type SourceSummary = EntitlementSnapshot & {
  source_id: string;
  account_id: string;
  provider: ProviderKind;
  name: string;
  default_name: string;
  quality_fallback: boolean;
  script_url: string;
  userid: string;
  account_status: AccountStatus | '';
  last_refresh_at: number | null;
  updated_at: number;
};

export type AccountEntitlement = {
  product_type: string;
  /** Upstream's active flag at the time of the snapshot. */
  active: boolean;
  begin_at: number | null;
  end_at: number | null;
};

export type SourceDetail = {
  source_id: string;
  account_id: string;
  provider: ProviderKind;
  script_url: string;
  name: string;
  default_name: string;
  name_advisory_chars: number;
  quality_fallback: boolean;
  runtime_token_preview: string;
  account: EntitlementSnapshot & {
    userid: string;
    status: AccountStatus;
    last_refresh_at: number | null;
    last_error_code: string | null;
    last_error: string | null;
  };
};

export type UpdateSourceResponse = {
  ok: true;
  source_id: string;
  /** 净化后**实际落盘**的标题，可能与提交值不同，界面应以它为准回显。 */
  name: string;
  default_name: string;
  /**
   * 落盘后实际生效的开关值。
   *
   * PATCH 是「缺失 = 不改」的语义，所以回显这个字段不只是方便：调用方要判断
   * 「刚才那次只带 name 的提交有没有意外把开关改掉」，只能看这里的值。
   */
  quality_fallback: boolean;
  updated_at: number;
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

export type RefreshTokenResponse = {
  ok: true;
  updated_at: number;
};

export type SyncEntitlementsResponse = {
  ok: true;
  synced_at: number;
};

export type LogView = 'all' | 'errors';

export type MusicUrlEvent = {
  stage: 'precheck' | 'fetch_music_url';
  track_title: string | null;
  artist_name: string | null;
  album_name: string | null;
  request_hash: string;
  album_audio_id: string | null;
  requested_quality: string;
  attempted_quality: string | null;
  quality_fallback_index: number;
  retry_count: number;
  reported_types: unknown;
  upstream_endpoint: string | null;
  status_code: number | null;
  human_verification: boolean;
  upstream_quality: string | null;
  upstream_status: number | null;
  upstream_priv_status: number | null;
  upstream_fail_process: string[] | null;
  upstream_ext_name: string | null;
  upstream_file_size: number | null;
  upstream_bit_rate: number | null;
  upstream_time_length: number | null;
};

export type TokenRefreshEvent = {
  reason: 'ttl' | 'rejected' | 'device' | 'manual' | 'qr_login';
  upstream_endpoint: string;
  human_verification: boolean;
};

export type AccountLoginEvent = { outcome: 'bound' | 'rebound' | 'expired' | 'failed' };

export type SourceEvent = {
  log_id: string;
  source_id: string | null;
  account_id: string | null;
  correlation_id: string;
  trigger: 'runtime' | 'admin' | 'qr';
  ok: boolean;
  latency_ms: number | null;
  error_code: string | null;
  created_at: number;
} & (
  | { event_type: 'music_url'; detail: MusicUrlEvent }
  | { event_type: 'token_refresh'; detail: TokenRefreshEvent }
  | { event_type: 'account_login'; detail: AccountLoginEvent }
  | { event_type: 'entitlement'; detail: { action: 'sync' } }
);

export type EventType = SourceEvent['event_type'];

export type SourceLogsResponse = {
  source_id: string;
  view: LogView;
  event_type: EventType | null;
  limit: number;
  items: SourceEvent[];
};
export type ApiErrorPayload = {
  code?: string;
  message?: string;
};
