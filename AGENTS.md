# AGENTS.md — AI 协作上下文

本文件是所有 AI 编码助手（Codebuddy / Claude Code / Cursor / Copilot 等）进入本仓库的**唯一入口**，
写清"项目是什么、代码在哪、怎么跑、别踩哪些坑"。人类使用说明见 [README.md](README.md)。

> 维护约定：架构或命令变化时，请同步更新本文件；不要把临时调试结论写进来（那些放 `.workbuddy/memory/` 的日志）。

---

## 1. 项目是什么

JToolsBox（作者 Jimmy，包名 `com.jimmy.jtoolsbox`）是一个 **Tauri v2 桌面工具集合**。
桌面壳里是工具列表，每个工具一个页面；当前已实现「文件分享」，其余工具待开发。

技术要点：桌面 UI 是 Vue 3 SPA；同时 Rust 侧进程内跑一个 **axum HTTP 服务**对外提供文件分享；
文件分享的网页端是一套**独立的 Vue 应用**（`web/file-sharing/`），在**编译期**被打包进二进制，
由同一个 axum 服务对外托管。所以"一个 exe 既是桌面程序，也是一个小型 Web 服务"。

## 2. 技术栈

| 层 | 选型 |
| --- | --- |
| 桌面壳 / 打包 | Tauri v2（`src-tauri/`，Rust edition 2024） |
| 桌面 UI | Vue 3 + TypeScript + Element Plus（中文语言包、暗色 CSS 变量） |
| 构建 | Vite 8（桌面端 1420 端口；网页端各自独立 vite 配置） |
| 后端 HTTP | axum 0.7 + tower-http（cors/fs）+ tokio |
| 认证 | jsonwebtoken（JWT）+ bcrypt，`/api/login` 签发 |
| 发布 | GitHub Actions + release-please（Conventional Commits 驱动版本与 CHANGELOG） |

## 3. 目录地图

```
├─ src/                      桌面端 UI（Tauri WebView 加载）
│  ├─ App.vue                视图状态机（无 vue-router）+ 布局
│  ├─ components/            TitleBar（自定义标题栏 + 全局搜索）、Sidebar（导航）
│  ├─ views/                 FileSharingView（管理端）、SettingsView（设置，带锚点）
│  ├─ composables/           useSettings（全局单例设置）、useNotify（提示）
│  └─ styles/theme.css       主题 CSS 变量（浅/深色）
├─ web/<功能>/                各功能独立网页端，互不共享代码
│  └─ file-sharing/          文件分享网页（登录 / 文件 / 用户 / 日志 / 预览）
├─ src-tauri/
│  ├─ src/lib.rs             Builder 装配：命令注册、托盘、窗口事件、状态初始化
│  ├─ src/features/<功能>/    功能后端（mod.rs 状态与持久化，server.rs 路由，auth.rs 认证）
│  ├─ src/web_support.rs     内嵌网页托管（WebApp + 静态资源响应）
│  ├─ build.rs               编译期扫描 web/*/dist → 生成 web_assets.rs 嵌入表
│  └─ capabilities/          Tauri 权限（新增窗口/对话框 API 必须在此放行）
└─ scripts/build-all.mjs     并行跑桌面端与所有网页端的 build
```

## 4. 常用命令

```bash
npm install                      # 首次
npm run tauri dev                # 日常开发（会先 build 网页端，再起 vite + 桌面窗口）
npm run dev                      # 只起桌面端前端（localhost:1420，不含 Rust）
npm run dev:web:file-sharing     # 只在浏览器调试文件分享网页（/api 代理到 9800）

npm run build                    # 桌面端类型检查 + 打包前端
npm run build:web:file-sharing   # 文件分享网页：类型检查 + 打包到 web/file-sharing/dist
npm run build:all                # 上面两个并行（tauri build 的 beforeBuildCommand）
npm run tauri build              # 出安装包
```

校验（改完代码至少跑一遍）：

```bash
npm run build                    # vue-tsc 严格类型检查 + vite build
cd src-tauri && cargo check      # Rust 编译检查（首次较慢）
```

## 5. 必须知道的机制（踩坑区）

1. **`dist` 不入库，且是编译期快照。**
   `web/<功能>/dist` 被 `.gitignore` 忽略。新克隆仓库时若没先跑 `npm run build:web:file-sharing`，
   网页端会返回 build.rs 生成的"网页端尚未构建"占位页。
   改了网页端源码后必须重新 `cargo build`/`tauri dev`（`include_bytes!` 是编译期嵌入，build.rs 通过
   `rerun-if-changed` 监听 `dist` 才触发重编）。

2. **`web/<目录名>` 存在 `index.html` 就等于注册了一个网页应用。**
   build.rs 会为它生成 `web_support::generated::<模块名>`（非字母数字转下划线，数字开头加 `app_`），
   内含 `ASSETS` 与 `INDEX_HTML`。新增网页端不需要改 build.rs，但要在 `package.json` 加对应
   `build:web:<name>` 脚本并加入 `scripts/build-all.mjs` 的并行列表。

3. **配置与数据不在 AppData。**
   `config.json` 与数据根 `function-datas/`（可被 `data_root_path` 覆盖）都位于 **exe 同级目录**
   （`get_config_path` / `get_data_root` 用 `current_exe()`）。因此改 `identifier` 不会丢用户数据；
   但切换数据根时代码只迁移一级子目录。

