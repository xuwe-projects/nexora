---
title: API 0.26.0
---

- 正式产物新增 `nexora-release.json`，应用可通过 `nexora::desktop::application_info` 读取经过
  校验的 app ID、版本、构建号和通道；开发模式仍可回退应用选项。
- updater 签名清单支持携带版本日志 URL、大小和 SHA-256，客户端只有在字段完整且内容验证通过
  时才返回 Markdown；旧清单保持安装兼容，但不会下载不完整日志。
- Windows 构建发布链路支持 x86_64/ARM64 WiX MSI、Burn Setup EXE、update ZIP 和最新安装器别名。
- `nexora build` 继续从 `${CARGO_PKG_VERSION}` 解析 SemVer，并用 `${BUILD_DATETIME}` 生成严格
  递增的本机时间构建号；两者与 target 一起冻结到 release receipt 和正式发布元数据。
