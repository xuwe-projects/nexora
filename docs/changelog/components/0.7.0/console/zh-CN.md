## 应用自定义图标资产

- 桌面应用现在可以通过 `ApplicationOptions::application_assets(...)` 注册自己的 GPUI
  `AssetSource`，自定义 SVG 会优先于框架默认资源查找。
- 新生成项目默认嵌入 `assets/icons/**/*.svg`，`icon = "warehouse"` 会读取
  `assets/icons/warehouse.svg`。