4. **前后端数据结构是两份手写。**
   Rust 的 `#[derive(Serialize, Deserialize)]` 结构体与前端 TS `interface`（`src/composables/useSettings.ts`、
   `web/file-sharing/src/types.ts`）必须人工保持一致：字段名、可空性、可选性。字段用 snake_case
   （Rust 默认，未开 rename_all）。

5. **桌面窗口 `decorations: false`**，标题栏是自己画的。涉及最小化/最大化/拖拽/主题的 API
   必须在 `src-tauri/capabilities/default.json` 放行，否则**运行期**才报权限错误。
   `close_action = hide` 时关闭按钮被拦截并隐藏到托盘（`lib.rs` 的 `on_window_event`）。
   托盘右键菜单（`TrayMenuItems`）里「开启/停止服务」的可用性由 `refresh_tray_menu` 维护，
   启停服务后**必须**调用它（托盘点击、`start_file_sharing`/`stop_file_sharing` 命令两处都已接入），
   否则菜单项状态会与实际相反；托盘操作结果通过 `tray-toast`、状态变化通过
   `file-sharing-status-changed` 事件推给前端（`App.vue` / `FileSharingView.vue`）。

6. **文件分享服务**：axum 监听 `0.0.0.0:<port>`（默认 9800）。路由 `/`（页面）、`/assets/*`、
   `/api/login`、`/api/*`（JWT 保护）。网页端每次请求带 `Authorization: Bearer`，
   下载/预览走 query token；401 触发重新登录。日志按日写入 `logs/YYYY-MM-DD.jsonl`，
   启动时与每 6 小时按 `log_retention_days` 清理。
   `stop_server` 会**等 axum 优雅关闭真正完成**（`status.running` 回落，最多 5 秒）才返回，
   不要改回"发完 shutdown 信号就返回"——否则调用方紧接着读状态会拿到仍在运行的中间态
   （表现为托盘菜单/界面状态不刷新）。

7. **版本号由 release-please 维护**（`package.json` + `Cargo.toml` + `tauri.conf.json` 三处），
   不要手改；提交信息必须符合 Conventional Commits（`feat:`、`fix:`、`refactor:` 等），否则不产生发版。

## 6. 编码约定

- 注释、UI 文案、日志文本用**中文**；标识符、文件名用英文。
- Rust 命令统一返回 `Result<T, String>`，错误信息面向用户（前端直接弹提示）。
- 前端不引入 vue-router：视图切换靠 `App.vue` 的 `currentView`，新增视图要同步
  `viewTitle` 映射、`Sidebar.vue` 的 `navItems`、`TitleBar.vue` 的 `searchIndex`。
- 设置项新增后：`SettingsView.vue` 里的卡片容器要有 `id="setting-<key>"`，
  搜索深链靠 `document.getElementById('setting-' + id)` 滚动并高亮。
- 颜色一律走 `theme.css` 的 CSS 变量，**浅色/深色两套都要核对**，禁止硬编码色值。
- 表格/表单用 Element Plus 组件，不手写基础控件。
- 磁盘 IO、网络、长任务不要阻塞 UI 线程；Rust 侧用 `tokio::spawn` / 异步命令。

## 7. 新增一个功能的标准动作

1. 桌面 UI：`src/views/XxxView.vue` → 注册到 `App.vue`（`titles`）→ `Sidebar.vue`（`navItems`）→ `TitleBar.vue`（`searchIndex`）。
2. 后端：`src-tauri/src/features/xxx/{mod.rs,server.rs}` → `features/mod.rs` 声明 → `lib.rs` 里
   `manage` 状态并加入 `generate_handler!`。
3. 需要网页端：新建 `web/xxx/index.html` + `vite.config.ts` + `tsconfig.json` + `src/`；
   `package.json` 加 `build:web:xxx`、`dev:web:xxx`；`scripts/build-all.mjs` 并行列表加上它。
4. 新权限：`capabilities/default.json` 补条目。
5. 文档：README 的功能特性/项目结构、本文件第 3 节目录地图。

## 8. 完成前自检

- [ ] `npm run build` 通过（含 `vue-tsc --noEmit`，类型错误一律修，不用 `any` 掩盖）
- [ ] `cd src-tauri && cargo check` 通过
- [ ] 涉及网页端时，`npm run build:web:file-sharing` 后重新编译并实际打开页面验证
- [ ] 浅色 + 深色主题各看一遍
- [ ] 没有提交 `dist/`、`web/*/dist`、`src-tauri/target`
- [ ] 提交信息符合 Conventional Commits

## 9. 不要做

- 不要为"顺手优化"重排目录、改 crate 名（`jtoolsbox` / `jtoolsbox_lib` 被 `main.rs` 引用）。
- 不要把配置改到 AppData（会破坏"绿色便携"的使用方式）。
- 不要在前端直接 `fetch` 桌面端能力，统一走 `invoke`（`@tauri-apps/api/core`）。
- 不要在 `web/*` 之间共享代码（设计上刻意隔离，复制优于耦合）。
- 不要删除 `.workbuddy/` 目录（协作记忆，不是缓存）。
