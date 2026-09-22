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
  /** 该音源是否开启了「音质自动降级」；默认关闭。详见 QualityFallbackSetting 的说明。 */
  quality_fallback: boolean;
  script_url: string;
  userid: string;
  /**
   * 该源账号的会员权益明细，与详情页同一个结构、同一份来源。
   *
   * 列表此前只带一个聚合布尔（`vip_active`），于是「这个账号还有什么权益」必须点进详情
   * 才看得到；而聚合值本身既回答不了「有哪几项」，也回答不了「哪一项快到期了」。现在列表
   * 直接给事实，命名与否交给 vipProductLabel——与详情页同一张表，两页口径不会分叉。
   *
   * 可选的原因与详情页同一套推理：静态产物与服务端二进制可以分别替换，键不存在时这里
   * 只是不渲染任何权益标记。本页**不**区分「键缺失」与「空数组」——两种情况都不产生
   * 任何断言，没有详情页那种「空态要不要说话」的问题。
   */
  entitlements?: AccountEntitlement[];
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

/**
 * 账号的一条**会员权益明细**（来自 GET /api/v1/sources/{source_id} 的
 * `account.entitlements`）。
 *
 * 这是账号权益的**唯一**口径：各项并列、各自独立到期。这里**不再**有「账号级是否会员」
 * 那种聚合结论——聚合值既冗余（逐条明细已经说全了），又回答不了「有哪几项」和
 * 「哪一项快到期了」。因此界面也不得由其中一条反推账号状态：某一条过期不代表账号失效，
 * 一条都没有也不代表登录态有问题。
 *
 * 字段由服务端原样透传上游：`product_type` 是上游自己的类型码，本类型刻意不把它
 * 收窄成枚举——上游新增一个码不该让前端类型崩塌，中文名缺失时由 vipProductLabel
 * 回退显示内部码（见 lib/domain/labels.ts）。
 */
