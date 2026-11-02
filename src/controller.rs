use crate::{AppWindow, BatchRow, Palette, ServerRow, StepRow};
use opensesame::{
    fonts,
    i18n::{self, Message},
    model::{self, Appearance, BatchUpdate, Config, KnockStep, Protocol, Server},
    msg,
    network::{self, PortState, ProbeResult},
    storage::ConfigStore,
};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    net::IpAddr,
    rc::Rc,
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};
use tokio::{runtime::Runtime, sync::Semaphore, task::JoinHandle};

struct ServerState {
    generation: u64,
    knock: Option<JoinHandle<()>>,
    probe: Option<JoinHandle<()>>,
    state: PortState,
    detail: Message,
    last_check: String,
    next_check: Instant,
    progress: usize,
    sent: bool,
    knock_error: Option<Message>,
    activity: Message,
    pinned_ip: Option<IpAddr>,
}

impl ServerState {
    fn new(generation: u64) -> Self {
        Self {
            generation,
            knock: None,
            probe: None,
            state: PortState::Idle,
            detail: Message::default(),
            last_check: "—".into(),
            next_check: Instant::now(),
            progress: 0,
            sent: false,
            knock_error: None,
            activity: Message::default(),
            pinned_ip: None,
        }
    }
    fn cancel(&mut self) {
        if let Some(task) = self.knock.take() {
            task.abort();
        }
        if let Some(task) = self.probe.take() {
            task.abort();
        }
    }
}

enum Event {
    Progress {
        id: String,
        generation: u64,
        step: usize,
    },
    Knocked {
        id: String,
        generation: u64,
        result: Result<(IpAddr, Duration), Message>,
    },
    Probed {
        id: String,
        generation: u64,
        result: ProbeResult,
    },
}

pub struct App {
    language: &'static str,
    font_families: Vec<String>,
    config: Config,
    store: ConfigStore,
    read_only: bool,
    states: HashMap<String, ServerState>,
    checked: HashSet<String>,
    batch: Vec<String>,
    editing_id: Option<String>,
    form_steps: Rc<VecModel<StepRow>>,
    runtime: Runtime,
    permits: Arc<Semaphore>,
    client: network::Client,
    sender: mpsc::Sender<Event>,
    receiver: mpsc::Receiver<Event>,
    generation: u64,
    ui: slint::Weak<AppWindow>,
    preview: bool,
    banner: Message,
    last_tick: Instant,
}

impl App {
    pub fn new(
        ui: &AppWindow,
        store: ConfigStore,
        config: Config,
        load_error: Option<Message>,
        preview: bool,
    ) -> Result<Rc<RefCell<Self>>, Message> {
        let (sender, receiver) = mpsc::channel();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .map_err(|error| Message::from(error.to_string()))?;
        let language = i18n::resolve(&config.settings.language);
        slint::select_bundled_translation(language)
            .map_err(|error| Message::from(error.to_string()))?;
        ui.set_config_path(store.path.to_string_lossy().into_owned().into());
        let mut app = Self {
            language,
            font_families: fonts::installed_families(),
            config,
            store,
            read_only: load_error.is_some(),
            states: HashMap::new(),
            checked: HashSet::new(),
            batch: vec![],
            editing_id: None,
            form_steps: Rc::new(VecModel::from(vec![])),
            runtime,
            permits: Arc::new(Semaphore::new(16)),
            client: network::Client::default(),
            sender,
            receiver,
            generation: 0,
            ui: ui.as_weak(),
            preview,
            banner: load_error.unwrap_or_default(),
            last_tick: Instant::now() - Duration::from_secs(2),
        };
        if !app
            .config
            .servers
            .iter()
            .any(|server| Some(&server.id) == app.config.current_server.as_ref())
        {
            app.config.current_server = app.config.servers.first().map(|server| server.id.clone());
        }
        let ids: Vec<_> = app
            .config
            .servers
            .iter()
            .map(|server| server.id.clone())
            .collect();
        for id in ids {
            app.reset_state(&id);
        }
        app.refresh();
        let app = Rc::new(RefCell::new(app));
        Self::bind(&app, ui);
        if app.borrow().config.settings.auto_knock && !preview {
            let id = app.borrow().config.current_server.clone();
            if let Some(id) = id {
                app.borrow_mut().start_knock(&id);
            }
        }
        Ok(app)
    }

