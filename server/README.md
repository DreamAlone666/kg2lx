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
- `ADMIN_TOKEN`：管理后台接口的鉴权 Token。
- `KUGOU_API_BASE_URL`：`KuGouMusicApi` 上游服务的地址。

**选填项：**

- `LISTEN_ADDR`：服务监听地址及端口，默认值为 `127.0.0.1:8787`。
- `DATA_DIR`：数据存储目录，默认值为 `./data`。
- `UPSTREAM_TIMEOUT_MS`：请求上游 API 的超时时间（毫秒），默认值为 `10000`。
- `SOURCE_NAME_PREFIX`：生成的音源名称前缀，默认值为 `Kugou Concept VIP`。
- `REFRESH_INTERVAL_SECS`：账号状态刷新间隔（秒），默认值为 `21600`。
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

**5. 强制刷新音源及账号状态**

```bash
curl -X POST \
  -H "Authorization: Bearer change-me" \
  http://127.0.0.1:8787/api/v1/admin/sources/<source_id>/refresh
```

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

服务的所有状态数据将以文件形式保存在配置的 `DATA_DIR` 目录下，主要结构如下：

- `accounts/*.json`：保存绑定的酷狗账号及凭证信息。
- `sources/*.json`：保存生成的音源配置及关联的 runtime token。
- `login_sessions/*.json`：保存扫码登录过程中的会话状态。
- `logs/runtime-YYYY-MM-DD.jsonl`：记录洛雪客户端调用获取链接接口的访问日志。
