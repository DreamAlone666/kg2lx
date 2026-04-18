# kg2lx

将**酷狗概念版 (Kugou Lite)** 会员作为洛雪音乐 ([LX Music](https://github.com/lyswhut/lx-music-desktop)) 的自定义音源。

它包含一个基于 Rust 的后端（负责协议处理和链接解析）以及一个基于 SvelteKit 的管理后台（负责账号管理和二维码登录）。

## 灵感来源

没别的，酷狗概念版是个很好的软件，但却没有一个好用的 PC 端（尤其是在 Linux 端）。

LX Music 的跨平台体验很好，我的歌单在上面同步，恰好我也有概念版会员，于是这个想法便诞生了。

## 免责声明

- 本项目仅供学习使用，请尊重版权，禁止用于商业或非法用途！
- 本项目**不提供**可用的账号，也不包含任何会员破解或音乐解锁行为，所有的音乐播放能力均自使用者自己的账号，请支持正版。
- 本项目仅作为中转服务，通过 [KuGouMusicApi](https://github.com/MakcRe/KuGouMusicApi) 直连官方服务器，不发往第三方 API。
- 本项目按原样提供源码，风险由使用者自负。

## 项目结构

- [`/server`](./server): Rust/Axum 后端，处理 API 请求、账号持久化和音乐链接解析。
- [`/web`](./web): SvelteKit 管理后台，提供管理音源、扫码登录的 UI 界面。
- [`/KuGouMusicApi`](./KuGouMusicApi): (子模块) 核心协议转换服务。

## Docker Compose 一键部署（推荐）

暂不提供构建好的 Docker 镜像，下面的 `compose.yaml` 会进行本地构建。

需要部署两个服务：对外提供管理 UI 和运行时 API 的 `kg2lx`，以及仅供内网访问的 `KuGouMusicApi`。

1. **克隆仓库（含子模块）：**
   ```bash
   git clone --recursive https://github.com/your-repo/kg2lx.git
   cd kg2lx
   ```
2. **在根目录准备 `compose.yaml`：**
   ```yaml
   services:
     # 上游 API，如果你有独立部署的 KuGouMusicApi，可以删掉
     kugou-api:
       build:
         context: ./KuGouMusicApi
       environment:
         # 必须为 lite 才能支持概念版接口
         platform: lite
         PORT: 3000
       expose:
         # 仅在 Compose 网络内暴露
         - "3000"
       restart: unless-stopped

     kg2lx:
       build:
         context: .
       depends_on:
         - kugou-api
       environment:
         # 【必填】本服务的对外访问地址，用于生成 LX 脚本链接
         PUBLIC_BASE_URL: http://your-server-host:8787
         # 【必填】管理后台登录 Token，建议修改为强密码
         ADMIN_TOKEN: change-me
         # 指向上游 API 地址，当你有独立部署的 KuGouMusicApi时，才需要修改
         KUGOU_API_BASE_URL: http://kugou-api:3000
         # 监听地址
         LISTEN_ADDR: 0.0.0.0:8787
         # 数据目录
         DATA_DIR: /data
       ports:
         - "8787:8787"
       volumes:
         # 持久化存储账号、音源及日志数据
         - ./server-data:/data
       restart: unless-stopped
   ```
4. **启动：**
   ```bash
   docker compose up -d --build
   ```

## 使用方法

1. **访问管理后台：** 浏览器打开管理后台。
2. **连接后端：** 在 UI 界面输入您的 `ADMIN_TOKEN` 完成鉴权。
3. **添加音源：** 进入“添加音源”页面，扫描二维码完成酷狗概念版账号绑定。
4. **导入 LX Music：**
   - 在“音源列表”页面复制 **脚本链接 (Script URL)**。
   - 打开 LX Music 客户端，在设置中将其作为 **自定义音源** 导入。

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

## 配置项 (环境变量)

| 变量名 | 描述 | 默认值 |
| :--- | :--- | :--- |
| `PUBLIC_BASE_URL` | 服务对外访问的公网/内网地址 (必填) | - |
| `ADMIN_TOKEN` | 管理后台 API 鉴权 Token (必填) | - |
| `KUGOU_API_BASE_URL` | `KuGouMusicApi` 服务的地址 (必填) | - |
| `LISTEN_ADDR` | 服务监听地址和端口 | `127.0.0.1:8787` |
| `DATA_DIR` | 数据持久化存储目录 | `./data` |
| `WEB_DIST_DIR` | 管理后台前端构建产物目录 (开启后同源托管) | - |
| `SOURCE_NAME_PREFIX` | 生成的音源名称前缀，生成结果形如 `酷狗概念版 <userid>` | `酷狗概念版` |
| `REFRESH_INTERVAL_SECS`| 账号状态自动刷新间隔 | `21600` (6 小时) |
