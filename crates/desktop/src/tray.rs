//! 主进程系统托盘。
//!
//! macOS 与 Windows 使用各自原生状态栏/通知区实现；Linux 直接实现
//! freedesktop StatusNotifierItem（SNI），不依赖 GTK 或 AppIndicator。

use std::fmt;

/// 托盘向主应用事件循环报告的用户意图或宿主状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayEvent {
    /// 显示并激活当前应用的全部窗口。
    ActivateWindowGroup,
    /// 退出当前应用及其全部原生窗口。
    ExitApplication,
    /// Linux SNI watcher 或平台托盘宿主暂时不可用。
    Unavailable,
    /// Linux SNI watcher 恢复可用。
    Available,
}

/// 创建原生托盘失败。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayError {
    message: String,
}

impl TrayError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for TrayError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for TrayError {}

/// 主进程持有的系统托盘生命周期对象。
///
/// 必须在桌面平台事件循环已经启动后创建，并保留到应用退出。`icon_png` 必须是有效
/// PNG；macOS/Windows 会解码为原生托盘图标，Linux 会转换为 SNI 要求的 ARGB32。
pub struct TrayController {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    native: NativeTray,
    #[cfg(target_os = "linux")]
    linux: LinuxTrayController,
    available: bool,
}

impl TrayController {
    /// 创建带“显示窗口”和“退出”菜单的系统托盘。
    ///
    /// # Errors
    ///
    /// PNG 无法解码、平台原生图标或菜单创建失败、Linux D-Bus/SNI 注册失败，或当前
    /// 平台没有托盘实现时返回错误。调用方应保持窗口可见或降级为普通最小化。
    pub fn new(
        application_id: &str,
        application_name: &str,
        icon_png: &[u8],
    ) -> Result<Self, TrayError> {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            let native = NativeTray::new(application_id, application_name, icon_png)?;
            Ok(Self {
                native,
                available: true,
            })
        }
        #[cfg(target_os = "linux")]
        {
            let linux = LinuxTrayController::new(application_id, application_name, icon_png)?;
            Ok(Self {
                linux,
                available: true,
            })
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        {
            let _ = (application_id, application_name, icon_png);
            Err(TrayError::new("当前平台没有 Nexora 系统托盘实现"))
        }
    }

    /// 返回托盘宿主当前是否可用。
    pub const fn is_available(&self) -> bool {
        self.available
    }