    fn bind(app: &Rc<RefCell<Self>>, ui: &AppWindow) {
        macro_rules! action {
            ($method:ident, $body:expr) => {{
                let app = app.clone();
                ui.$method(move || {
                    let mut state = app.borrow_mut();
                    ($body)(&mut state);
                    state.refresh();
                });
            }};
        }
        {
            let app = app.clone();
            ui.on_select_server(move |id| {
                let mut app = app.borrow_mut();
                if !app
                    .config
                    .servers
                    .iter()
                    .any(|server| server.id == id.as_str())
                {
                    return;
                }
                let mut config = app.config.clone();
                config.current_server = Some(id.to_string());
                if app.persist(config) {
                    app.ui.upgrade().unwrap().set_batch_view(false);
                    if !app.config.settings.check_all {
                        app.restart_checks();
                    }
                    if let Some(state) = app.states.get_mut(id.as_str()) {
                        state.next_check = Instant::now();
                    }
                }
                app.refresh();
            });
        }
        {
            let app = app.clone();
            ui.on_toggle_server(move |id| {
                let mut app = app.borrow_mut();
                if !app.checked.remove(id.as_str()) {
                    app.checked.insert(id.to_string());
                }
                app.refresh();
            });
        }
        action!(on_add_server, |app: &mut App| {
            app.open_editor(false);
        });
        action!(on_edit_server, |app: &mut App| {
            app.open_editor(true);
        });
        action!(on_delete_server, |app: &mut App| {
            app.delete_current();
        });
        action!(on_save_server, |app: &mut App| {
            if let Err(error) = app.save_editor() {
                app.form_error(error);
            }
        });
        action!(on_edit_batch, |app: &mut App| {
            app.open_batch_editor();
        });
        action!(on_save_batch, |app: &mut App| {
            if let Err(error) = app.save_batch() {
                app.form_error(error);
            }
        });
        action!(on_open_server, |app: &mut App| {
            if let Some(id) = app.config.current_server.clone() {
                app.start_knock(&id);
            }
        });
        action!(on_open_batch, |app: &mut App| {
            let ids: Vec<_> = app
                .config
                .servers
                .iter()
                .filter(|server| app.checked.contains(&server.id))
                .map(|server| server.id.clone())
                .collect();
            if ids.is_empty() {
                return;
            }
            app.batch = ids.clone();
            if let Some(ui) = app.ui.upgrade() {
                ui.set_batch_view(true);
                ui.set_show_batch_summary(true);
            }
            for id in ids {
                app.start_knock(&id);
            }
        });
        action!(on_open_settings, |app: &mut App| {
            app.open_settings();
        });
        action!(on_save_settings, |app: &mut App| {
            if let Err(error) = app.save_settings() {
                app.form_error(error);
            }
        });
        {
            let app = app.clone();
            ui.on_toggle_auto(move |value| {
                let mut app = app.borrow_mut();
                let mut config = app.config.clone();
                config.settings.auto_check = value;
                if app.persist(config) {
                    app.restart_checks();
                }
                app.refresh();
            });
        }
        action!(on_add_step, |app: &mut App| {
            app.form_steps.push(StepRow {
                port: "".into(),
                udp: false,
            });
        });
        {
            let app = app.clone();
            ui.on_step_port(move |index, value| {
                let app = app.borrow();
                if let Some(mut row) = app.form_steps.row_data(index as usize) {
                    row.port = value;
                    app.form_steps.set_row_data(index as usize, row);
                }
            });
        }
        {
            let app = app.clone();
            ui.on_step_protocol(move |index, udp| {
                let app = app.borrow();
                if let Some(mut row) = app.form_steps.row_data(index as usize) {
                    row.udp = udp;
                    app.form_steps.set_row_data(index as usize, row);
                }
            });
        }
        {
            let app = app.clone();
            ui.on_move_step(move |index, direction| {
                let app = app.borrow();
                let mut rows: Vec<_> = app.form_steps.iter().collect();
                let target = index + direction;
                if index >= 0
                    && target >= 0
                    && (index as usize) < rows.len()
                    && (target as usize) < rows.len()
                {
                    rows.swap(index as usize, target as usize);
                    app.form_steps.set_vec(rows);
                }
            });
        }
        {
            let app = app.clone();
            ui.on_remove_step(move |index| {
                let app = app.borrow();
                if index >= 0 && (index as usize) < app.form_steps.row_count() {
                    app.form_steps.remove(index as usize);
                }
            });
        }
    }

    fn next_generation(&mut self) -> u64 {
        self.generation += 1;
        self.generation
    }

