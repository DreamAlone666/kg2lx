# kg2lx

将**酷狗概念版 (Kugou Lite)** 会员作为洛雪音乐 ([LX Music](https://github.com/lyswhut/lx-music-desktop)) 的自定义音源。

它包含一个基于 Rust 的后端（负责协议处理和链接解析）以及一个基于 SvelteKit 的管理后台（负责账号管理和二维码登录）。

## 灵感来源

没别的，酷狗概念版是个很好的软件，但却没有一个好用的 PC 端（尤其是在 Linux 端）。

LX Music 的跨平台体验很好，我的歌单在上面同步，恰好我也有概念版会员，于是这个想法便诞生了。

## 免责声明

- 本项目仅供学习使用，请尊重版权，禁止用于商业或非法用途！
- 本项目**不提供**可用的账号，也不包含任何会员破解或音乐解锁行为，所有的音乐播放能力均自使用者自己的账号。
- 本项目仅作为中转服务，通过 [KuGouMusicApi](https://github.com/MakcRe/KuGouMusicApi) 直连官方服务器，不发往第三方 API。

## 项目结构

- [`/server`](./server): Rust/Axum 后端，处理 API 请求、账号持久化和音乐链接解析。
- [`/web`](./web): SvelteKit 管理后台，提供连接后端和管理音源的 UI 界面。

## 快速开始

### 前置要求

- [Rust](https://www.rust-lang.org/) 工具链 (最新稳定版)。
- [Node.js](https://nodejs.org/) 和 [pnpm](https://pnpm.io/)。
- 正在运行的 `KuGouMusicApi` 服务，并配置为 `platform=lite`。

### 安装步骤

1. **克隆仓库（包含子模块）：**
   ```bash
   git clone --recursive https://github.com/your-repo/kg2lx.git
   cd kg2lx
   ```

2. **KuGouMusicApi：**
   参考原仓库 [README](https://github.com/MakcRe/KuGouMusicApi?tab=readme-ov-file#%E4%BD%BF%E7%94%A8%E6%8E%A5%E5%8F%A3%E4%B8%BA%E6%A6%82%E5%BF%B5%E7%89%88)，一定要注意使用配置为 `platform=lite`，这样才能使用概念版接口。

3. **后端配置：**
   进入 `/server` 目录，创建 `.env` 文件或设置环境变量：
   ```bash
   cd server
   # 填写以下必要变量：
   # PUBLIC_BASE_URL="http://your-server-ip:8787"
   # ADMIN_TOKEN="your-secure-token"
   # KUGOU_API_BASE_URL="http://your-kugou-api:3000"
   cargo run
   ```

4. **前端配置：**
   进入 `/web` 目录，安装依赖并启动开发服务器：
   ```bash
   cd ../web
   pnpm install
   pnpm dev
   ```

### 使用方法

1. 在浏览器中打开管理后台（默认 http://localhost:5173）。
2. 在页面输入您的后端地址和 `ADMIN_TOKEN` 连接到服务端。
3. 在**添加音源**页面扫描生成的二维码进行概念版账号绑定。
4. 绑定成功后，在音源页面选择音源并复制 **脚本链接 (Script URL)**。
5. 将该链接作为自定义音源导入到 **LX Music**。

## 配置项

后端可以通过环境变量或当前工作目录下的 `.env` 文件进行配置：

| 变量名 | 描述 | 默认值 |
| :--- | :--- | :--- |
| `PUBLIC_BASE_URL` | 服务对外访问的公网/内网地址 (必填) | - |
| `ADMIN_TOKEN` | 管理后台 API 鉴权 Token (必填) | - |
| `KUGOU_API_BASE_URL` | `KuGouMusicApi` 服务的地址 (必填) | - |
| `LISTEN_ADDR` | 服务监听地址和端口 | `127.0.0.1:8787` |
| `DATA_DIR` | 数据持久化存储目录 | `./data` |
| `SOURCE_NAME_PREFIX` | 生成的音源名称前缀 | `Kugou Concept VIP` |
| `REFRESH_INTERVAL_SECS`| 账号状态自动刷新间隔 | `21600` (6 小时) |
