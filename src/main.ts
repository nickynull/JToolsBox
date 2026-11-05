import { createApp } from "vue";
import ElementPlus from "element-plus";
import "element-plus/dist/index.css";
import "element-plus/theme-chalk/dark/css-vars.css";
import "./styles/theme.css";
import App from "./App.vue";
import { initSettings } from "./composables/useSettings";

async function bootstrap() {
  await initSettings();
  const app = createApp(App);
  app.use(ElementPlus);
  app.mount("#app");
}

bootstrap();