    fn reset_state(&mut self, id: &str) {
        if let Some(state) = self.states.get_mut(id) {
            state.cancel();
        }
        let generation = self.next_generation();
        self.states.insert(id.into(), ServerState::new(generation));
    }

    fn current(&self) -> Option<&Server> {
        self.config
            .servers
            .iter()
            .find(|server| Some(&server.id) == self.config.current_server.as_ref())
    }

    fn persist(&mut self, config: Config) -> bool {
        if self.read_only {
            self.banner =
                "The original configuration was preserved. Fix the file and restart the app."
                    .into();
            return false;
        }
        if !self.preview
            && let Err(error) = self.store.save(&config)
        {
            self.banner = error;
            return false;
        }
        self.config = config;
        self.banner.clear();
        true
    }

    fn set_steps(&mut self, steps: &[KnockStep]) {
        self.form_steps = Rc::new(VecModel::from(
            steps
                .iter()
                .map(|step| StepRow {
                    port: step.port.to_string().into(),
                    udp: step.protocol == Protocol::Udp,
                })
                .collect::<Vec<_>>(),
        ));
        if let Some(ui) = self.ui.upgrade() {
            ui.set_form_steps(ModelRc::from(self.form_steps.clone()));
        }
    }

    fn read_steps(&self) -> Result<Vec<KnockStep>, Message> {
        self.form_steps
            .iter()
            .map(|step| {
                Ok(KnockStep {
                    port: parse_port(&step.port)?,
                    protocol: protocol(step.udp),
                })
            })
            .collect()
    }

    fn open_editor(&mut self, editing: bool) {
        let server = if editing {
            let Some(server) = self.current() else {
                return;
            };
            server.clone()
        } else {
            Server {
                check_interval_secs: self.config.settings.check_interval_secs,
                check_timeout_secs: self.config.settings.check_timeout_secs,
                ..Server::default()
            }
        };
        self.editing_id = editing.then(|| server.id.clone());
        self.set_steps(&server.open_sequence);
        if let Some(ui) = self.ui.upgrade() {
            ui.set_editing(editing);
            ui.set_form_name(server.name.into());
            ui.set_form_host(server.host.into());
            ui.set_form_gap(server.interval_ms.to_string().into());
            ui.set_form_limit(server.sequence_timeout_secs.to_string().into());
            ui.set_form_port(server.service_port.to_string().into());
            ui.set_form_udp(server.service_protocol == Protocol::Udp);
            ui.set_form_check_interval(server.check_interval_secs.to_string().into());
            ui.set_form_check_timeout(server.check_timeout_secs.to_string().into());
            ui.set_form_error("".into());
            ui.set_dialog(1);
        }
    }

    fn save_editor(&mut self) -> Result<(), Message> {
        let ui = self.ui.upgrade().ok_or("The window is closed.")?;
        let server = Server {
            id: self.editing_id.clone().unwrap_or_else(model::new_id),
            name: ui.get_form_name().trim().into(),
            host: ui.get_form_host().trim().into(),
            open_sequence: self.read_steps()?,
            interval_ms: parse_number(&ui.get_form_gap(), "Step interval")?,
            sequence_timeout_secs: parse_number(&ui.get_form_limit(), "Sequence timeout")?,
            service_port: parse_port(&ui.get_form_port())?,
            service_protocol: protocol(ui.get_form_udp()),
            check_interval_secs: parse_number(&ui.get_form_check_interval(), "Check interval")?,
            check_timeout_secs: parse_number(&ui.get_form_check_timeout(), "Check timeout")?,
        };
        server.validate()?;
        let id = server.id.clone();
        let mut config = self.config.clone();
        if let Some(existing) = config.servers.iter_mut().find(|item| item.id == id) {
            *existing = server;
        } else {
            config.servers.push(server);
        }
        config.current_server = Some(id.clone());
        if !self.persist(config) {
            return Err(self.banner.clone());
        }
        self.reset_state(&id);
        ui.set_dialog(0);
        ui.set_batch_view(false);
        Ok(())
    }

    fn delete_current(&mut self) {
        let Some(id) = self.config.current_server.clone() else {
            return;
        };
        let mut config = self.config.clone();
        config.servers.retain(|server| server.id != id);
        config.current_server = config.servers.first().map(|server| server.id.clone());
        if !self.persist(config) {
            return;
        }
        if let Some(mut state) = self.states.remove(&id) {
            state.cancel();
        }
        self.checked.remove(&id);
        self.batch.retain(|item| item != &id);
        if let Some(ui) = self.ui.upgrade() {
            ui.set_dialog(0);
            ui.set_batch_view(false);
        }
    }

