use crate::{AppWindow, BatchRow, controller::App};
use opensesame::{
    i18n::Message,
    model::{Appearance, Config, KnockStep, Protocol, Server},
    msg,
    storage::ConfigStore,
};
use slint::{
    ComponentHandle, Model, ModelRc, PhysicalSize, Rgb8Pixel, VecModel,
    platform::{
        Platform, WindowAdapter,
        software_renderer::{MinimalSoftwareWindow, RepaintBufferType},
    },
};
use std::{path::PathBuf, rc::Rc};

struct PreviewPlatform {
    window: Rc<MinimalSoftwareWindow>,
}

impl Platform for PreviewPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(self.window.clone())
    }
}

fn platform() -> Result<Rc<MinimalSoftwareWindow>, Box<dyn std::error::Error>> {
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(PreviewPlatform {
        window: window.clone(),
    }))?;
    window.set_size(PhysicalSize::new(1120, 720));
    Ok(window)
}

fn fixture() -> Config {
    let sequence = vec![
        KnockStep {
            port: 7000,
            protocol: Protocol::Tcp,
        },
        KnockStep {
            port: 8000,
            protocol: Protocol::Udp,
        },
        KnockStep {
            port: 9000,
            protocol: Protocol::Tcp,
        },
        KnockStep {
            port: 22222,
            protocol: Protocol::Udp,
        },
    ];
    let entries = [
        ("家庭 NAS", "nas.home.example", 22, Protocol::Tcp),
        ("云主机 HK", "203.0.113.24", 22, Protocol::Tcp),
        ("办公室网关", "gw.office.example", 51820, Protocol::Udp),
        ("测试机", "192.168.1.50", 8080, Protocol::Tcp),
    ];
    let servers: Vec<_> = entries
        .into_iter()
        .enumerate()
        .map(|(index, (name, host, port, protocol))| Server {
            name: name.into(),
            host: host.into(),
            service_port: port,
            service_protocol: protocol,
            open_sequence: if index == 2 {
                sequence[..3].to_vec()
            } else {
                sequence.clone()
            },
            ..Server::default()
        })
        .collect();
    Config {
        current_server: Some(servers[0].id.clone()),
        servers,
        ..Config::default()
    }
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let output = args
        .iter()
        .position(|arg| arg == "--screenshot")
        .and_then(|index| args.get(index + 1))
        .ok_or("缺少截图输出路径")?;
    let scene = args
        .iter()
        .position(|arg| arg == "--scene")
        .and_then(|index| args.get(index + 1))
        .map(String::as_str)
        .unwrap_or("main-dark");
    let window = platform()?;
    let ui = AppWindow::new()?;
    let mut config = if scene.starts_with("empty") {
        Config::default()
    } else {
        fixture()
    };
    let language = args
        .iter()
        .position(|arg| arg == "--language")
        .and_then(|index| args.get(index + 1))
        .map(String::as_str)
        .unwrap_or("zh-CN");
    let language = opensesame::i18n::resolve(language);
    config.settings.language = language.into();
    if scene.contains("-large-") {
        config.settings.font_size = 18;
    }
    if scene.starts_with("protocol") {
        for (step, port) in config.servers[0]
            .open_sequence
            .iter_mut()
            .zip([10240, 40960, 20480, 22222])
        {
            step.port = port;
        }
    }
    // 预览数据与真实 AppData 完全隔离，并禁止发出网络请求。
    let app = App::new(
        &ui,
        ConfigStore {
            path: PathBuf::from(if cfg!(windows) {
                r"%APPDATA%\OpenSesame\config.json"
            } else if cfg!(target_os = "macos") {
                "~/Library/Application Support/OpenSesame/config.json"
            } else {
                "~/.config/OpenSesame/config.json"
            }),
        },
        config,
        None,
        true,
    )?;
    app.borrow_mut().preview_scene(scene);
    if scene.starts_with("editor-error") {
        ui.set_form_host("https://example.com/path".into());
        ui.invoke_save_server();
    }
    if scene.starts_with("batch-service") {
        ui.invoke_edit_batch();
        ui.set_update_sequence(false);
        ui.set_update_service(true);
    }
    if scene.starts_with("batch-progress") {
        ui.set_batch_view(true);
        ui.set_batch_running(true);
        ui.set_batch_counter(
            msg!("{sent} / {total} sent", "sent" => 1, "total" => 2)
                .render(language)
                .into(),
        );
        ui.set_batch_rows(ModelRc::new(VecModel::from(vec![
            BatchRow {
                name: "云主机 HK".into(),
                host: "203.0.113.24".into(),
                stage: msg!("Sequence sent · Steps: {current} / {total}", "current" => 4, "total" => 4).render(language).into(),
                status: Message::from("Checking").render(language).into(),
                progress: 1.0,
                kind: 1,
            },
            BatchRow {
                name: "办公室网关".into(),
                host: "gw.office.example".into(),
                stage: msg!("Sending step {current} / {total}", "current" => 2, "total" => 3).render(language).into(),
                status: msg!("Check {protocol} {port} after knocking", "protocol" => "UDP", "port" => 51820).render(language).into(),
                progress: 2.0 / 3.0,
                kind: 1,
            },
        ])));
    }
    ui.show()?;
    window.set_size(PhysicalSize::new(1120, 720));
    slint::platform::update_timers_and_animations();
    if scene.starts_with("editor-service") || scene.starts_with("settings-font") {
        window.dispatch_event(slint::platform::WindowEvent::PointerScrolled {
            position: slint::LogicalPosition::new(800.0, 530.0),
            delta_x: 0.0,
            delta_y: -600.0,
        });
        slint::platform::update_timers_and_animations();
    }
    let mut pixels = vec![Rgb8Pixel::default(); 1120 * 720];
    window.draw_if_needed(|renderer| {
        renderer.render(&mut pixels, 1120);
    });
    let bytes: Vec<u8> = pixels
        .into_iter()
        .flat_map(|pixel| [pixel.r, pixel.g, pixel.b])
        .collect();
    if let Some(parent) = std::path::Path::new(output).parent() {
        std::fs::create_dir_all(parent)?;
    }
    image::save_buffer(output, &bytes, 1120, 720, image::ColorType::Rgb8)?;
    ui.hide()?;
    println!("已保存界面预览：{output}");
    Ok(())
}

