mod auth;
mod runtime;
mod store;

use crate::model::{BatchUpdate, Config, KnockStep, Protocol, Server, Settings};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, State},
    http::{StatusCode, header},
    middleware,
    response::{Html, IntoResponse, Response},
    routing::{get, post, put},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

type WebState = Arc<App>;
type ApiResult<T> = Result<Json<T>, ApiError>;

pub struct App {
    store: Arc<Mutex<store::Store>>,
    runtime: Mutex<HashMap<String, runtime::ServerState>>,
    client: crate::network::Client,
    permits: Arc<tokio::sync::Semaphore>,
    operation: tokio::sync::Mutex<()>,
    auth: auth::Auth,
    instance: String,
    events: tokio::sync::broadcast::Sender<runtime::ServerEvent>,
}

#[derive(Debug)]
pub struct ApiError(StatusCode, String);

impl ApiError {
    fn bad(message: impl Into<String>) -> Self {
        Self(StatusCode::BAD_REQUEST, message.into())
    }
    fn validation(message: crate::i18n::Message) -> Self {
        Self::bad(message.render("zh-CN"))
    }
    fn unauthorized() -> Self {
        Self(StatusCode::UNAUTHORIZED, "请先登录".into())
    }
    fn internal(error: impl std::fmt::Display) -> Self {
        eprintln!("Web 请求失败：{error}");
        Self(
            StatusCode::INTERNAL_SERVER_ERROR,
            "操作失败，请查看服务器日志".into(),
        )
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(serde_json::json!({"error": self.1}))).into_response()
    }
}
impl From<rusqlite::Error> for ApiError {
    fn from(e: rusqlite::Error) -> Self {
        Self::internal(e)
    }
}
impl From<serde_json::Error> for ApiError {
    fn from(e: serde_json::Error) -> Self {
        Self::internal(e)
    }
}

impl App {
    async fn db<T: Send + 'static>(
        &self,
        operation: impl FnOnce(&mut store::Store) -> Result<T, ApiError> + Send + 'static,
    ) -> Result<T, ApiError> {
        let store = self.store.clone();
        // SQLite 与密码散列都在阻塞线程池执行，网络任务不持有数据库锁。
        tokio::task::spawn_blocking(move || {
            operation(&mut *store.lock().map_err(ApiError::internal)?)
        })
        .await
        .map_err(ApiError::internal)?
    }
}

pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::var_os("OPENSESAME_WEB_DB")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "opensesame-web.sqlite3".into());
    let store = store::Store::open(&path)?;
    let auth = auth::Auth::from_env(&store.auth_salt)?;
    let app = Arc::new(App {
        store: Arc::new(Mutex::new(store)),
        runtime: Mutex::new(HashMap::new()),
        client: Default::default(),
        permits: Arc::new(tokio::sync::Semaphore::new(16)),
        operation: Default::default(),
        auth,
        instance: crate::model::new_id(),
        events: tokio::sync::broadcast::channel(256).0,
    });
    let listen =
        std::env::var("OPENSESAME_WEB_LISTEN").unwrap_or_else(|_| "127.0.0.1:18010".into());
    let listener = tokio::net::TcpListener::bind(&listen).await?;
    print_banner();
    println!("Listening on {listen}");
    let result = axum::serve(
        listener,
        router(app).into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await;
    result?;
    Ok(())
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut terminate = signal(SignalKind::terminate()).expect("无法注册退出信号");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {},
            _ = terminate.recv() => {},
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

fn print_banner() {
    use std::io::IsTerminal;
    let version = concat!(
        "v",
        env!("CARGO_PKG_VERSION"),
        "_",
        env!("OPENSESAME_BUILD_COMMIT")
    );
    if std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none() {
        println!(
            "\x1b[48;5;29m\x1b[97m OpenSesame Web \x1b[48;5;194m\x1b[38;5;29m {version} \x1b[0m"
        );
        println!("\x1b[38;5;29mCopyright © 2026 Qwerto107. All rights reserved.\x1b[0m");
    } else {
        println!("OpenSesame Web  {version}\nCopyright © 2026 Qwerto107. All rights reserved.");
    }
}

fn router(app: WebState) -> Router {
    let api = Router::new()
        .route("/state", get(snapshot))
        .route("/events", get(runtime::events))
        .route("/servers", post(create_server))
        .route("/servers/{id}", put(update_server).delete(delete_server))
        .route("/selection", put(selection))
        .route("/current", put(current))
        .route("/settings", put(settings))
        .route("/batch", put(batch))
        .route("/knock", post(knock))
        .route("/probe", post(probe))
        .route("/logout", post(auth::logout))
        .route("/forget", post(auth::forget))
        .route_layer(middleware::from_fn_with_state(
            app.clone(),
            auth::require_session,
        ));
    Router::new()
        .nest("/api", api)
        .route("/auth/status", get(auth::status))
        .route("/auth/login", post(auth::login))
        .route("/auth/google", get(auth::google_start))
        .route("/auth/google/callback", get(auth::google_callback))
        .route(
            "/",
            get(|| async { Html(include_str!("../../web/index.html")) }),
        )
        .route(
            "/app.css",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
                    include_str!("../../web/app.css"),
                )
            }),
        )
        .route("/app.js", get(browser_script))
        .route(
            "/fonts/inter.woff2",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "font/woff2")],
                    include_bytes!("../../web/fonts/inter.woff2").as_slice(),
                )
            }),
        )
        .route(
            "/fonts/mono.woff2",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "font/woff2")],
                    include_bytes!("../../web/fonts/mono.woff2").as_slice(),
                )
            }),
        )
        .layer(DefaultBodyLimit::max(128 * 1024))
        .layer(middleware::from_fn_with_state(
            app.clone(),
            security_headers,
        ))
        .with_state(app)
}

