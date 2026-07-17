<script setup lang="ts">
import { withBase } from "vitepress";

const props = withDefaults(defineProps<{ locale?: "zh" | "en" }>(), {
  locale: "zh",
});

const copy =
  props.locale === "en"
    ? {
        label: "Nexora desktop interface overview in light and dark modes",
        workspaceDark: "Nexora workspace in dark mode",
        workspaceLight: "Nexora workspace in light mode",
        settingsDark: "Nexora appearance settings in dark mode",
        settingsLight: "Nexora appearance settings in light mode",
        loginDark: "Nexora login screen in dark mode",
        loginLight: "Nexora login screen in light mode",
      }
    : {
        label: "Nexora 桌面界面浅色与深色模式总览",
        workspaceDark: "Nexora 深色工作区",
        workspaceLight: "Nexora 浅色工作区",
        settingsDark: "Nexora 深色外观设置",
        settingsLight: "Nexora 浅色外观设置",
        loginDark: "Nexora 深色登录页",
        loginLight: "Nexora 浅色登录页",
      };

const image = (name: string) => withBase(`/images/showcase/${name}.png`);
</script>

<template>
  <section class="nexora-showcase" :aria-label="copy.label">
    <div class="nexora-showcase__card nexora-showcase__workspace">
      <div class="nexora-showcase__pair">
        <img :src="image('workspace-dark')" :alt="copy.workspaceDark" />
        <img
          class="nexora-showcase__light"
          :src="image('workspace-light')"
          :alt="copy.workspaceLight"
        />
        <span class="nexora-showcase__divider" aria-hidden="true" />
      </div>
    </div>

    <div class="nexora-showcase__card nexora-showcase__settings">
      <div class="nexora-showcase__pair">
        <img :src="image('settings-dark')" :alt="copy.settingsDark" />
        <img
          class="nexora-showcase__light"
          :src="image('settings-light')"
          :alt="copy.settingsLight"
        />
        <span class="nexora-showcase__divider" aria-hidden="true" />
      </div>
    </div>

    <div class="nexora-showcase__card nexora-showcase__login">
      <div class="nexora-showcase__pair">
        <img :src="image('login-dark')" :alt="copy.loginDark" />
        <img
          class="nexora-showcase__light"
          :src="image('login-light')"
          :alt="copy.loginLight"
        />
        <span class="nexora-showcase__divider" aria-hidden="true" />
      </div>
    </div>
  </section>
</template>
