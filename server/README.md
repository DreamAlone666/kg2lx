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
- `ADMIN_TOKEN`：管理后台接口的鉴权 Token，启动时去除首尾空白并校验剩余内容非空。
- `KUGOU_API_BASE_URL`：`KuGouMusicApi` 上游服务的地址。

**选填项：**

- `LISTEN_ADDR`：服务监听地址及端口，默认值为 `127.0.0.1:8787`。
- `DATA_DIR`：数据存储目录，默认值为 `./data`。
- `UPSTREAM_TIMEOUT_MS`：请求上游 API 的超时时间（毫秒），默认值为 `10000`。
- `ACCOUNT_REFRESH_TTL`：账号登录态的保鲜阈值，默认 `21600` 秒。取链时按需判断是否需要刷新登录。
- `EVENT_LOG_LIMIT`：取链日志的保留上限，默认 `10000`，可设为 1–100,000。刷新 Token、账号登录和权益事件分别保留，单类最多取该设置和 1,000 的较小值。
- `WEB_DIST_DIR`：管理后台构建产物目录。设置后，服务端同源托管 API 与前端，单页应用导航可回退到 `index.html`。缺失的 API、脚本及静态资源地址返回 `404`。目录在启动时校验，留空时仅启用 API 和脚本分发。

**启动示例：**

```bash
export PUBLIC_BASE_URL="http://127.0.0.1:8787"
export ADMIN_TOKEN="change-me"
export KUGOU_API_BASE_URL="http://127.0.0.1:3000"
export DATA_DIR="./data"
cargo run
```

## 健康检查

`GET /healthz` 检查服务与 SQLite 连接。数据库探活失败时返回 `503`。

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

**3. 查询和维护账号**

- **查询账号**。`GET /api/v1/admin/accounts` 与 `GET /api/v1/admin/accounts/{account_id}` 返回登录状态、权益快照、自动领取 VIP 调度状态及关联音源列表（`sources` 数组）。
- **刷新 Token**。发送 `POST /api/v1/admin/accounts/{account_id}/refresh-token` 更新登录凭据。
- **同步权益**。发送 `POST /api/v1/admin/accounts/{account_id}/entitlements/sync` 读取上游快照，失败时保留上次成功记录。
- **更新音源设置**。发送 `PATCH /api/v1/admin/sources/{source_id}`，通过 `name` 修改标题，通过 `quality_fallback` 设置音质自动降级。

### 全站事件日志

`GET /api/v1/admin/logs` 查询取链、Token 刷新、扫码登录与权益事件。支持 `event_type`、`view=all|errors`、`account_id` 和 `source_id` 筛选。`limit` 默认为 30，可取 1–100。事件时间由 `created_at_ms` 返回，单位为 Unix 毫秒。

- `before_seq` 向前翻页，有更早的记录时返回 `next_before_seq`。
- `after_seq` 查询更新的记录，按 `seq` 降序返回。`has_more=true` 时用该批次最小的 `seq` 作 `before_seq` 继续翻页，直到补齐原游标之后的记录。
- `correlation_id` 按 `seq` 升序读取同一请求链路。两个游标和 `correlation_id` 每次只能使用其中一个。

### 自动领取 VIP

- `PATCH /api/v1/admin/accounts/{account_id}/entitlements/renewal` 接收 `{"enabled":true}` 或 `{"enabled":false}`。开关按账号生效，存量和新绑定账号默认为关闭。
- `POST /api/v1/admin/accounts/{account_id}/entitlements/renewal/claim` 手动领取或继续升级。手动操作在自动领取 VIP 关闭时仍可用，共用每日进度、互斥和退避。响应返回当前调度状态，业务结果见 `last_outcome`。例如，`awaiting_upgrade` 表示已领待升级，`waiting_for_reconciliation` 表示要先核对日记录。
- 账号列表与详情通过 `renewal` 字段返回调度状态。两个自动领取 VIP 接口直接返回该结构，其中包含每日领取与升级进度、上游日、`svip` 确认状态、退避截止及生效等待。账号和权益相关时间戳使用 Unix 秒。`estimated_end_at` 是本地推算，确认的到期信息见 `entitlements` 与 `entitlements_synced_at`。
- 全站日志包含领取、升级与日记录对账。自动领取沿用触发它的播放请求 ID。业务码 `131001` 代表已领取，`297002` 代表已升级。

自动领取在目标档返回可用链接且已确认的 `svip` 剩余不足 48 小时但仍在期时触发。权益确认已过期时，只有目标档明确返回 `status: 2` 才触发。两种情况均由后续后台任务处理，本次取链照常返回链接、执行降档或返回拒绝。升级确认后有约 120 秒传播等待。

写入发出前先保存当天对应步骤的进度。结果不明确时，下一次触发先查询日记录。当天的 `tvip` 记录确认领取，`svip` 记录确认两个步骤。写入尝试的单次预算依次为 5 秒、10 秒和 30 秒。记录未覆盖目标日期或无法解析时继续等待核对，读次数用尽时后台会提示。

人机验证或缺少明确业务结果的 HTTP 502 会暂停到次日。HTTP 502 中的明确业务拒绝按拒绝处理，幂等完成码仍确认对应步骤。限流的指数退避从 60 秒起，最多 1 小时，重启和跨日后继续生效。

发给 KuGouMusicApi 的扫码、刷新、设备注册、取链和权益请求都带有 `X-Apicache-Bypass: 1`，按当前会话获取响应。

当前按 UTC+8 划分上游日并解释没有时区的权益时间，带时区的 RFC3339 时间保留其偏移。切日前 35 秒暂缓写入，跨日收到的结果通过日记录核对。上游切日、跨日升级归属、实际日记录响应格式与可见延迟仍需实测。

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
- **首次启动**。新建 SQLite 基线的 `user_version` 为 1。同次事务从 v0.2.0 的 `accounts/` 和 `sources/` 导入账号、音源、令牌和已有权益快照。遇到损坏文件或引用冲突时，导入回滚并指出文件。
- **历史记录**。旧扫码会话与 JSONL 日志留在原目录，新会话和日志进入 SQLite。
- **备份与回退**。停服务后备份整个数据卷，切回 v0.2.0 时读取切换前的旧文件。

## 升级适配

- **开发 SQLite 库**。来自未发布开发版的库会被拒绝。停服备份后使用新的 `DATA_DIR` 创建基线；需要导入 v0.2.0 数据时，先把备份中的旧 `accounts/` 和 `sources/` 放进去。
- **管理 API 与页面**。音源列表和详情的旧读接口及 `/sources` 页面已下线。通过 `GET /api/v1/admin/accounts` 或 `GET /api/v1/admin/accounts/{account_id}` 读取账号及关联音源。旧音源刷新与日志接口分别改用账号的 `refresh-token` 和全站 `/api/v1/admin/logs?source_id=…`。按账号刷新返回 `ok` 和 `updated_at`，原音源设置 `PATCH` 地址仍可用。
- **配置与数据字段**。使用 `ACCOUNT_REFRESH_TTL` 配置登录态保鲜时间。账号响应移除了 `display_name`，旧 JSON 中的 `pending`、`disabled` 和 `expired` 在导入时归为 `active`。
- **洛雪脚本**。原 `/s/<script_token>.js` 地址和运行时令牌可继续使用，运行时接口兼容旧客户端包装为字符串的 JSON 请求体。要应用脚本端修复，请重新导入 `0.1.3`。