async fn browser_script() -> impl IntoResponse {
    let version = concat!(
        "v",
        env!("CARGO_PKG_VERSION"),
        "_",
        env!("OPENSESAME_BUILD_COMMIT")
    );
    let version = serde_json::to_string(version).expect("版本号必须是有效 JSON 字符串");
    let script = include_str!("../../web/app.js").replace("'__BUILD_VERSION__'", &version);
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        script,
    )
}

async fn security_headers(
    State(app): State<WebState>,
    request: axum::extract::Request,
    next: middleware::Next,
) -> Response {
    if !matches!(
        *request.method(),
        axum::http::Method::GET | axum::http::Method::HEAD
    ) && (request
        .headers()
        .get("x-requested-with")
        .and_then(|h| h.to_str().ok())
        != Some("OpenSesame")
        || request
            .headers()
            .get(header::ORIGIN)
            .and_then(|h| h.to_str().ok())
            .is_some_and(|origin| origin != app.auth.origin))
    {
        return ApiError(StatusCode::FORBIDDEN, "请求来源无效，请刷新页面".into()).into_response();
    }
    let mut response = next.run(request).await;
    for (key, value) in [
        (
            "content-security-policy",
            "default-src 'self'; script-src 'self'; style-src 'self'; font-src 'self'; img-src 'self' data:; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'",
        ),
        ("x-content-type-options", "nosniff"),
        ("referrer-policy", "no-referrer"),
        ("cache-control", "no-store"),
    ] {
        response.headers_mut().insert(key, value.parse().unwrap());
    }
    response
}

#[derive(Serialize)]
struct Snapshot {
    instance: String,
    config: Config,
    revision: u64,
    states: HashMap<String, runtime::ServerView>,
}
async fn snapshot(State(app): State<WebState>) -> ApiResult<Snapshot> {
    let (config, revision) = app.db(|db| Ok((db.config.clone(), db.revision))).await?;
    let states = app
        .runtime
        .lock()
        .unwrap()
        .iter()
        .map(|(id, state)| (id.clone(), state.view.clone()))
        .collect();
    Ok(Json(Snapshot {
        instance: app.instance.clone(),
        config,
        revision,
        states,
    }))
}

#[derive(Deserialize)]
struct Edit<T> {
    revision: u64,
    value: T,
}
async fn edit(
    app: &App,
    revision: u64,
    change: impl FnOnce(&mut Config) -> Result<(), ApiError> + Send + 'static,
) -> ApiResult<u64> {
    let _operation = app.operation.lock().await;
    let (revision, changed) = app
        .db(move |db| {
            if db.revision != revision {
                return Err(ApiError(
                    StatusCode::CONFLICT,
                    "数据已更新，请刷新后重试".into(),
                ));
            }
            let mut config = db.config.clone();
            change(&mut config)?;
            let changed: Vec<_> = db
                .config
                .servers
                .iter()
                .filter(|server| !config.servers.contains(server))
                .map(|server| server.id.clone())
                .collect();
            db.save(config)?;
            Ok((db.revision, changed))
        })
        .await?;
    for id in changed {
        app.cancel(&id);
    }
    Ok(Json(revision))
}