    fn open_batch_editor(&mut self) {
        let Some(server) = self
            .config
            .servers
            .iter()
            .find(|server| self.checked.contains(&server.id))
            .cloned()
        else {
            return;
        };
        self.set_steps(&server.open_sequence);
        if let Some(ui) = self.ui.upgrade() {
            ui.set_form_port(server.service_port.to_string().into());
            ui.set_form_udp(server.service_protocol == Protocol::Udp);
            ui.set_update_sequence(true);
            ui.set_update_service(false);
            ui.set_form_error("".into());
            ui.set_dialog(2);
        }
    }

    fn save_batch(&mut self) -> Result<(), Message> {
        let ui = self.ui.upgrade().ok_or("The window is closed.")?;
        let update = BatchUpdate {
            sequence: if ui.get_update_sequence() {
                Some(self.read_steps()?)
            } else {
                None
            },
            service: if ui.get_update_service() {
                Some((
                    parse_port(&ui.get_form_port())?,
                    protocol(ui.get_form_udp()),
                ))
            } else {
                None
            },
        };
        let ids: Vec<_> = self.checked.iter().cloned().collect();
        let mut config = self.config.clone();
        model::apply_batch(&mut config, &ids, &update)?;
        if !self.persist(config) {
            return Err(self.banner.clone());
        }
        for id in ids {
            self.reset_state(&id);
        }
        ui.set_dialog(0);
        Ok(())
    }

    fn open_settings(&self) {
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        let settings = &self.config.settings;
        ui.set_settings_knock(settings.auto_knock);
        ui.set_settings_check(settings.auto_check);
        ui.set_settings_all(settings.check_all);
        ui.set_settings_interval(settings.check_interval_secs.to_string().into());
        ui.set_settings_timeout(settings.check_timeout_secs.to_string().into());
        let mut font_labels = vec![msg!("Default font").render(self.language)];
        font_labels.extend(self.font_families.iter().cloned());
        let mut font_values = vec![fonts::default_family().to_owned()];
        font_values.extend(self.font_families.iter().cloned());
        let font_index = if settings.font_family.is_empty() {
            0
        } else if let Some(index) = self
            .font_families
            .iter()
            .position(|family| family.eq_ignore_ascii_case(&settings.font_family))
        {
            index + 1
        } else {
            font_labels.push(
                msg!("{font} (unavailable)", "font" => settings.font_family.as_str())
                    .render(self.language),
            );
            font_values.push(fonts::default_family().to_owned());
            font_labels.len() - 1
        };
        ui.set_font_labels(ModelRc::new(VecModel::from(
            font_labels.into_iter().map(Into::into).collect::<Vec<_>>(),
        )));
        ui.set_font_values(ModelRc::new(VecModel::from(
            font_values.into_iter().map(Into::into).collect::<Vec<_>>(),
        )));
        ui.set_settings_font(font_index as i32);
        ui.set_settings_font_size(settings.font_size.into());
        let labels = std::iter::once(msg!("Follow system").render(self.language))
            .chain(i18n::LANGUAGES.iter().map(|(_, name)| (*name).to_string()))
            .map(Into::into)
            .collect::<Vec<_>>();
        ui.set_language_labels(ModelRc::new(VecModel::from(labels)));
        ui.set_settings_language(if settings.language == i18n::SYSTEM {
            0
        } else {
            i18n::LANGUAGES
                .iter()
                .position(|(tag, _)| *tag == self.language)
                .map_or(1, |index| index as i32 + 1)
        });
        ui.set_settings_theme(match settings.appearance {
            Appearance::System => 0,
            Appearance::Dark => 1,
            Appearance::Light => 2,
        });
        ui.set_form_error("".into());
        ui.set_dialog(3);
    }