    /// 非阻塞读取一个托盘事件。
    pub fn try_recv(&mut self) -> Option<TrayEvent> {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        let event = self.native.try_recv();
        #[cfg(target_os = "linux")]
        let event = self.linux.try_recv();
        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        let event = None;

        match event {
            Some(TrayEvent::Unavailable) => self.available = false,
            Some(TrayEvent::Available) => self.available = true,
            _ => {}
        }
        event
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
struct NativeTray {
    _icon: tray_icon::TrayIcon,
    tray_id: tray_icon::TrayIconId,
    show_menu_id: tray_icon::menu::MenuId,
    exit_menu_id: tray_icon::menu::MenuId,
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl NativeTray {
    fn new(
        application_id: &str,
        application_name: &str,
        icon_png: &[u8],
    ) -> Result<Self, TrayError> {
        use tray_icon::{
            Icon, TrayIconBuilder,
            menu::{Menu, MenuItem},
        };

        let image = image::load_from_memory_with_format(icon_png, image::ImageFormat::Png)
            .map_err(|error| TrayError::new(format!("无法解码托盘 PNG：{error}")))?
            .into_rgba8();
        let (width, height) = image.dimensions();
        let icon = Icon::from_rgba(image.into_raw(), width, height)
            .map_err(|error| TrayError::new(format!("无法创建原生托盘图标：{error}")))?;
        let show_menu_id = tray_icon::menu::MenuId::new(format!("{application_id}.show"));
        let exit_menu_id = tray_icon::menu::MenuId::new(format!("{application_id}.exit"));
        let show = MenuItem::with_id(show_menu_id.clone(), "显示窗口", true, None);
        let exit = MenuItem::with_id(exit_menu_id.clone(), "退出", true, None);
        let menu = Menu::with_items(&[&show, &exit])
            .map_err(|error| TrayError::new(format!("无法创建托盘菜单：{error}")))?;
        let tray_id = tray_icon::TrayIconId::new(application_id);
        let native = TrayIconBuilder::new()
            .with_id(tray_id.clone())
            .with_tooltip(application_name)
            .with_icon(icon)
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .build()
            .map_err(|error| TrayError::new(format!("无法注册系统托盘：{error}")))?;

        Ok(Self {
            _icon: native,
            tray_id,
            show_menu_id,
            exit_menu_id,
        })
    }

    fn try_recv(&self) -> Option<TrayEvent> {
        use tray_icon::{MouseButton, MouseButtonState, TrayIconEvent, menu::MenuEvent};

        while let Ok(event) = MenuEvent::receiver().try_recv() {
            if event.id == self.show_menu_id {
                return Some(TrayEvent::ActivateWindowGroup);
            }
            if event.id == self.exit_menu_id {
                return Some(TrayEvent::ExitApplication);
            }
        }
        while let Ok(event) = TrayIconEvent::receiver().try_recv() {
            if let TrayIconEvent::Click {
                id,
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
                && id == self.tray_id
            {
                return Some(TrayEvent::ActivateWindowGroup);
            }
        }
        None
    }
}

#[cfg(target_os = "linux")]
struct LinuxTrayController {
    _handle: ksni::blocking::Handle<LinuxTray>,
    receiver: std::sync::mpsc::Receiver<TrayEvent>,
}

#[cfg(target_os = "linux")]
impl LinuxTrayController {
    fn new(
        application_id: &str,
        application_name: &str,
        icon_png: &[u8],
    ) -> Result<Self, TrayError> {
        use ksni::blocking::TrayMethods as _;

        let image = image::load_from_memory_with_format(icon_png, image::ImageFormat::Png)
            .map_err(|error| TrayError::new(format!("无法解码托盘 PNG：{error}")))?
            .into_rgba8();
        let (width, height) = image.dimensions();
        let mut data = image.into_raw();
        for pixel in data.chunks_exact_mut(4) {
            pixel.rotate_right(1);
        }
        let icon = ksni::Icon {
            width: i32::try_from(width)
                .map_err(|_| TrayError::new("托盘图标宽度超出 Linux SNI 范围"))?,
            height: i32::try_from(height)
                .map_err(|_| TrayError::new("托盘图标高度超出 Linux SNI 范围"))?,
            data,
        };
        let (sender, receiver) = std::sync::mpsc::channel();
        let tray = LinuxTray {
            id: sanitize_sni_id(application_id),
            title: application_name.to_owned(),
            icon,
            sender,
        };
        let handle = tray
            .spawn()
            .map_err(|error| TrayError::new(format!("无法注册 Linux SNI 托盘：{error:?}")))?;
        Ok(Self {
            _handle: handle,
            receiver,
        })
    }

    fn try_recv(&self) -> Option<TrayEvent> {
        self.receiver.try_recv().ok()
    }
}

#[cfg(target_os = "linux")]
fn sanitize_sni_id(application_id: &str) -> String {
    let id = application_id
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '_' {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    if id.is_empty() {
        "nexora".to_owned()
    } else {
        id
    }
}

#[cfg(target_os = "linux")]
struct LinuxTray {
    id: String,
    title: String,
    icon: ksni::Icon,
    sender: std::sync::mpsc::Sender<TrayEvent>,
}

#[cfg(target_os = "linux")]
impl ksni::Tray for LinuxTray {
    fn id(&self) -> String {
        self.id.clone()
    }

    fn title(&self) -> String {
        self.title.clone()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        vec![self.icon.clone()]
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        let _ = self.sender.send(TrayEvent::ActivateWindowGroup);
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::StandardItem;

        vec![
            StandardItem {
                label: "显示窗口".to_owned(),
                activate: Box::new(|tray: &mut Self| {
                    let _ = tray.sender.send(TrayEvent::ActivateWindowGroup);
                }),
                ..Default::default()
            }
            .into(),
            ksni::MenuItem::Separator,
            StandardItem {
                label: "退出".to_owned(),
                icon_name: "application-exit".to_owned(),
                activate: Box::new(|tray: &mut Self| {
                    let _ = tray.sender.send(TrayEvent::ExitApplication);
                }),
                ..Default::default()
            }
            .into(),
        ]
    }

    fn watcher_online(&self) {
        let _ = self.sender.send(TrayEvent::Available);
    }

    fn watcher_offline(&self, _reason: ksni::OfflineReason) -> bool {
        let _ = self.sender.send(TrayEvent::Unavailable);
        true
    }
}
