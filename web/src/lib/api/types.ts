export type ProviderKind = 'kugou_lite';

export type AccountStatus = 'active' | 'login_failed';

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

export type AccountEntitlement = {
  product_type: string;
  /** Upstream's active flag at the time of the snapshot. */
  active: boolean;
  begin_at: number | null;
  end_at: number | null;
};

export type AccountSourceItem = {
  source_id: string;
  provider: ProviderKind;
  name: string;
  default_name: string;
  name_advisory_chars: number;
  script_url: string;
  runtime_token_preview: string;
  quality_fallback: boolean;
  created_at: number;
  updated_at: number;
};

export type StepState = 'ready' | 'uncertain' | 'done';

export type RenewalOutcome = 'succeeded' | 'already_done' | 'failed' | 'uncertain' | 'deferred';

export type RenewalReason =
  | 'manual'
  | 't1_low_battery'
  | 't2_expired_refusal'
  | 'confirm_snapshot'
  | 'reconcile_mismatch'
  | 'recover_write'
  | 'post_refresh'
  | 'post_bind';

export type RenewalState =
  | 'ready'
  | 'waiting_for_reconciliation'
  | 'reconciliation_exhausted'
  | 'verification_paused'
  | 'upstream_unavailable'
  | 'rate_limited'
  | 'propagating'
  | 'awaiting_upgrade'
  | 'completed'
  | 'attempts_exhausted';

export type RenewalStatus = {
  enabled: boolean;
  upstream_day: string;
  state: RenewalState;
  claim: StepState;
  upgrade: StepState;
  svip_confirmation_required: boolean;
  paused_until: number | null;
  retry_at: number | null;
  rate_limit_until: number | null;
  propagating_until: number | null;
  estimated_end_at: number | null;
  last_outcome: RenewalOutcome | null;
};

export type AccountItem = EntitlementSnapshot & {
  account_id: string;
  provider: ProviderKind;
  userid: string;
  status: AccountStatus;
  last_refresh_at: number | null;
  last_error: string | null;
  last_error_code: string | null;
  renewal: RenewalStatus;
  sources: AccountSourceItem[];
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

export type RotateTokensResponse = {
  ok: true;
  source_id: string;
  script_url: string;
  runtime_token_preview: string;
  updated_at: number;
};

export type DeleteAccountResponse = {
  ok: true;
  account_id: string;
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
  /** Unix timestamp in milliseconds. */
  created_at_ms: number;
} & (
  | { event_type: 'music_url'; detail: MusicUrlEvent }
  | { event_type: 'token_refresh'; detail: TokenRefreshEvent }
  | { event_type: 'account_login'; detail: AccountLoginEvent }
  | { event_type: 'entitlement'; detail: EntitlementEventDetail }
);

export type EntitlementAction = 'sync' | 'claim' | 'upgrade' | 'reconcile_record';

export type EntitlementEventDetail = {
  action: EntitlementAction;
  reason?: RenewalReason;
  upstream_day?: string;
  outcome?: RenewalOutcome;
  upstream_code?: number | null;
  http_status?: number | null;
  human_verification?: boolean;
  retry_at?: number | null;
  propagating_until?: number | null;
  estimated_end_at?: number | null;
};

export type EventType = SourceEvent['event_type'];

export type StoredEvent = SourceEvent & { seq: number };

export type GlobalLogsResponse = {
  view: LogView;
  event_type: EventType | null;
  account_id: string | null;
  source_id: string | null;
  correlation_id: string | null;
  limit: number;
  has_more: boolean;
  next_before_seq: number | null;
  items: StoredEvent[];
};

export type ApiErrorPayload = {
  code?: string;
  message?: string;
};
