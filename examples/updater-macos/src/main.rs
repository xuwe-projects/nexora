use gpui::{Context, IntoElement, Window, div, prelude::*, px};
use gpui_component::{
    ActiveTheme as _, IconName, Sizable as _, StyledExt as _, WindowExt as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    notification::Notification,
    v_flex,
};
use nexora::{Application as _, ApplicationOptions, FeatureElement, NavigationContextExt as _};

#[derive(Default, nexora::Feature)]
#[nexora(
    title = "更新程序",
    path = "/",
    section = "Updater",
    icon = "rotate-ccw",
    order = 0
)]
struct UpdaterFeature {
    update_config: Option<Result<updater::UpdateConfig, String>>,
}

impl FeatureElement for UpdaterFeature {
    fn initialize(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if option_env!("NEXORA_EXAMPLE_HEALTH_FAILURE") != Some("before-health") {
            window.defer(cx, |_, _| {
                _ = updater::report_health_from_env_args();
            });
        }
        self.update_config =
            Some(updater::UpdateConfig::from_current_bundle().map_err(|error| error.to_string()));
        if let Some(Ok(config)) = self.update_config.clone() {
            updater::start_update_check_on_launch(config, window, cx);
        }
    }

    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let size = theme::component_size(cx);
        let release = self.update_config.as_ref().map_or_else(
            || "更新配置尚未加载".to_owned(),
            |config| match config {
                Ok(config) => format!(
                    "{} / build {}",
                    config.current_version(),
                    config.current_build_number()
                ),
                Err(_) => "更新配置不可用".to_owned(),
            },
        );
        let update_config = self.update_config.clone();
        v_flex()
            .size_full()
            .p_6()
            .gap_4()
            .child(
                div()
                    .text_xl()
                    .font_semibold()
                    .child("macOS 更新程序示例"),
            )
            .child(format!("当前运行版本：{release}"))
            .child(format!(
                "故障注入：{}",
                option_env!("NEXORA_EXAMPLE_HEALTH_FAILURE").unwrap_or("off")
            ))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("updater-example-open-window")
                            .icon(IconName::PanelRightOpen)
                            .label("第二窗口")
                            .with_size(size)
                            .on_click(|_, _, cx| {
                                _ = cx.navigate("/second-window");
                            }),
                    )
                    .child(
                        Button::new("updater-example-check")
                            .primary()
                            .icon(IconName::CircleCheck)
                            .label("检查更新")
                            .with_size(size)
                            .on_click(move |_, window, cx| match update_config.clone() {
                                Some(Ok(config)) => {
                                    updater::open_update_dialog(config, window, cx);
                                }
                                Some(Err(error)) => window.push_notification(
                                    Notification::error(error).title("更新配置错误"),
                                    cx,
                                ),
                                None => window.push_notification(
                                    Notification::error("更新配置尚未加载")
                                        .title("暂时无法检查更新"),
                                    cx,
                                ),
                            }),
                    ),
            )
            .child(
                div()
                    .rounded_md()
                    .border_1()
                    .border_color(cx.theme().border)
                    .p_4()
                    .max_w(px(680.0))
                    .child("这个 example 用本地 RustFS 发布源验证可选更新、强制更新、sidecar 替换和健康失败回滚。"),
            )
    }
}

#[derive(Default, nexora::Window)]
#[nexora(title = "第二窗口", path = "/second-window")]
struct SecondWindow;

impl nexora::WindowElement for SecondWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .p_5()
            .gap_3()
            .child(div().text_lg().font_semibold().child("第二原生窗口"))
            .child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child("强制更新门禁测试会确认这里不能绕过主窗口持有的更新 Dialog。"),
            )
    }
}

struct ExampleApplication;

impl nexora::Application for ExampleApplication {
    fn options(&self) -> ApplicationOptions {
        ApplicationOptions::new()
            .application_name("macOS 更新程序示例")
            .initial_path("/")
            .window_size(960.0, 640.0)
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    ExampleApplication.run()?;
    Ok(())
}
