# JToolsBox

> 作者：Jimmy · 应用标识：`com.jimmy.jtoolsbox`

一个基于 Tauri v2 + Vue 3 + TypeScript 构建的桌面工具集合应用。

## 功能特性

### 1. 自定义标题栏

- **搜索功能**：支持快速搜索功能模块和设置项
  - 快捷键 `Ctrl+F` 聚焦搜索框
  - 搜索结果支持点击直接跳转到对应功能或设置项
  - 设置项搜索支持深度链接，自动定位并高亮显示
- **主题切换**：支持浅色/深色/跟随系统三种主题模式
- **侧边栏控制**：可切换侧边栏显示/隐藏
- **窗口控制**：最小化、最大化、关闭按钮（关闭行为可在设置中配置）

### 2. 侧边栏导航

- 可折叠的侧边栏，宽度 180px
- 当前支持的功能模块：
  - 文件分享
  - 设置

### 3. 文件分享功能

基于 Web 服务器的文件分享功能，支持多用户认证和文件管理。

#### 服务器配置

- **端口配置**：自定义 Web 服务器监听端口
- **共享文件夹**：支持添加多个共享文件夹路径（支持文件夹选择器，可一键在文件资源管理器中打开）
- **启用控制**：可开关文件分享服务
- **Token 有效期**：配置客户端登录后的免登录时长（单位：小时）

#### 用户管理

- 创建/编辑/删除用户
- 用户角色管理
- 用户名密码认证（bcrypt 加密）
- **在线状态**：显示每个用户是否在线及最近登录时间
- **强制下线**：立即失效指定用户的全部登录会话（不能下线当前登录账号）；修改/删除用户也会使其已有会话失效

#### 客户端功能

网页端为独立的 Vue 3 + Element Plus 应用，构建产物内嵌进二进制，随服务启动由 Rust 直接托管。

- **Web 界面访问**：通过浏览器访问 `http://localhost:<端口>`，手机/其他电脑同网段可用
- **登录认证**：JWT + 服务端会话，token 保存在本地，会话可被管理员强制下线
- **文件操作**：
  - 文件列表浏览（面包屑、排序、当前目录搜索、共享目录切换）
  - 文件上传（拖拽或点选，分片上传，**逐文件实时进度、速度与目标路径**，支持暂停/继续/取消；上传开始即在服务端占用文件名，磁盘同名或并发同名上传立即提示，确认后追加时间戳改名，不覆盖原文件）
  - 文件下载（流式传输，支持断点/多线程续传所需的 Range 请求）
  - 在线预览：图片、视频（可拖动进度条）、音频、PDF、文本/代码
  - 文件/文件夹删除（二次确认，**仅管理员可操作**，普通用户按钮禁用并提示原因）
- **用户管理界面**：管理员可在线增删改用户，查看在线状态与最近登录时间，支持强制下线，禁止删除/下线当前登录账号
- **操作日志查看**：记录并筛选所有用户操作
- **外观**：浅色/深色切换，窄屏自适应

#### 数据存储

- 配置文件：`function-datas/file-sharing/config.json`
- 用户数据：`function-datas/file-sharing/users.json`
- 操作日志：`function-datas/file-sharing/logs/YYYY-MM-DD.jsonl`
- 共享文件：用户配置的共享文件夹路径

### 4. 设置功能

#### 外观设置

- **主题模式**：浅色/深色/跟随系统

#### 系统设置

- **关闭按钮行为**：配置点击关闭按钮时最小化到托盘或直接退出

#### 数据存储设置

- **数据文件存放路径**：
  - 显示当前 `function-datas` 目录的完整路径
  - 支持点击路径打开文件夹
  - 支持通过文件夹选择器切换路径
  - 支持重置为默认路径（程序根目录/function-datas）
  - **自动迁移**：切换路径时，自动将所有功能模块的数据文件迁移到新位置

#### 数据存储架构

每个功能模块独占自己的数据文件夹：

```
function-datas/
├── file-sharing/          # 文件分享功能数据
│   ├── config.json       # 功能配置
│   ├── users.json        # 用户数据
│   └── logs/             # 操作日志
└── [其他功能]/            # 未来其他功能模块
```

设置功能视为特例，其配置只存放于程序根路径的 `config.json`。

## 技术栈

### 后端（Rust）

- **Tauri v2**：桌面应用框架
- **axum**：Web 服务器框架
- **tokio / tokio-util**：异步运行时与流式 I/O（大文件上传/下载）
- **jsonwebtoken**：JWT 认证
- **bcrypt**：密码加密
- **uuid**：唯一标识符生成
- **chrono**：时间处理
- **serde/serde_json**：序列化/反序列化

### 前端（TypeScript + Vue 3）

桌面端与各功能网页端共用同一套依赖版本，但代码完全独立。

- **Vue 3**：前端框架（使用 `<script setup>` SFC）
- **Element Plus**：组件库（桌面端与网页端统一视觉）
- **Vite**：构建工具
- **TypeScript**：类型安全
- **@tauri-apps/api**：Tauri 前端 API
- **@tauri-apps/plugin-dialog**：文件对话框插件
- **@tauri-apps/plugin-opener**：文件打开插件

