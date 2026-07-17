import DefaultTheme from "vitepress/theme";
import type { Theme } from "vitepress";
import ProductShowcase from "./ProductShowcase.vue";
import "./style.css";

const theme: Theme = {
  extends: DefaultTheme,
  enhanceApp({ app }) {
    app.component("ProductShowcase", ProductShowcase);
    if (typeof window === "undefined") return;

    const base = import.meta.env.BASE_URL;
    const atChineseHome =
      window.location.pathname === base || window.location.pathname === `${base}index.html`;
    const detectionKey = "nexora-docs-language-detected";
    if (!atChineseHome || window.sessionStorage.getItem(detectionKey)) return;

    window.sessionStorage.setItem(detectionKey, "1");
    const preferredLanguage = navigator.languages[0] ?? navigator.language;
    if (preferredLanguage && !preferredLanguage.toLowerCase().startsWith("zh")) {
      window.location.replace(`${base}en/`);
    }
  },
};

export default theme;