    fn save_settings(&mut self) -> Result<(), Message> {
        let ui = self.ui.upgrade().ok_or("The window is closed.")?;
        let interval = parse_number(&ui.get_settings_interval(), "Check interval")?;
        let timeout = parse_number(&ui.get_settings_timeout(), "Check timeout")?;
        model::validate_check_times(interval, timeout)?;
        let mut config = self.config.clone();
        let font_index = ui.get_settings_font();
        config.settings.font_family = if font_index == 0 {
            String::new()
        } else if font_index == self.font_families.len() as i32 + 1
            && !config.settings.font_family.is_empty()
        {
            // 未安装的已保存字体仍保留，避免修改其他设置时丢失跨设备字体偏好。
            config.settings.font_family.clone()
        } else {
            self.font_families
                .get((font_index - 1) as usize)
                .ok_or("Invalid font selection.")?
                .clone()
        };
        config.settings.font_size = ui
            .get_settings_font_size()
            .try_into()
            .map_err(|_| Message::from("Font size must be between 12 and 18 px."))?;
        // 全局设置更新默认值；跟随原默认值的服务器同步更新，单独定制的数值保留。
        for server in &mut config.servers {
            if server.check_interval_secs == config.settings.check_interval_secs {
                server.check_interval_secs = interval;
            }
            if server.check_timeout_secs == config.settings.check_timeout_secs {
                server.check_timeout_secs = timeout;
            }
        }
        config.settings.language = if ui.get_settings_language() == 0 {
            i18n::SYSTEM.into()
        } else {
            i18n::LANGUAGES
                .get((ui.get_settings_language() - 1) as usize)
                .ok_or("Invalid language selection.")?
                .0
                .into()
        };
        config.settings.auto_knock = ui.get_settings_knock();
        config.settings.auto_check = ui.get_settings_check();
        config.settings.check_all = ui.get_settings_all();
        config.settings.check_interval_secs = interval;
        config.settings.check_timeout_secs = timeout;
        config.settings.appearance = match ui.get_settings_theme() {
            1 => Appearance::Dark,
            2 => Appearance::Light,
            _ => Appearance::System,
        };
        if !self.persist(config) {
            return Err(self.banner.clone());
        }
        self.language = i18n::resolve(&self.config.settings.language);
        slint::select_bundled_translation(self.language)
            .map_err(|error| Message::from(error.to_string()))?;
        self.restart_checks();
        ui.set_dialog(0);
        Ok(())
    }

    fn restart_checks(&mut self) {
        let ids: Vec<_> = self.states.keys().cloned().collect();
        for id in ids {
            let generation = self.next_generation();
            if let Some(state) = self.states.get_mut(&id) {
                // 正在敲门时保留该操作的事件代次，其结束后会重新执行一次检测。
                if state.knock.is_none() {
                    if let Some(task) = state.probe.take() {
                        task.abort();
                    }
                    state.generation = generation;
                    if state.state == PortState::Checking {
                        state.state = PortState::Idle;
                    }
                }
                state.next_check = Instant::now();
            }
        }
    }

    fn form_error(&self, error: Message) {
        if let Some(ui) = self.ui.upgrade() {
            ui.set_form_error(error.render(self.language).into());
        }
    }

    fn start_knock(&mut self, id: &str) {
        if self.preview {
            return;
        }
        let Some(server) = self
            .config
            .servers
            .iter()
            .find(|server| server.id == id)
            .cloned()
        else {
            return;
        };
        if self
            .states
            .get(id)
            .is_some_and(|state| state.knock.is_some())
        {
            return;
        }
        if let Err(error) = server.validate() {
            self.banner = error;
            return;
        }
        self.reset_state(id);
        let state = self.states.get_mut(id).unwrap();
        let generation = state.generation;
        let sender = self.sender.clone();
        let permits = self.permits.clone();
        let client = self.client.clone();
        let id = id.to_string();
        state.knock = Some(self.runtime.spawn(async move {
            let Ok(_permit) = permits.acquire_owned().await else {
                return;
            };
            let result = client
                .execute(
                    &server.host,
                    &server.open_sequence,
                    Duration::from_millis(server.interval_ms),
                    Duration::from_secs(server.sequence_timeout_secs),
                    |step| {
                        let _ = sender.send(Event::Progress {
                            id: id.clone(),
                            generation,
                            step,
                        });
                    },
                )
                .await;
            let _ = sender.send(Event::Knocked {
                id,
                generation,
                result,
            });
        }));
    }

