## Account 认证作用域

- 新增不含 token 的 `AccountAuthenticationScope`，以进程内 revision 和本地
  `account.users(id)` 标识当前认证作用域。
- 桌面宿主可以读取并观察作用域，在退出、重新登录或换账号时清理自己的会话级 Global，
  并丢弃属于旧作用域的异步响应。
