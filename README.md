# kg2lx

将**酷狗概念版 (Kugou Lite)** 会员作为洛雪音乐 ([LX Music](https://github.com/lyswhut/lx-music-desktop)) 的自定义音源。

它包含一个基于 Rust 的后端（负责协议处理和链接解析）以及一个基于 SvelteKit 的管理后台（负责账号管理和二维码登录）。

## 特性

- **扫码绑定：** 每个酷狗概念版账号对应一个独立音源，互不影响。
- **快捷导入：** 在后台复制脚本地址，粘贴到 LX Music 的自定义音源即可。
- **名字自定义：** 音源标题可自定义，但改完需要重新导入。
- 查看并同步**会员权益**。后台展示概念版 VIP 与畅听 VIP 的有效期，支持按账号开启自动领取 VIP、手动领取或核对当日记录。
- **音质自动降级：** 开启后，自动沿音质档位向下重试，尽量让你听到声音。
- 在**全站事件日志**中按账号和事件类型查询取链、Token 刷新、扫码登录及权益操作。取链事件还记录上游业务状态、实际音质与降级结果。
- **一键部署：** 提供预构建的 `linux/amd64` Docker 镜像与 Docker Compose 示例（见下）。

## 灵感来源

没别的，酷狗概念版是个很好的软件，但却没有一个好用的 PC 端（尤其是在 Linux 端）。

LX Music 的跨平台体验很好，我的歌单在上面同步，恰好我也有概念版会员，于是这个想法便诞生了。

## 免责声明

- 本项目仅供学习使用，请尊重版权，禁止用于商业或非法用途！
- 本项目**不提供**可用的账号，也不包含任何会员破解或音乐解锁行为，所有的音乐播放能力均自使用者自己的账号，请支持正版。
- 本项目仅作为中转服务，通过 [KuGouMusicApi](https://github.com/MakcRe/KuGouMusicApi) 直连官方服务器，不发往第三方 API。
- 本项目按原样提供源码，风险由使用者自负。

## 界面预览

![kg2lx v0.2.0 界面预览](./docs/images/demo.avif)

## Docker Compose 一键部署（推荐）

项目提供预构建的 `linux/amd64` Docker 镜像，发布在 GitHub Container Registry。需要部署两个服务：对外提供管理 UI 和运行时 API 的 `kg2lx`，以及仅供 Compose 网络内部访问的 `KuGouMusicApi`。

在任意目录下准备一个 `compose.yaml`，可以复制下面的示例，按需修改：

```yaml
services:
  # 上游 API。如果有独立部署的 KuGouMusicApi，可以删除此服务，
  kugou-api:
    image: ghcr.io/dreamalone666/kugou-music-api:45f31fefe97b-p5a78e70e
    environment:
      # 必须为 lite 才能支持概念版接口
      platform: lite
      HOST: 0.0.0.0
      PORT: 3000
    expose:
      # 仅在 Compose 网络内暴露
      - "3000"
    restart: unless-stopped

  kg2lx:
    image: ghcr.io/dreamalone666/kg2lx:v0.2.0
    # 有独立部署的 KuGouMusicApi 时，才需要删除
    depends_on:
      - kugou-api
    environment:
      # 【必填】本服务的对外访问地址，用于生成 LX 脚本链接
      PUBLIC_BASE_URL: http://your-server-host:8787
      # 【必填】管理后台登录 Token，建议修改为强密码
      ADMIN_TOKEN: change-me
      # 指向上游 API 地址，有独立部署的 KuGouMusicApi时，才需要修改
      KUGOU_API_BASE_URL: http://kugou-api:3000
      # 监听地址
      LISTEN_ADDR: 0.0.0.0:8787
      # 数据目录
      DATA_DIR: /data
    ports:
      - "8787:8787"
    volumes:
      # 持久化存储账号、音源及日志数据
      - kg2lx-data:/data
    restart: unless-stopped

volumes:
  kg2lx-data:
```

启动：

```bash
docker compose up -d
```

`kg2lx` 与 `KuGouMusicApi` 使用兼容的版本组合。更新版本时，将 `kg2lx` 镜像替换为目标 release tag，并将 `KuGouMusicApi` 替换为该 release 兼容的短 commit tag。

### 从 v0.2.0 升级到 SQLite

1. 停止旧容器并备份整个数据卷。用同一个卷及原来的 `PUBLIC_BASE_URL` 启动新版。
2. 首次启动将 v0.2.0 的账号、音源和已有权益快照导入 SQLite。旧脚本地址和两种令牌会沿用，已导入的洛雪音源可以继续播放。若要使用 `0.1.3` 脚本本身的修复，请在洛雪中重新导入。
3. 调用旧音源管理接口的客户端需切换到账号接口与全站日志接口。使用 `REFRESH_INTERVAL_SECS` 的部署需将配置移到 `ACCOUNT_REFRESH_TTL`。 [管理接口与配置迁移](./server/README.md#升级适配)

旧 JSON 文件、扫码会话和 JSONL 日志仍留在卷中。回退到 v0.2.0 会读取切换前的文件，升级后的新增和修改留在 SQLite。

之前运行过未发布 SQLite 开发版的环境，请停服备份后改用新的 `DATA_DIR`。如果需要保留 v0.2.0 的数据，可以将备份中原始的 `accounts/` 与 `sources/` 复制进去。开发 SQLite 库及其 WAL 文件留在备份中。

## 使用方法

1. **访问管理后台：** 浏览器打开管理后台（比如 `PUBLIC_BASE_URL`）。
2. **连接后端：** 在 UI 界面输入您的 `ADMIN_TOKEN` 完成鉴权。
3. **绑定账号：** 在“账号管理”页点击“绑定账号”，使用酷狗概念版 App 扫描二维码完成绑定。
4. **导入 LX Music：**
   - 在账号详情的「洛雪脚本接入」中复制 **脚本链接 (Script URL)**。
   - 打开 LX Music 客户端，在设置中将其作为 **自定义音源** 导入。
5. **维护账号与排障：** 在账号详情页刷新 Token、同步会员权益、开启或手动触发自动领取 VIP；在独立的“日志”页按事件类型与账号查看全站事件及因果链路。

## 手动部署

较为繁杂，只推荐开发使用。

### 前置要求
- [Rust](https://www.rust-lang.org/) 工具链 (最新稳定版)。
- [Node.js](https://nodejs.org/) 和 [pnpm](https://pnpm.io/)。

### 手动安装步骤

1. **KuGouMusicApi：**
   参考[原仓库文档](https://github.com/MakcRe/KuGouMusicApi)，配置 `platform=lite` 并启动。
2. **后端 (Server)：**
   ```bash
   cd server
   # 配置环境变量 PUBLIC_BASE_URL, ADMIN_TOKEN, KUGOU_API_BASE_URL
   cargo run
   ```
3. **前端 (Web)：**
   ```bash
   cd web
   pnpm install
   pnpm dev
   ```

## 项目结构

- [`/server`](./server): Rust/Axum 后端，处理 API 请求、账号持久化和音乐链接解析。
- [`/web`](./web): SvelteKit 管理后台，提供账号管理、扫码登录和全站事件日志。
- [`/KuGouMusicApi`](./KuGouMusicApi): (子模块) 核心协议转换服务。

## 配置项 (环境变量)

| 变量名 | 描述 | 默认值 |
| :--- | :--- | :--- |
| `PUBLIC_BASE_URL` | 服务对外访问的公网/内网地址 (必填) | - |
| `ADMIN_TOKEN` | 管理后台 API 鉴权 Token（必填，不能为空） | - |
| `KUGOU_API_BASE_URL` | `KuGouMusicApi` 服务的地址 (必填) | - |
| `LISTEN_ADDR` | 服务监听地址和端口 | `127.0.0.1:8787` |
| `DATA_DIR` | 数据持久化存储目录 | `./data` |
| `WEB_DIST_DIR` | 管理后台前端构建产物目录 (开启后同源托管) | - |
| `UPSTREAM_TIMEOUT_MS` | 请求上游 API 的超时时间（毫秒，正整数） | `10000` |
| `ACCOUNT_REFRESH_TTL` | 账号登录态保鲜阈值（秒，正整数）。**只在收到取链请求时按需判断，服务内没有定时任务** | `21600` (6 小时) |
| `EVENT_LOG_LIMIT` | 取链日志池保留上限，范围 1–100,000；其余三类事件各自最多保留 `min(该值, 1000)` 条 | `10000` |

## 许可证

本项目使用 [MIT](LICENSE) 许可证。