## 开发环境

### 推荐 IDE

- [VS Code](https://code.visualstudio.com/) + [Vue - Official](https://marketplace.visualstudio.com/items?itemName=Vue.volar) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)

### 系统要求

- Node.js 18+
- Rust 1.70+
- Windows 10/11（当前开发平台）

### 安装依赖

```bash
# 安装前端依赖
npm install

# Rust 依赖会在首次构建时自动下载
```

### 开发命令

```bash
# 启动桌面端开发（vite + tauri）
npm run tauri dev

# 构建桌面端（仅前端类型检查 + 打包）
npm run build

# 构建全部网页模块后再打包桌面端（网页产物会嵌入二进制）
npm run build:web:file-sharing && npm run tauri build

# 单独构建某个功能网页（类型检查 + vite build）
npm run build:web:file-sharing

# 单独调试某个功能网页（独立浏览器访问，/api 代理到 9800）
npm run dev:web:file-sharing
```

> 网页产物位于 `web/<功能>/dist`，已被 `.gitignore` 忽略。新拉取代码后先执行对应的 `build:web:*`，否则网页端会显示"网页端尚未构建"提示。

## 项目结构

```
JToolsBox/
├── src/                              # 桌面端前端源码
│   ├── components/                   # Vue 组件
│   │   ├── TitleBar.vue             # 自定义标题栏
│   │   └── Sidebar.vue              # 侧边栏导航
│   ├── views/                        # 页面视图
│   │   ├── FileSharingView.vue      # 文件分享管理界面
│   │   └── SettingsView.vue         # 设置页面
│   ├── composables/                  # 组合式函数
│   │   └── useSettings.ts           # 设置管理（单例模式）
│   ├── styles/                       # 样式文件
│   │   └── theme.css                # 主题变量系统
│   ├── App.vue                       # 主应用组件
│   └── main.ts                       # 应用入口
├── web/                              # 各功能独立的网页端（互不共享代码）
│   └── file-sharing/                 # 文件分享网页端
│       ├── index.html
│       ├── vite.config.ts            # 独立配置（/api 代理到 9800）
│       ├── tsconfig.json
│       ├── dist/                     # 构建产物（git 忽略，编译时嵌入）
│       └── src/
│           ├── main.ts
│           ├── App.vue
│           ├── api.ts                # 接口封装（含分片上传控制器：进度/暂停/取消）
│           ├── types.ts / utils.ts
│           ├── theme.css
│           ├── views/                # 登录 / 文件 / 用户 / 日志
│           └── components/           # 预览弹窗等
├── src-tauri/                        # Rust 后端
│   ├── src/
│   │   ├── features/                 # 功能模块区（一个功能一个目录）
│   │   │   └── file_sharing/
│   │   │       ├── mod.rs           # 模块定义和状态管理
│   │   │       ├── auth.rs          # JWT 认证和密码加密
│   │   │       └── server.rs        # Web 服务器和 API 路由
│   │   ├── web_support.rs           # 内嵌网页托管（WebApp / 静态资源）
│   │   └── lib.rs                   # 主库和 Tauri 命令
│   ├── build.rs                      # 编译期扫描 web/*/dist 并生成嵌入表
│   ├── capabilities/
│   │   └── default.json             # Tauri 权限配置
│   └── Cargo.toml                   # Rust 依赖
└── README.md                         # 项目说明
```

## 网页端模块

功能网页按"一个功能一个目录"组织，互不共享代码，只在托管方式上复用 `web_support.rs`。

- `web/<功能>/index.html` 存在即被视为一个网页应用
- 编译时 `build.rs` 扫描该目录，把 `dist` 全部文件嵌入，并生成 Rust 模块 `web_support::generated::<功能名>`（`file-sharing` → `file_sharing`，非字母数字转下划线）
- 功能自己的服务端代码用它来响应页面与静态资源

新增一个功能网页的步骤：

1. 新建 `web/<功能名>/`，放入 `index.html`、`vite.config.ts`（`root` 指向自身目录）、`tsconfig.json` 与 `src/`
2. 在 `package.json` 增加 `build:web:<功能名>` / `dev:web:<功能名>` 脚本
3. 在 `src-tauri/src/features/<功能>/server.rs` 中声明
   `static WEB_APP: WebApp = WebApp::new(generated::<模块名>::ASSETS, generated::<模块名>::INDEX_HTML);`
4. 在 `features/mod.rs` 注册模块，把页面路由挂到该功能自己的服务上
5. 执行 `npm run build:web:<功能名>` 后再编译 Rust

## 功能规划

- [x] 自定义标题栏和窗口控制
- [x] 主题系统（浅色/深色/跟随系统）
- [x] 搜索功能和快捷键
- [x] 文件分享功能
- [x] 文件分享网页端（Vue 3 + Element Plus，分片上传进度/暂停/取消、在线预览）
- [x] 功能网页模块化（每功能独立网页，构建期嵌入二进制）
- [x] 数据路径管理和迁移
- [ ] 更多工具功能（待开发）

## 许可证

本项目基于 [MIT License](LICENSE) 开源。

Copyright (c) 2026 Jimmy
