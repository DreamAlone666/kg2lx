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
  /** 当前生效的标题：恢复默认后它等于 `default_name`，不会为空。 */
  name: string;
  /**
   * 后端按**当前配置**算出来的默认标题（`SOURCE_NAME_PREFIX + userid`）。
   *
   * **不要拿它当「用户是否改过名」的判据**：前缀是环境变量，运维改一次，所有旧音源
   * 的 `name` 就都与它对不上了。实测有一条音源建于前缀还是「酷狗概念版」时、而部署
   * 早已改成别的值——没人改过名，`name !== default_name` 却成立。任何据它做的分支
   * 都会在换过前缀的部署上误判。
   *
   * 列表当前不消费它，保留是因为本类型如实镜像接口返回。
   */
  default_name: string;
  enabled: boolean;
  script_url: string;
  userid: string;
  vip_active: boolean;
  /**
   * 账号状态。空串表示账号记录已不存在（后端不给未知状态造新枚举值），
   * 由 labelOf / accountStatusTone 回退为「未知」与 neutral。
   */
  account_status: AccountStatus | '';
  /** 账号最近一次刷新成功的时刻；从未刷新过则为 null。 */
  last_refresh_at: number | null;
  /** 源记录的更新时间，仅在绑定/改配置时变动，界面不再展示。 */
  updated_at: number;
};

export type SourceDetail = {
  source_id: string;
  account_id: string;
  provider: ProviderKind;
  /** 当前生效的标题：恢复默认后它等于 `default_name`，不会为空。 */
  name: string;
  /** 后端按当前配置算出的默认标题；只用于在编辑框留空时告诉用户「存下去会变成什么」。 */
  default_name: string;
  /**
   * 标题的**建议**字符数上限（当前 24），只用来在编辑框旁边显示计数器。
   *
   * 它不构成校验：超出建议值不会导致 400（后端拒绝的超长阈值是一条 256 字节的
   * 技术护栏，与这个数字无关），渲染路径也只做净化、从不截断。前端因此不得用它
   * 拦截保存——本仓库已有过「界面文案比字段本身语义更强」的记录，若把建议值写成
   * 硬限制，用户会以为超长标题存不进去，而实际上存得进去。
   */
  name_advisory_chars: number;
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

export type UpdateSourceResponse = {
  ok: true;
  source_id: string;
  /** 净化后**实际落盘**的标题，可能与提交值不同，界面应以它为准回显。 */
  name: string;
  default_name: string;
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

export type ApiErrorPayload = {
  code?: string;
  message?: string;
};

export type RuntimeLogView = 'all' | 'errors';

export type RuntimeLogListItem = {
  log_id: string;
  source_id: string;
  account_id: string;
  provider: 'kugou_lite';
  action: 'musicUrl';
  track_title: string | null;
  artist_name: string | null;
  album_name: string | null;
  request_hash: string;
  album_audio_id: string | null;
  requested_quality: string;
  /**
   * 本行所描述的那次上游调用实际使用的酷狗质量参数（128 / 320 / flac / high）。
   *
   * 只有描述 song/url 那次调用的行才有值：刷新登录 / DFID 两次调用不带质量参数，
   * 服务端据此不记。它是「发生过的事实」而不是可推导的常量，所以由服务端落盘、
   * 前端只读——服务端在写日志时填入（server/src/services/runtime.rs 的 append_log，
   * 映射来自 kugou_lite_client 的 map_quality）。
   *
   * 本次改动之前落盘的行没有这个键，读出来是 null（后端有 #[serde(default)]）。
   */
  upstream_quality: string | null;
  upstream_endpoint: string;
  stage: 'precheck' | 'ensure_dfid' | 'refresh_login' | 'fetch_music_url';
  refresh_attempted: boolean;
  retry_count: number;
  ok: boolean;
  status_code: number | null;
  error_code: string | null;
  latency_ms: number;
  error: string | null;
  created_at: number;
};

export type SourceLogsResponse = {
  source_id: string;
  view: RuntimeLogView;
  limit: number;
  items: RuntimeLogListItem[];
};
