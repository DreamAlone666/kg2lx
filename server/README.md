# 服务端 (Server)

本模块是基于 Rust 开发的后端服务。其核心工作机制如下：

- 管理员触发酷狗概念版（Kugou Lite）的二维码登录流程。
- 服务端负责将登录成功的酷狗账号与生成的一个洛雪（LX Music）自定义音源绑定。
- 服务端为每个绑定的音源提供专属的音源脚本（JS 文件）分发。
- 洛雪音乐客户端加载脚本后，会调用服务端的运行时 API 来获取真实可播放的音乐链接。

## 运行环境要求

- 已安装 Rust 开发工具链。
- 需要有一个正在运行的 `KuGouMusicApi` 上游服务，并且配置环境为 `platform=lite`。

## 环境变量配置

**必填项：**

- `PUBLIC_BASE_URL`：本服务的对外可访问地址。
- `ADMIN_TOKEN`：管理后台接口的鉴权 Token（不能为空或纯空白）。
- `KUGOU_API_BASE_URL`：`KuGouMusicApi` 上游服务的地址。

**选填项：**

- `LISTEN_ADDR`：服务监听地址及端口，默认值为 `127.0.0.1:8787`。
- `DATA_DIR`：数据存储目录，默认值为 `./data`。
- `UPSTREAM_TIMEOUT_MS`：请求上游 API 的超时时间（毫秒），默认值为 `10000`。
- `ACCOUNT_REFRESH_TTL`：账号登录态的保鲜阈值（秒），默认值为 `21600`。**它不是刷新周期：只在收到取链请求时才判断是否需要刷新登录，服务内没有定时任务，没有取链流量就不会打扰上游。** 旧名 `REFRESH_INTERVAL_SECS` 仍可读取，但会打一条弃用提示。
- `EVENT_LOG_LIMIT`：取链日志的保留上限，默认 `10000`，可设为 1–100,000。刷新 Token、账号登录和权益事件分别保留，单类最多取该设置和 1,000 的较小值。
- `WEB_DIST_DIR`：可选，管理后台前端构建产物目录。配置后，服务端将同时提供 API 和前端静态文件服务，未匹配到 API 路由的请求将回退到 `index.html`（SPA 模式）。未配置时仅提供 API 服务。配置后目录不存在将在启动时直接报错退出。

**启动示例：**

```bash
export PUBLIC_BASE_URL="http://127.0.0.1:8787"
export ADMIN_TOKEN="change-me"
export KUGOU_API_BASE_URL="http://127.0.0.1:3000"
export DATA_DIR="./data"
cargo run
```

## 健康检查

你可以通过以下命令检查服务是否正常运行：

```bash
curl http://127.0.0.1:8787/healthz
```

## 管理后台流程 (Admin Flow)

所有管理接口均需在请求头中携带 `Authorization: Bearer <ADMIN_TOKEN>` 进行鉴权。

**1. 发起二维码登录**

```bash
curl -X POST \
  -H "Authorization: Bearer change-me" \
  http://127.0.0.1:8787/api/v1/admin/providers/kugou-lite/login/qr
```

**2. 轮询登录状态**

使用上一步返回的 `session_id` 查询用户扫码状态：

```bash
curl \
  -H "Authorization: Bearer change-me" \
  http://127.0.0.1:8787/api/v1/admin/providers/kugou-lite/login/qr/<session_id>
```

当响应状态变为 `bound` 时，表示账号已成功绑定，请将返回结果中的 `source.script_url` 复制到洛雪音乐中作为自定义音源导入。

**3. 获取所有音源列表**

```bash
curl \
  -H "Authorization: Bearer change-me" \
  http://127.0.0.1:8787/api/v1/admin/sources
```

**4. 查询单个音源信息**

```bash
curl \
  -H "Authorization: Bearer change-me" \
  http://127.0.0.1:8787/api/v1/admin/sources/<source_id>
```

**5. 账号查询、刷新登录与同步权益**

- **查询账号**。`GET /api/v1/admin/accounts` 与 `GET /api/v1/admin/accounts/{account_id}` 返回账号会话、权益快照、自动领取 VIP 调度状态及关联音源列表（`sources` 数组）。
- **刷新 Token**。发送 `POST /api/v1/admin/accounts/{account_id}/refresh-token` 更新登录凭据。
- **同步权益**。发送 `POST /api/v1/admin/accounts/{account_id}/entitlements/sync` 读取上游快照，失败时保留上次成功记录。
- **旧刷新地址**。`POST /api/v1/admin/sources/{source_id}/refresh` 保留原回执，刷新 Token 成功后在后台同步权益。
- **全站事件日志**。`GET /api/v1/admin/logs` 默认包含每日领取与升级事件，支持 `event_type`、`view`、`account_id`、`source_id`、`correlation_id`（按时间正序返回因果链路）、`before_seq` 游标分页及 `after_seq` 增量拉取（仅返回 `seq > after_seq` 的新事件，与 `before_seq` 互斥；若超过 `limit` 仅设置 `has_more=true` 且 `next_before_seq=null`）；事件日志以 `created_at_ms`（Unix 毫秒）记录创建时间，旧 `GET /api/v1/admin/sources/{source_id}/logs` 仍保留。

