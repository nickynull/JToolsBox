// 把各网页端的构建产物暂存到 src-tauri/webapps/<name>/。
// 目的：让 tauri 的 bundle.resources 能把它们带进安装包，且资源路径不跨越 `..`
// （跨 `..` 的源路径会被 Tauri 映射到 `_up_/` 下，和运行期的查找路径对不上）。
import { cpSync, existsSync, mkdirSync, readdirSync, rmSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const webRoot = join(root, 'web');
const targetRoot = join(root, 'src-tauri', 'webapps');

if (!existsSync(webRoot)) {
  console.log('[stage-webapps] 没有 web/ 目录，跳过');
  process.exit(0);
}

// 与旧的 build.rs 约定一致：存在 index.html 的目录就是一个网页应用
const apps = readdirSync(webRoot, { withFileTypes: true })
  .filter((entry) => entry.isDirectory() && existsSync(join(webRoot, entry.name, 'index.html')))
  .map((entry) => entry.name);

if (apps.length === 0) {
  console.log('[stage-webapps] 未发现网页应用，跳过');
  process.exit(0);
}

// 整体重建，避免已删除的模块留下陈旧资源
rmSync(targetRoot, { recursive: true, force: true });
mkdirSync(targetRoot, { recursive: true });

let staged = 0;
for (const name of apps) {
  const dist = join(webRoot, name, 'dist');
  if (!existsSync(join(dist, 'index.html'))) {
    console.warn(`[stage-webapps] 跳过 ${name}：未找到 ${dist}，请先运行 npm run build:web:${name}`);
    continue;
  }

  // dest 不存在时 cpSync 会把 dist 整体复制成 dest
  cpSync(dist, join(targetRoot, name), { recursive: true });
  console.log(`[stage-webapps] web/${name}/dist → src-tauri/webapps/${name}`);
  staged += 1;
}

if (staged === 0) {
  console.warn('[stage-webapps] 没有任何网页资源被暂存，安装包内将缺少网页端');
}