pub fn self_test() -> Result<(), Box<dyn std::error::Error>> {
    let window = platform()?;
    let ui = AppWindow::new()?;
    let path = std::env::current_dir()?
        .join(".design")
        .join(format!("qa-{}.json", opensesame::model::new_id()));
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    let mut config = Config::default();
    config.settings.language = "zh-CN".into();
    config.settings.auto_check = false;
    config.settings.font_family = "OpenSesame Missing QA Font".into();
    let app = App::new(&ui, ConfigStore { path: path.clone() }, config, None, false)?;
    assert_eq!(
        ui.global::<crate::Palette>().get_font_family(),
        opensesame::fonts::default_family()
    );
    ui.show()?;
    window.set_size(PhysicalSize::new(1120, 720));
    slint::platform::update_timers_and_animations();
    // 直接向离屏组件派发指针事件，验证设计稿中“新增”按钮的实际点击路径。
    let position = slint::LogicalPosition::new(260.0, 78.0);
    window.dispatch_event(slint::platform::WindowEvent::PointerPressed {
        position,
        button: slint::platform::PointerEventButton::Left,
    });
    window.dispatch_event(slint::platform::WindowEvent::PointerReleased {
        position,
        button: slint::platform::PointerEventButton::Left,
    });
    assert_eq!(ui.get_dialog(), 1);
    ui.set_form_name("回环测试".into());
    ui.set_form_host("127.0.0.1".into());
    ui.set_form_port(port.to_string().into());
    ui.set_form_check_interval("1".into());
    ui.invoke_step_port(0, port.to_string().into());
    ui.invoke_save_server();
    assert_eq!(ui.get_dialog(), 0, "{}", ui.get_form_error());
    let saved = ConfigStore { path: path.clone() }.load()?;
    assert_eq!(saved.servers.len(), 1);
    let id = saved.servers[0].id.clone();
    ui.invoke_open_server();
    for _ in 0..100 {
        app.borrow_mut().tick();
        if ui.get_port_state() == "TCP 可连接" {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(ui.get_port_state(), "TCP 可连接");
    ui.invoke_toggle_server(id.clone().into());
    ui.invoke_edit_batch();
    ui.set_update_sequence(false);
    ui.set_update_service(true);
    ui.set_form_port("443".into());
    ui.invoke_save_batch();
    assert_eq!(ui.get_dialog(), 0, "{}", ui.get_form_error());
    let saved = ConfigStore { path: path.clone() }.load()?;
    assert_eq!(saved.servers[0].service_port, 443);
    assert_eq!(saved.servers[0].open_sequence[0].port, port);
    ui.invoke_open_settings();
    ui.set_settings_theme(2);
    ui.invoke_save_settings();
    assert!(!ui.global::<crate::Palette>().get_dark());
    assert_eq!(
        ConfigStore { path: path.clone() }
            .load()?
            .settings
            .appearance,
        Appearance::Light
    );
    verify_font_settings(&ui, &path)?;
    ui.invoke_edit_server();
    ui.set_form_port(port.to_string().into());
    ui.invoke_save_server();
    ui.invoke_open_settings();
    ui.set_settings_check(true);
    ui.set_settings_all(true);
    ui.invoke_save_settings();
    for _ in 0..150 {
        app.borrow_mut().tick();
        if ui.get_port_state() == "TCP 可连接" {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(ui.get_port_state(), "TCP 可连接");
    let previous = ui.get_last_check();
    for _ in 0..150 {
        app.borrow_mut().tick();
        if ui.get_last_check() != previous {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_ne!(ui.get_last_check(), previous, "周期检测应持续刷新");
    ui.invoke_add_server();
    ui.set_form_name("第二台回环测试".into());
    ui.set_form_host("127.0.0.1".into());
    ui.set_form_port(port.to_string().into());
    ui.invoke_step_port(0, port.to_string().into());
    ui.invoke_save_server();
    assert_eq!(ui.get_dialog(), 0, "{}", ui.get_form_error());
    let saved = ConfigStore { path: path.clone() }.load()?;
    let second_id = saved.servers[1].id.clone();
    for _ in 0..100 {
        app.borrow_mut().tick();
        if ui.get_servers().iter().all(|row| row.status == "可连接") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(
        ui.get_servers().iter().all(|row| row.status == "可连接"),
        "全部服务器应独立检测"
    );
    ui.invoke_toggle_server(second_id.into());
    ui.invoke_open_batch();
    assert!(ui.get_batch_running());
    for _ in 0..150 {
        app.borrow_mut().tick();
        if !ui.get_batch_running() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(!ui.get_batch_running());
    assert_eq!(ui.get_batch_counter(), "2 / 2 已发送");
    assert_eq!(ui.get_batch_rows().row_count(), 2);
    ui.invoke_open_settings();
    ui.set_settings_language(1);
    ui.invoke_save_settings();
    assert_eq!(ui.get_port_state(), "TCP reachable");
    assert!(ui.get_activity().contains("Sequence sent"));
    assert_eq!(ui.get_current_name(), "第二台回环测试");
    assert_eq!(
        ConfigStore { path: path.clone() }.load()?.settings.language,
        "en"
    );
    ui.invoke_edit_server();
    ui.set_form_host("https://example.com/path".into());
    ui.invoke_save_server();
    assert!(ui.get_form_error().contains("valid IP address"));
    ui.set_form_host("127.0.0.1".into());
    ui.invoke_save_server();
    ui.invoke_open_settings();
    ui.set_settings_language(2);
    ui.invoke_save_settings();
    assert_eq!(
        ConfigStore { path: path.clone() }.load()?.settings.language,
        "zh-CN"
    );
    ui.invoke_edit_server();
    ui.set_form_host("https://example.com/path".into());
    ui.invoke_save_server();
    assert!(ui.get_form_error().contains("有效的 IP"));
    ui.hide()?;
    drop(app);
    drop(ui);
    // 重新创建控制器，从磁盘恢复字体，验证重启后的显示设置。
    let restored = ConfigStore { path: path.clone() }.load()?;
    let restored_family = restored.settings.font_family.clone();
    let restored_ui = AppWindow::new()?;
    let restored_app = App::new(
        &restored_ui,
        ConfigStore { path: path.clone() },
        restored,
        None,
        true,
    )?;
    assert_eq!(restored_ui.global::<crate::Palette>().get_font_size(), 16);
    assert_eq!(
        restored_ui.global::<crate::Palette>().get_font_family(),
        restored_family
    );
    drop(restored_app);
    drop(restored_ui);
    std::fs::remove_file(path)?;
    println!(
        "界面操作、配置保存、字体选择与恢复、单台／批量敲门、周期检测及中英文切换与校验提示验证通过"
    );
    Ok(())
}

fn verify_font_settings(
    ui: &AppWindow,
    path: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let store = ConfigStore { path: path.into() };
    assert_eq!(
        store.load()?.settings.font_family,
        "OpenSesame Missing QA Font"
    );
    ui.invoke_open_settings();
    assert!(
        ui.get_font_labels()
            .iter()
            .last()
            .unwrap()
            .contains("未安装")
    );
    assert!(
        ui.get_font_values().row_count() > 2,
        "本机应能枚举到已安装字体"
    );
    let family = ui.get_font_values().row_data(1).unwrap();
    ui.set_settings_font(1);
    ui.set_settings_font_size(18);
    // 取消只丢弃表单，不改变已生效或已保存的字体。
    ui.set_dialog(0);
    assert_eq!(ui.global::<crate::Palette>().get_font_size(), 13);
    assert_eq!(store.load()?.settings.font_size, 13);
    ui.invoke_open_settings();
    assert_eq!(ui.get_settings_font_size(), 13);
    ui.set_settings_font(1);
    ui.set_settings_font_size(18);
    ui.invoke_save_settings();
    assert_eq!(ui.get_dialog(), 0, "{}", ui.get_form_error());
    assert_eq!(ui.global::<crate::Palette>().get_font_family(), family);
    assert_eq!(ui.global::<crate::Palette>().get_font_size(), 18);
    assert!((ui.global::<crate::Palette>().get_font_scale() - 18.0 / 13.0).abs() < 0.001);
    assert_eq!(store.load()?.settings.font_family, family.as_str());
    assert_eq!(store.load()?.settings.font_size, 18);
    ui.invoke_open_settings();
    ui.set_settings_font_size(19);
    ui.invoke_save_settings();
    assert_eq!(ui.get_dialog(), 3);
    assert!(!ui.get_form_error().is_empty());
    assert_eq!(store.load()?.settings.font_size, 18);
    ui.invoke_reset_font_settings();
    ui.invoke_save_settings();
    assert_eq!(ui.get_dialog(), 0, "{}", ui.get_form_error());
    assert_eq!(
        ui.global::<crate::Palette>().get_font_family(),
        opensesame::fonts::default_family()
    );
    assert_eq!(store.load()?.settings.font_family, "");
    assert_eq!(store.load()?.settings.font_size, 13);
    ui.invoke_open_settings();
    ui.set_settings_font(1);
    ui.set_settings_font_size(16);
    ui.invoke_save_settings();
    Ok(())
}
