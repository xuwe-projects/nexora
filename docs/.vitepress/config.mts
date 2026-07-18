import { defineConfig, type DefaultTheme } from "vitepress";

const repository = "https://github.com/xuwe-projects/nexora";

const zhSidebar: DefaultTheme.SidebarItem[] = [
  {
    text: "开始使用",
    items: [
      { text: "介绍", link: "/guide/introduction" },
      { text: "快速开始", link: "/guide/getting-started" },
    ],
  },
  {
    text: "桌面应用",
    items: [
      { text: "Application 与品牌", link: "/desktop/application" },
      { text: "Feature 与导航", link: "/desktop/features" },
      { text: "公共桌面组件", link: "/desktop/components" },
      { text: "Account", link: "/desktop/account" },
    ],
  },
  {
    text: "服务端",
    items: [
      { text: "Server 与 Router", link: "/server/overview" },
      { text: "HTTP API 完整参考", link: "/server/http-api" },
      { text: "Rust 服务端 API", link: "/server/rust-api" },
    ],
  },
  {
    text: "参考",
    items: [
      { text: "配置", link: "/reference/configuration" },
      { text: "CLI", link: "/reference/cli" },
    ],
  },
  {
    text: "版本发布",
    items: [
      { text: "更新日志", link: "/changelog/" },
      { text: "0.5.0", link: "/changelog/0.5.0" },
      { text: "从 0.4.1 升级", link: "/changelog/0.5.0#破坏性变更与迁移" },
    ],
  },
];

const enSidebar: DefaultTheme.SidebarItem[] = [
  {
    text: "Getting Started",
    items: [
      { text: "Introduction", link: "/en/guide/introduction" },
      { text: "Quick Start", link: "/en/guide/getting-started" },
    ],
  },
  {
    text: "Desktop",
    items: [
      { text: "Application and Branding", link: "/en/desktop/application" },
      { text: "Features and Navigation", link: "/en/desktop/features" },
      { text: "Shared Desktop Components", link: "/en/desktop/components" },
      { text: "Account", link: "/en/desktop/account" },
    ],
  },
  {
    text: "Server",
    items: [
      { text: "Server and Routers", link: "/en/server/overview" },
      { text: "Complete HTTP API", link: "/en/server/http-api" },
      { text: "Rust server API", link: "/en/server/rust-api" },
    ],
  },
  {
    text: "Reference",
    items: [
      { text: "Configuration", link: "/en/reference/configuration" },
      { text: "CLI", link: "/en/reference/cli" },
    ],
  },
  {
    text: "Releases",
    items: [
      { text: "Changelog", link: "/en/changelog/" },
      { text: "0.5.0", link: "/en/changelog/0.5.0" },
      { text: "Upgrade from 0.4.1", link: "/en/changelog/0.5.0#upgrade-from-041" },
    ],
  },
];

const sharedTheme: DefaultTheme.Config = {
  search: { provider: "local" },
  socialLinks: [{ icon: "github", link: repository }],
};

export default defineConfig({
  title: "Nexora",
  description: "Rust desktop full-stack framework built with GPUI and Axum.",
  base: process.env.DOCS_BASE ?? "/nexora/",
  cleanUrls: true,
  lastUpdated: true,
  head: [["meta", { name: "theme-color", content: "#2563eb" }]],
  locales: {
    root: {
      label: "简体中文",
      lang: "zh-CN",
      title: "Nexora",
      description: "基于 GPUI 与 Axum 的 Rust 桌面全栈框架。",
      themeConfig: {
        ...sharedTheme,
        nav: [
          { text: "指南", link: "/guide/getting-started" },
          { text: "桌面端", link: "/desktop/application" },
          { text: "服务端", link: "/server/overview" },
          { text: "CLI", link: "/reference/cli" },
          { text: "更新日志", link: "/changelog/" },
        ],
        sidebar: zhSidebar,
        outline: { label: "本页目录" },
        docFooter: { prev: "上一页", next: "下一页" },
        lastUpdated: { text: "最后更新" },
        editLink: {
          pattern: `${repository}/edit/main/docs/:path`,
          text: "在 GitHub 上编辑此页",
        },
        footer: {
          message: "Nexora 目前处于 early alpha。",
          copyright: "MIT OR Apache-2.0",
        },
      },
    },
    en: {
      label: "English",
      lang: "en-US",
      link: "/en/",
      title: "Nexora",
      description: "A Rust desktop full-stack framework built with GPUI and Axum.",
      themeConfig: {
        ...sharedTheme,
        nav: [
          { text: "Guide", link: "/en/guide/getting-started" },
          { text: "Desktop", link: "/en/desktop/application" },
          { text: "Server", link: "/en/server/overview" },
          { text: "CLI", link: "/en/reference/cli" },
          { text: "Releases", link: "/en/changelog/" },
        ],
        sidebar: enSidebar,
        editLink: {
          pattern: `${repository}/edit/main/docs/:path`,
          text: "Edit this page on GitHub",
        },
        footer: {
          message: "Nexora is currently early alpha.",
          copyright: "MIT OR Apache-2.0",
        },
      },
    },
  },
  markdown: {
    defaultHighlightLang: "rust",
  },
});
