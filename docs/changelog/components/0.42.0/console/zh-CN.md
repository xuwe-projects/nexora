---
title: Console 0.42.0
---

- stable、beta、nightly 现在使用独立安装目录、进程单例、更新状态、登录凭据和用户偏好。
- 旧版 beta/nightly 不会迁移共享数据；请使用新目录重新安装、登录并设置偏好。
- updater 会保留最近 10 次操作日志，每次最多 1 MiB；日志失败不会影响更新或回滚。
