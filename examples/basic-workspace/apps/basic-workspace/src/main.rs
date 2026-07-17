mod features;

use nexora::{Application as _, ApplicationOptions};

struct DesktopApplication;

impl nexora::Application for DesktopApplication {
    fn options(&self) -> ApplicationOptions {
        ApplicationOptions::new()
            .application_name("basic-workspace")
            .initial_path("/")
            .window_size(900.0, 640.0)
    }
}

fn main() -> Result<(), nexora::ApplicationError> {
    DesktopApplication.run()
}
