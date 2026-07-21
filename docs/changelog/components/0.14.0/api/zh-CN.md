## Account 头像上传与 ZITADEL 同步

- Account API 新增 `POST /avatars`，上传头像后返回可写入用户资料的 `avatar_url`。
- Account API 新增 `PATCH /users/{user_id}/avatar`，可更新或清空用户头像并同步到 ZITADEL metadata。
- 新增 `users:avatar.write` 权限和迁移，内置 admin 角色升级后自动获得该权限。