### 每日领取与升级（自动领取 VIP）

- `PATCH /api/v1/admin/accounts/{account_id}/entitlements/renewal` 接收 `{"enabled":true}` 或 `{"enabled":false}`。开关按账号生效，存量和新绑定账号默认为关闭。
- `POST /api/v1/admin/accounts/{account_id}/entitlements/renewal/claim` 手动领取或继续升级。手动操作在自动领取 VIP 关闭时仍可用，共用每日进度、互斥和退避。响应返回当前调度状态，业务结果见 `last_outcome`。例如，`awaiting_upgrade` 表示已领待升级，`waiting_for_reconciliation` 表示要先核对日记录。
- 音源列表的 `renewal` 和详情的 `account.renewal` 回显调度状态，两个自动领取 VIP 接口也返回该结构。它包含每日领取与升级进度、上游日、是否需要确认 `svip`、退避截止及传播等待。账号、权益快照与自动领取 VIP 调度中的时间戳单位为 Unix 秒，事件日志时间戳字段为 `created_at_ms`（Unix 毫秒）。`estimated_end_at` 是本地推算，确认的到期信息由 `entitlements` 和 `entitlements_synced_at` 提供。
- 日志查询加 `include_renewal=true` 可查看领取、升级与日记录对账。后台自动领取 VIP 沿用触发它的播放请求 ID。业务码 `131001` 和 `297002` 分别作为领取和升级的幂等完成记录。

自动领取在目标档返回可用链接且已确认的 `svip` 剩余不足 48 小时但仍在期时触发。权益确认已过期时，只有目标档明确返回 `status: 2` 才触发。两种情况均由后续后台任务处理，本次取链照常返回链接、执行降档或返回拒绝。升级确认后有约 120 秒传播等待。

写入发出前先保存当天对应步骤的进度。请求结果不明确时，后续播放或手动触发先读取日记录。当天的 `tvip` 确认领取，`svip` 确认两个步骤。超时尝试的单次预算依次为 1 秒、5 秒和 30 秒。缺失记录需等候传播预算后再确认，无法解析或未覆盖所需日期的记录会保留待对账状态。遇到人机验证或结果不明的 HTTP 502 时暂停到次日，502 中对应步骤的幂等码仍确认该步骤完成。限流的指数退避从 60 秒起，最多 1 小时，重启和跨日后继续生效。

权益快照、领取、升级和日记录请求带有 `X-Apicache-Bypass: 1`，以绕过 KuGouMusicApi 按 URL 共享的缓存。

当前按 UTC+8 划分上游日和解释无偏移的 `svip` 时间。切日前 35 秒暂缓写入，跨日收到的结果通过日记录核对。切日、跨日升级归属及实际日记录响应格式与可见延迟仍需上游实测。

## 洛雪运行流程 (LX Runtime Flow)

**获取音源脚本：**

客户端通过以下地址获取专属的 JS 脚本：

```text
GET /s/<script_token>.js
```

**脚本获取音乐播放链接：**

音源脚本在客户端运行时，会向服务端发起请求获取歌曲的真实 URL：

```text
POST /api/v1/runtime/music-url
Authorization: Bearer <runtime_token>
```

**运行时 API 请求示例：**

```bash
curl -X POST \
  -H "Authorization: Bearer <runtime_token>" \
  -H "Content-Type: application/json" \
  -d '{"hash":"<hash>","album_audio_id":null,"quality":"320k"}' \
  http://127.0.0.1:8787/api/v1/runtime/music-url
```

## 数据存储

- **数据库**。状态保存在 `DATA_DIR/kg2lx.db`，请使用服务进程可写的本地数据卷。
- **数据库升级**。启动时依次应用 SQLite 迁移，失败的步骤回滚，高于当前服务支持的库版本会拒绝启动。
- **首次启动**。旧 `accounts/` 和 `sources/` 一次性导入，遇到损坏文件或引用冲突时启动报错并指出文件。
- **历史记录**。旧扫码会话与 JSONL 日志留在原目录，新会话和日志进入 SQLite。
- **备份与回退**。停服务后备份整个数据卷，切回 v0.2.0 时读取切换前的旧文件。
