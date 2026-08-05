---
title: API 0.27.0
---

- CLI 已迁移到独立 `cli` package；正式安装与仓库 lint/build/publish 命令不再使用 `nexora`
  package 的 `cli` feature。
- `nexora build` 继续用 `${CARGO_PKG_VERSION}` 解析所选应用版本，并用 `${BUILD_DATETIME}`
  生成严格递增构建号；外部分发文件改用展示名称与架构，内部 executable 仍保持 package 身份。
- publish 支持 channel 级对象存储覆盖与隔离凭据组，按不可变产物、品牌化 channel 根文件、
  sequence manifest、签名 `latest.json` 的顺序写入。
- 新增 `nexora update` 与六个平台目标的预编译 CLI 资产；更新只替换经过 size/SHA-256 校验的
  CLI binary，不修改应用依赖或源码。