export type AccountEntitlement = {
  /** 上游权益类型码，实测出现过 svip / tvip / qvip / dvip；原样透传，不做归一化。 */
  product_type: string;
  /** 该条是否**当前**在有效期内；已过期与尚未生效都是 false。 */
  active: boolean;
  /** 生效时刻，秒级 Unix 时间戳；上游未给出时为 null。 */
  begin_at: number | null;
  /** 到期时刻，秒级 Unix 时间戳；上游未给出时为 null（此时无从谈「还有多久」。） */
  end_at: number | null;
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
  /** 该音源是否开启了「音质自动降级」；默认关闭。详见 QualityFallbackSetting 的说明。 */
  quality_fallback: boolean;
  script_url: string;
  runtime_token_preview: string;
  account: {
    userid: string;
    status: AccountStatus;
    last_refresh_at: number | null;
    last_error_code: string | null;
    last_error: string | null;
    /**
     * 账号的会员权益明细。**可选字段**，且「键不存在」与「存在但为空数组」是两件事，
     * 消费方必须分开处理：
     *
     * - 键不存在 = 服务端没有提供这条信息。这是真实会发生的版本错配：服务端用
     *   `WEB_DIST_DIR` 直接服务 web/build 里的静态产物（server/src/routes/static_fs.rs），
     *   二进制与前端产物是两个可以分别替换的东西，缺了它不该被读成「账号没有权益」。
     * - 存在但为空数组 = 服务端明确说这个账号一条权益都没有，这是可以如实陈述的事实。
     *
     * 所以**不要用 `?? []` 把它折叠成空数组**：那等于把「不知道」渲染成「没有」。
     * 与本文件其它字段同一条规矩——界面文案不得断言字段本身没说的东西。
     */
    entitlements?: AccountEntitlement[];
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

export type RefreshSourceResponse = {
  ok: true;
  source_id: string;
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
   * **本次**这一行向上游要的 LX 档位（取值域同 `requested_quality`）。
   *
   * 只在发生了降级时与 `requested_quality` 不同：用户要 `flac` 被拒后我们改要
   * `320k`，那一行的 `requested_quality` 仍是 `flac`（用户当初要的就是它），
   * `attempted_quality` 才是 `320k`。两者相等时它没有额外信息，因此界面只在
   * 不相等时才展示（见 LogsPanel 的 qualityTitle）。
   *
   * 由服务端落盘而非前端推导：它是「那一刻发生过什么」的事实，前端不持有阶梯。
   */
  attempted_quality: string | null;
  /**
   * 这一行是阶梯上的第几档：0 = 用户请求的那一档，1.. = 已经向下改了几档。
   *
   * 它与 `requested_quality` 的组合才是完整的：`quality_fallback_index > 0` 说明
   * 这一行不是用户当初要的那次尝试。**一次降级后成功的播放会写两行**——被拒的
   * 那一档 `ok=false`、成功的那一档 `ok=true`，两行靠 `client_request_id` 关联。
   * 所以 `ok=false` 且 index>0 的行不该被读成「一次独立的失败」。
   *
   * 非 `Option`：本次改动前落盘的行读出来是 0（后端有 `#[serde(default)]`），
   * 恰好就是「没降过级」这个正确含义。
   */
  quality_fallback_index: number;
  /**
   * 本行所描述的那次上游调用实际使用的酷狗质量参数（128 / 320 / flac / high）。
   *
   * 只有描述 song/url 那次调用的行才有值：刷新登录 / DFID 两次调用不带质量参数，
   * 服务端据此不记。它是「发生过的事实」而不是可推导的常量，所以由服务端落盘、
   * 前端只读——服务端在写日志时填入（server/src/services/runtime.rs 的 append_log，
   * 映射来自 kugou_lite_client 的 map_quality）。
   *
   * **它是 `attempted_quality` 那一档映射出来的**，不是用户请求档位映射出来的：
   * 降级之后的每一行都应该记自己那一档的参数值。若按用户请求档位算，阶梯上每一行
   * 会记成同一个值，这一列恰恰在最该看出阶梯的地方看不出任何东西。
   *
   * 本次改动之前落盘的行没有这个键，读出来是 null（后端有 #[serde(default)]）。
   */
  upstream_quality: string | null;
  /**
   * 上游自己给的业务状态：1 = 成功，2 = **明确拒绝**这一次取链，0 = 资源不存在或
   * 参数非法（后两者形状完全不同，0 会另带 `error_code`）。
   *
   * 是上游的原话，不是我们的判断结果——「我们怎么处理它」（降级 / 终局）由
   * `quality_fallback_index` 与 `error_code` 分别承载。
   *
   * 只有真的调用过上游的行才有值：预检 / ensure_dfid / refresh_login 不经过
   * song/url，读出来是 null。
   */
  upstream_status: number | null;
  /** 上游同一响应里的 priv_status。与 `upstream_status` 同源、同为空。 */
  upstream_priv_status: number | null;
  /**
   * 上游响应里的 `fail_process` 数组，**原样记录的事实，永远不作判据**。
   *
   * 实测它极不稳定：同一账号同一小时内对同一首歌见过 `["pkg","buy"]`、`["buy"]`
   * 和 `[]` 三种取值。任何「按 fail_process 推断原因」的分支都会在下次请求上失效，
   * 因此界面只呈现它，不解释它、更不据它下结论。
   *
   * 服务端落盘前会截断长度，避免每一行 jsonl 被这个数组撑大。
   */
  upstream_fail_process: string[] | null;
  /** 上游实际返回的音频格式（`mp3` / `flac` / `mkv`）。 */
  upstream_ext_name: string | null;
  /**
   * 上游实际返回的字节数。
   *
   * 与 `upstream_ext_name` 一起回答一个此前无从回答的问题：**我们拿到的是不是
   * 想要的那一档**。上游对没有该声部的歌会静默回落（例如要 `high` 却回 128k mp3，
   * `status` 仍是 1），只有这两个字段的取值能看出来。
   */
  upstream_file_size: number | null;
  /**
   * 上游自报的码率（bps）。**两个无损档恒为 0**（实测 flac / high 都是 `0`，
   * 而 mp3 两档是精确的 `128000` / `320000`），所以 0 是「上游没报这个值」，
   * 不是「0 kbps」——展示时要用 `upstream_file_size × 8 ÷ upstream_time_length`
   * 回退（见 lib/domain/format.ts 的 effectiveBitRate）。
   *
   * 与 `upstream_file_size` 一样是「上次观测」而不是「这次请求」：它由服务端在
   * 写那一行日志时从上游响应里摘下来，前端只读、不推导。
   */
  upstream_bit_rate: number | null;
  /**
   * 上游返回的音频时长（秒）。单独看没有诊断价值，它存在的唯一理由是给上面那条
   * 回退公式当除数：`file_size × 8 ÷ time_length` 实测四档全有值且分离度足够
   * （约 128 / 321 / 1038 / 1747 kbps）。
   *
   * 这个字段本轮才落盘，因此更早的行读出来是 null，回退也就退不了——那时只显示格式。
   */
  upstream_time_length: number | null;
  /**
   * 本次入站 LX 请求的关联 id：一次请求一个 id，同一请求的多次尝试共用它。
   *
   * 用途是把「同一次请求的第 N 档」与「两次独立请求」区分开。上文两处说的
   * 「降级后的两行」就是靠它配对的——只看时间戳与 hash 分不清是哪种。
   *
   * 在查到音源之后立刻生成，因此预检失败的行也带同一个 id。
   * 本次改动前落盘的行没有这个键，读出来是 null。
   */
  client_request_id: string | null;
  /**
   * LX 客户端上报的歌曲元数据（`musicInfo.types`，即宿主认为这首歌有哪些档位、
   * 各自的 hash 与大小），**原样落盘、不参与任何判断**。
   *
   * 类型故意留成 `unknown`：它的形状由冻结在用户机器上的脚本决定，前端既无法约束
   * 也不该校验。它是一个**记录**——出问题时用来对照「客户端以为有什么」与「上游
   * 实际给了什么」，而不是我们据以做分支的输入。
   *
   * 本次改动前落盘的脚本不上报它，读出来是 `null`——类型写成 `unknown` 是因为**形状**
   * 不可约束，不是因为它一定有值，消费方仍要判空。
   */
  reported_types: unknown;
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
