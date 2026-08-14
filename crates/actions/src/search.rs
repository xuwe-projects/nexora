//! Shell 全局搜索相关 action。
//!
//! 本模块只声明稳定命令身份，不注册默认快捷键。下游应用可以按自身平台约定调用
//! `App::bind_keys`；Nexora Shell 只在真实绑定存在时展示对应按键提示。

gpui::actions!(
    global_search,
    [
        /// 打开当前主窗口的全局搜索。
        OpenGlobalSearch
    ]
);
