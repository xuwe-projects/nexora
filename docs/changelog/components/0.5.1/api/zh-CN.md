## FormDialog 公共 API 行为修复

- 修正 `submit_disabled` 的渲染映射，使实现与公开 rustdoc 契约一致。
- 本补丁不改变 HTTP API、ZITADEL 集成或数据库 schema。
