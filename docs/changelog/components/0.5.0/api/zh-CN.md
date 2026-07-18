## ZITADEL 用户与权限闭环

- 创建用户由服务端调用 ZITADEL gRPC 完成，再自动绑定本地 Account；登录时同步最新用户名、
  邮箱、展示名与头像。
- 新权限会自动加入系统管理员角色，升级迁移同时补齐已有权限。
- GPUI 与 gpui-component 使用经过无锁下游编译验证的固定兼容 revision。
