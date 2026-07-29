## Account 头像能力移除

- Account API 删除所有 `avatar_url` 契约字段、`POST /avatars`、`PATCH /users/{user_id}/avatar`
  和 `users:avatar.write` 权限；下游需要移除 DTO 字段访问与头像 API 调用。
- Account 服务端删除 `AvatarStorage`、`LocalAvatarStorage`、`AvatarUpload`、
  `AvatarStorageError` 和 `AccountDependencies::avatar_storage` 注入点。
- 新增迁移 `202607290001_account_remove_avatar_capability`，清理头像权限并删除
  `account.users.avatar_url`；该版本号高于当前 iMES 历史 seed `202607180006`，以保证空库合并迁移时
  旧 iMES seed 先执行、Nexora 再删除列。
- 默认用户管理和 Shell 登录用户区域仍保留圆形首字母/默认 Avatar 视觉标识，但不再读取图片 URL。