async fn create_server(
    State(app): State<WebState>,
    Json(input): Json<Edit<Server>>,
) -> ApiResult<u64> {
    edit(&app, input.revision, move |config| {
        if config.servers.len() >= 1000 {
            return Err(ApiError::bad("最多支持 1000 台服务器"));
        }
        let mut server = input.value;
        server.id = crate::model::new_id();
        validate_size(&server)?;
        config.servers.push(server);
        Ok(())
    })
    .await
}
fn validate_size(server: &Server) -> Result<(), ApiError> {
    if server.name.len() > 256 || server.host.len() > 253 || server.open_sequence.len() > 256 {
        return Err(ApiError::bad("名称、地址或敲门序列过长"));
    }
    server.validate().map_err(ApiError::validation)
}
async fn update_server(
    State(app): State<WebState>,
    Path(id): Path<String>,
    Json(input): Json<Edit<Server>>,
) -> ApiResult<u64> {
    let result = edit(&app, input.revision, {
        let id = id.clone();
        move |config| {
            validate_size(&input.value)?;
            let server = config
                .servers
                .iter_mut()
                .find(|s| s.id == id)
                .ok_or_else(|| ApiError::bad("服务器已删除"))?;
            *server = Server { id, ..input.value };
            Ok(())
        }
    })
    .await?;
    Ok(result)
}
async fn delete_server(
    State(app): State<WebState>,
    Path(id): Path<String>,
    Json(input): Json<Edit<()>>,
) -> ApiResult<u64> {
    let result = edit(&app, input.revision, {
        let id = id.clone();
        move |config| {
            config.servers.retain(|s| s.id != id);
            config.checked_servers.retain(|s| s != &id);
            if config.current_server.as_ref() == Some(&id) {
                config.current_server = None;
            }
            Ok(())
        }
    })
    .await?;
    Ok(result)
}
async fn selection(
    State(app): State<WebState>,
    Json(input): Json<Edit<Vec<String>>>,
) -> ApiResult<u64> {
    edit(&app, input.revision, move |config| {
        ensure_ids(config, &input.value, false)?;
        config.checked_servers = input.value;
        Ok(())
    })
    .await
}
async fn current(
    State(app): State<WebState>,
    Json(input): Json<Edit<Option<String>>>,
) -> ApiResult<u64> {
    edit(&app, input.revision, move |config| {
        if let Some(id) = &input.value {
            ensure_ids(config, std::slice::from_ref(id), true)?;
        }
        config.current_server = input.value;
        Ok(())
    })
    .await
}
async fn settings(
    State(app): State<WebState>,
    Json(input): Json<Edit<Settings>>,
) -> ApiResult<u64> {
    edit(&app, input.revision, move |config| {
        config.settings = input.value;
        Ok(())
    })
    .await
}
#[derive(Deserialize)]
struct Batch {
    ids: Vec<String>,
    sequence: Option<Vec<KnockStep>>,
    service: Option<(u16, Protocol)>,
}
async fn batch(State(app): State<WebState>, Json(input): Json<Edit<Batch>>) -> ApiResult<u64> {
    let result = edit(&app, input.revision, move |config| {
        ensure_ids(config, &input.value.ids, true)?;
        if input
            .value
            .sequence
            .as_ref()
            .is_some_and(|steps| steps.len() > 256)
        {
            return Err(ApiError::bad("敲门序列过长"));
        }
        crate::model::apply_batch(
            config,
            &input.value.ids,
            &BatchUpdate {
                sequence: input.value.sequence,
                service: input.value.service,
                tcp_flags: None,
            },
        )
        .map_err(ApiError::validation)?;
        Ok(())
    })
    .await?;
    Ok(result)
}
fn ensure_ids(config: &Config, ids: &[String], required: bool) -> Result<(), ApiError> {
    let unique: std::collections::HashSet<_> = ids.iter().collect();
    if (required && ids.is_empty())
        || unique.len() != ids.len()
        || ids
            .iter()
            .any(|id| !config.servers.iter().any(|s| &s.id == id))
    {
        return Err(ApiError::bad("请重新选择服务器"));
    }
    Ok(())
}
#[derive(Deserialize)]
struct Ids {
    ids: Vec<String>,
}
async fn knock(State(app): State<WebState>, Json(input): Json<Ids>) -> ApiResult<()> {
    operate(app, input.ids, true).await
}
async fn probe(State(app): State<WebState>, Json(input): Json<Ids>) -> ApiResult<()> {
    operate(app, input.ids, false).await
}
async fn operate(app: WebState, ids: Vec<String>, knock: bool) -> ApiResult<()> {
    let _operation = app.operation.lock().await;
    let servers = app
        .db(move |db| {
            ensure_ids(&db.config, &ids, true)?;
            let mut servers: Vec<_> = db
                .config
                .servers
                .iter()
                .filter(|s| ids.contains(&s.id))
                .cloned()
                .collect();
            for server in &mut servers {
                server.check_timeout_secs = db.config.settings.check_timeout_secs;
            }
            Ok(servers)
        })
        .await?;
    app.start(servers, knock)?;
    Ok(Json(()))
}

#[cfg(test)]
mod tests;
