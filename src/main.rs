#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod controller;
#[cfg(feature = "qa")]
mod preview;

slint::include_modules!();

fn main() {
    if let Err(error) = run() {
        let preference = opensesame::storage::ConfigStore::app_data()
            .and_then(|store| store.load())
            .map(|config| config.settings.language)
            .unwrap_or_else(|_| "system".into());
        let language = opensesame::i18n::resolve(&preference);
        let detail = error
            .downcast_ref::<opensesame::i18n::Message>()
            .map(|message| message.render(language))
            .unwrap_or_else(|| error.to_string());
        let message = opensesame::msg!("OpenSesame could not start: {error}", "error" => detail)
            .render(language);
        eprintln!("{message}");
        #[cfg(windows)]
        if !std::env::args()
            .any(|argument| matches!(argument.as_str(), "--screenshot" | "--self-test"))
        {
            let message: Vec<u16> = format!("{message}\0").encode_utf16().collect();
            let title: Vec<u16> = "OpenSesame\0".encode_utf16().collect();
            // 发布版没有控制台，初始化失败时仍需向用户显示原因。
            unsafe {
                windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW(
                    std::ptr::null_mut(),
                    message.as_ptr(),
                    title.as_ptr(),
                    0x10,
                );
            }
        }
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(feature = "qa")]
    if std::env::args().any(|argument| argument == "--screenshot") {
        return preview::run();
    }
    #[cfg(feature = "qa")]
    if std::env::args().any(|argument| argument == "--self-test") {
        return preview::self_test();
    }

    let ui = AppWindow::new()?;
    let store = opensesame::storage::ConfigStore::app_data()?;
    let (config, load_error) = match store.load() {
        Ok(config) => (config, None),
        Err(error) => (opensesame::model::Config::default(), Some(error)),
    };
    let app = controller::App::new(&ui, store, config, load_error, false)?;
    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_millis(200),
        move || app.borrow_mut().tick(),
    );
    ui.run()?;
    drop(timer);
    Ok(())
}