    fn start_probe(&mut self, id: &str, immediate: bool) {
        if self.preview {
            return;
        }
        let Some(server) = self
            .config
            .servers
            .iter()
            .find(|server| server.id == id)
            .cloned()
        else {
            return;
        };
        let Some(state) = self.states.get_mut(id) else {
            return;
        };
        if state.knock.is_some() || state.probe.is_some() {
            return;
        }
        if !immediate && state.next_check > Instant::now() {
            return;
        }
        state.state = PortState::Checking;
        // 仅敲门后的首次检测沿用同一 IP，后续周期重新解析，以支持动态域名。
        let pinned = state.pinned_ip.take();
        let generation = state.generation;
        let id = id.to_string();
        let sender = self.sender.clone();
        let permits = self.permits.clone();
        let client = self.client.clone();
        state.probe = Some(self.runtime.spawn(async move {
            let Ok(_permit) = permits.acquire_owned().await else {
                return;
            };
            let result = client.probe(&server, pinned).await;
            let _ = sender.send(Event::Probed {
                id,
                generation,
                result,
            });
        }));
    }

    pub fn tick(&mut self) {
        let mut refresh = false;
        let mut after_knock = vec![];
        while let Ok(event) = self.receiver.try_recv() {
            match event {
                Event::Progress {
                    id,
                    generation,
                    step,
                } => {
                    if let Some(state) = self.states.get_mut(&id)
                        && state.generation == generation
                    {
                        state.progress = step;
                        refresh = true;
                    }
                }
                Event::Knocked {
                    id,
                    generation,
                    result,
                } => {
                    if let Some(state) = self.states.get_mut(&id)
                        && state.generation == generation
                    {
                        state.knock.take();
                        match result {
                            Ok((ip, duration)) => {
                                state.sent = true;
                                state.pinned_ip = Some(ip);
                                state.activity = msg!("{time}  Sequence sent · Steps: {steps} · Time: {elapsed}s", "time" => now(), "steps" => state.progress, "elapsed" => format!("{:.1}", duration.as_secs_f32()));
                                after_knock.push(id);
                            }
                            Err(error) => {
                                state.knock_error = Some(error.clone());
                                state.activity =
                                    msg!("{time}  {error}", "time" => now(), "error" => error);
                            }
                        }
                        refresh = true;
                    }
                }
                Event::Probed {
                    id,
                    generation,
                    result,
                } => {
                    if let Some(state) = self.states.get_mut(&id)
                        && state.generation == generation
                    {
                        state.probe.take();
                        state.state = result.state;
                        state.detail = result.detail;
                        state.last_check = now();
                        let interval = self
                            .config
                            .servers
                            .iter()
                            .find(|server| server.id == id)
                            .map(|server| server.check_interval_secs)
                            .unwrap_or(30);
                        state.next_check = Instant::now() + Duration::from_secs(interval);
                        refresh = true;
                    }
                }
            }
        }
        for id in after_knock {
            self.start_probe(&id, true);
        }
        if self.config.settings.auto_check {
            let ids: Vec<_> = self
                .config
                .servers
                .iter()
                .filter(|server| {
                    self.config.settings.check_all
                        || Some(&server.id) == self.config.current_server.as_ref()
                })
                .map(|server| server.id.clone())
                .collect();
            for id in ids {
                self.start_probe(&id, false);
            }
        }
        if refresh || self.last_tick.elapsed() >= Duration::from_secs(1) {
            self.last_tick = Instant::now();
            self.refresh();
        }
    }

