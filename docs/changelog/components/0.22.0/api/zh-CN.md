---
title: API 0.22.0
---

- `nexora build` 支持以 `${CARGO_PKG_VERSION}` 和 `${BUILD_DATETIME}` 解析并冻结发布身份；
  publish、dry-run 与 yank 只消费构建收据并完整校验既有产物。
