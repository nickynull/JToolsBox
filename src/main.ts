import { createApp, nextTick } from "vue";
import ElementPlus from "element-plus";
import zhCn from "element-plus/es/locale/lang/zh-cn";
import "element-plus/dist/index.css";
import "element-plus/theme-chalk/dark/css-vars.css";
import "./styles/theme.css";
import App from "./App.vue";
import { initSettings } from "./composables/useSettings";
import { getCurrentWindow } from "@tauri-apps/api/window";

/**
 * 等页面加载完成，最长等 1 秒：个别资源加载慢时不要拖住窗口显示。
 * load 事件不依赖窗口是否可见，比 requestAnimationFrame 可靠
 *（窗口隐藏时 rAF 可能被 WebView 节流甚至暂停）。
 */
function waitForLoad(timeoutMs = 1000): Promise<void> {
  if (document.readyState === "complete") return Promise.resolve();
  return new Promise((resolve) => {
    const finish = () => resolve();
    window.addEventListener("load", finish, { once: true });
    window.setTimeout(finish, timeoutMs);
  });
}

async function bootstrap() {
  await initSettings();
  const app = createApp(App);
  app.use(ElementPlus, { locale: zhCn });
  app.mount("#app");

  // 窗口以 visible: false 启动（见 tauri.conf.json），渲染就绪后才显示。
  // 显示前露出的是 Rust 侧按当前主题设置好的窗口底色，所以不会出现白色闪屏。
  // 这里失败不阻塞启动：Rust 侧还有超时兜底显示。
  await nextTick();
  await waitForLoad();
  try {
    await getCurrentWindow().show();
  } catch (error) {
    console.error("显示主窗口失败，等待兜底逻辑", error);
  }
}

bootstrap();