    pub fn refresh(&self) {
        let Some(ui) = self.ui.upgrade() else {
            return;
        };
        ui.global::<Palette>().set_font_family(
            fonts::resolve_family(&self.config.settings.font_family, &self.font_families).into(),
        );
        ui.global::<Palette>()
            .set_font_size(self.config.settings.font_size.into());
        ui.global::<Palette>()
            .set_appearance(match self.config.settings.appearance {
                Appearance::System => 0,
                Appearance::Dark => 1,
                Appearance::Light => 2,
            });
        let rows = self
            .config
            .servers
            .iter()
            .map(|server| {
                let state = self.states.get(&server.id);
                let (label, kind) = if let Some(state) = state {
                    if state.knock.is_some() {
                        (
                            msg!("Knocking {current}/{total}", "current" => state.progress, "total" => server.open_sequence.len()).render(self.language),
                            1,
                        )
                    } else if state.knock_error.is_some() {
                        (msg!("Send failed").render(self.language), 4)
                    } else {
                        (
                            Message::from(state.state.short_label(server.service_protocol)).render(self.language),
                            state.state.kind(),
                        )
                    }
                } else {
                    (msg!("Not checked").render(self.language), 0)
                };
                ServerRow {
                    id: server.id.clone().into(),
                    name: server.name.clone().into(),
                    host: server.host.clone().into(),
                    service: format!(
                        "{} {}",
                        server.service_protocol.label(),
                        server.service_port
                    )
                    .into(),
                    udp: server.service_protocol == Protocol::Udp,
                    checked: self.checked.contains(&server.id),
                    selected: Some(&server.id) == self.config.current_server.as_ref(),
                    status: label.into(),
                    kind,
                }
            })
            .collect::<Vec<_>>();
        ui.set_servers(ModelRc::new(VecModel::from(rows)));
        ui.set_selected_count(self.checked.len() as i32);
        ui.set_batch_targets(ModelRc::new(VecModel::from(
            self.config
                .servers
                .iter()
                .filter(|server| self.checked.contains(&server.id))
                .map(|server| server.name.clone().into())
                .collect::<Vec<_>>(),
        )));
        ui.set_auto_check(self.config.settings.auto_check);
        ui.set_banner(self.banner.render(self.language).into());
        let current = self.current();
        ui.set_has_current(current.is_some());
        if let Some(server) = current {
            ui.set_current_name(server.name.clone().into());
            ui.set_current_host(server.host.clone().into());
            ui.set_sequence(ModelRc::new(VecModel::from(
                server
                    .open_sequence
                    .iter()
                    .map(|step| StepRow {
                        port: step.port.to_string().into(),
                        udp: step.protocol == Protocol::Udp,
                    })
                    .collect::<Vec<_>>(),
            )));
            ui.set_check_cycle(
                msg!("Every {seconds}s", "seconds" => server.check_interval_secs)
                    .render(self.language)
                    .into(),
            );
            if let Some(state) = self.states.get(&server.id) {
                ui.set_knocking(state.knock.is_some());
                ui.set_knock_stage(stage(server, state).render(self.language).into());
                ui.set_knock_progress(
                    state.progress as f32 / server.open_sequence.len().max(1) as f32,
                );
                ui.set_port_state(
                    Message::from(state.state.label(server.service_protocol))
                        .render(self.language)
                        .into(),
                );
                ui.set_port_kind(state.state.kind());
                let detail = if state.detail.is_empty() {
                    Message::from("Waiting for the first check").render(self.language)
                } else {
                    state.detail.render(self.language)
                };
                ui.set_port_detail(
                    format!(
                        "{} / {} · {detail}",
                        server.service_port,
                        server.service_protocol.label()
                    )
                    .into(),
                );
                ui.set_last_check(state.last_check.clone().into());
                ui.set_next_check(if !self.config.settings.auto_check {
                    Message::from("Off").render(self.language).into()
                } else if state.knock.is_some() {
                    Message::from("Check after knocking").render(self.language).into()
                } else if state.probe.is_some() {
                    Message::from("Checking").render(self.language).into()
                } else {
                    msg!("In {seconds}s", "seconds" => state.next_check.saturating_duration_since(Instant::now()).as_secs() + 1).render(self.language)
                    .into()
                });
                ui.set_activity(state.activity.render(self.language).into());
            }
        }
        let mut sent = 0;
        let mut reachable = 0;
        let mut unknown = 0;
        let mut failed = 0;
        let mut running = false;
        let batch = self
            .batch
            .iter()
            .filter_map(|id| {
                let server = self.config.servers.iter().find(|server| &server.id == id)?;
                let state = self.states.get(id)?;
                if state.sent {
                    sent += 1;
                }
                if state.state == PortState::Reachable {
                    reachable += 1;
                }
                if state.state == PortState::Unknown {
                    unknown += 1;
                }
                if state.knock_error.is_some() {
                    failed += 1;
                }
                if state.knock.is_some() || state.probe.is_some() {
                    running = true;
                }
                let status = if state.knock.is_some() {
                    msg!("Check {protocol} {port} after knocking", "protocol" => server.service_protocol.label(), "port" => server.service_port).render(self.language)
                } else if let Some(error) = &state.knock_error {
                    error.render(self.language)
                } else {
                    format!(
                        "{} {} {} · {}",
                        server.service_protocol.label(),
                        server.service_port,
                        Message::from(state.state.short_label(server.service_protocol)).render(self.language),
                        if state.state == PortState::Unknown {
                            Message::from("No response received; this does not mean the port is closed.").render(self.language)
                        } else {
                            state.last_check.clone()
                        }
                    )
                };
                Some(BatchRow {
                    name: server.name.clone().into(),
                    host: server.host.clone().into(),
                    stage: stage(server, state).render(self.language).into(),
                    status: status.into(),
                    progress: state.progress as f32 / server.open_sequence.len().max(1) as f32,
                    kind: if state.knock.is_some() {
                        1
                    } else if state.knock_error.is_some() {
                        4
                    } else {
                        state.state.kind()
                    },
                })
            })
            .collect::<Vec<_>>();
        ui.set_batch_rows(ModelRc::new(VecModel::from(batch)));
        ui.set_batch_running(running);
        ui.set_batch_counter(
            msg!("{sent} / {total} sent", "sent" => sent, "total" => self.batch.len())
                .render(self.language)
                .into(),
        );
        let summary = msg!("Sent: {sent} · Reachable: {reachable} · Unknown: {unknown}", "sent" => sent, "reachable" => reachable, "unknown" => unknown).render(self.language);
        ui.set_batch_summary(
            (if failed > 0 {
                format!(
                    "{summary} · {}",
                    msg!("Failed: {count}", "count" => failed).render(self.language)
                )
            } else {
                summary
            })
            .into(),
        );
    }

