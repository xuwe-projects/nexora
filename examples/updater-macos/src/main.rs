use gpui::{App, Context, IntoElement, Window, div, prelude::*, px};
use gpui_component::{
    ActiveTheme as _, IconName, Sizable as _, StyledExt as _,
    button::{Button, ButtonVariants as _},
    h_flex, v_flex,
};
use nexora::{
    Application as _, ApplicationLogo, ApplicationOptions, FeatureElement,
    NavigationContextExt as _, desktop,
};

#[derive(Default, nexora::Feature)]
#[nexora(
    title = "更新程序",
    path = "/",
    section = "Updater",
    icon = "rotate-ccw",
    order = 0
)]
struct UpdaterFeature;

impl FeatureElement for UpdaterFeature {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let size = theme::component_size(cx);
        let update_button = desktop::check_for_updates_button("updater-example-check", cx)
            .map(|button| button.primary().with_size(size));
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
            .child(format!("Cargo 版本：{}", env!("CARGO_PKG_VERSION")))
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
                    .children(update_button),
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

struct ExampleApplication {
    updater: desktop::UpdateConfig,
}

impl nexora::Application for ExampleApplication {
    const PACKAGE_NAME: &'static str = env!("CARGO_PKG_NAME");

    fn options(&self) -> ApplicationOptions {
        ApplicationOptions::new()
            .application_name("macOS 更新程序示例")
            .application_logo(ApplicationLogo::png(include_bytes!(
                "../assets/logos/updater-macos/logo-icon-128.png"
            )))
            .initial_path("/")
            .window_size(960.0, 640.0)
    }

    fn initialize(&mut self, cx: &mut App) {
        desktop::install_updater(self.updater.clone(), cx)
            .expect("example 只能安装一份 updater 配置");
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let updater = desktop::UpdateConfig::from_current_bundle()?.with_health_report_on_launch(
        option_env!("NEXORA_EXAMPLE_HEALTH_FAILURE") != Some("before-health"),
    );
    ExampleApplication { updater }.run()?;
    Ok(())
}