    #[cfg(feature = "qa")]
    pub fn preview_scene(&mut self, scene: &str) {
        self.config.settings.appearance = if scene.ends_with("light") {
            Appearance::Light
        } else {
            Appearance::Dark
        };
        let ids: Vec<_> = self
            .config
            .servers
            .iter()
            .map(|server| server.id.clone())
            .collect();
        if ids.len() >= 4 {
            self.checked.extend([ids[1].clone(), ids[2].clone()]);
            for (index, id) in ids.iter().enumerate() {
                let state = self.states.get_mut(id).unwrap();
                state.state = match index {
                    0 => PortState::Reachable,
                    1 => PortState::TimedOut,
                    2 => PortState::Unknown,
                    _ => PortState::Checking,
                };
                state.last_check = "14:32:08".into();
                state.next_check = Instant::now() + Duration::from_secs(11);
                state.detail = if index == 0 {
                    "A connection was established; authentication was not checked.".into()
                } else {
                    "No response received; this does not mean the port is closed.".into()
                };
                if index == 0 {
                    state.sent = true;
                    state.progress = self.config.servers[index].open_sequence.len();
                    state.activity = msg!("{time}  Sequence sent · Steps: {steps} · Time: {elapsed}s", "time" => "14:32:05", "steps" => 4, "elapsed" => "0.9");
                }
            }
        }
        self.refresh();
        if scene.starts_with("editor") {
            self.open_editor(true);
        }
        if scene.starts_with("settings") {
            self.open_settings();
        }
        if scene.starts_with("batch-settings") {
            self.open_batch_editor();
        }
        if scene.starts_with("batch-result") {
            self.batch = ids.iter().skip(1).take(2).cloned().collect();
            for id in &self.batch {
                let state = self.states.get_mut(id).unwrap();
                state.sent = true;
                state.progress = self
                    .config
                    .servers
                    .iter()
                    .find(|server| &server.id == id)
                    .unwrap()
                    .open_sequence
                    .len();
            }
            self.states.get_mut(&ids[1]).unwrap().state = PortState::Reachable;
            self.ui.upgrade().unwrap().set_batch_view(true);
        }
        self.refresh();
    }
}

fn stage(server: &Server, state: &ServerState) -> Message {
    if let Some(error) = &state.knock_error {
        msg!("Send failed · {error}", "error" => error.clone())
    } else if state.knock.is_some() {
        msg!("Sending step {current} / {total}", "current" => state.progress.min(server.open_sequence.len()), "total" => server.open_sequence.len())
    } else if state.sent {
        msg!("Sequence sent · Steps: {current} / {total}", "current" => state.progress, "total" => server.open_sequence.len())
    } else {
        "Not knocked yet".into()
    }
}

fn parse_number(value: &str, label: &str) -> Result<u64, Message> {
    value
        .trim()
        .parse()
        .map_err(|_| msg!("{field} must be a positive integer.", "field" => Message::from(label)))
}
fn parse_port(value: &str) -> Result<u16, Message> {
    let port = value
        .trim()
        .parse::<u16>()
        .map_err(|_| "Port must be an integer between 1 and 65535.")?;
    if port == 0 {
        Err("Port must be an integer between 1 and 65535.".into())
    } else {
        Ok(port)
    }
}
fn protocol(udp: bool) -> Protocol {
    if udp { Protocol::Udp } else { Protocol::Tcp }
}
fn now() -> String {
    chrono::Local::now().format("%H:%M:%S").to_string()
}
