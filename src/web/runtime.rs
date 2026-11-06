use super::{ApiError, App, WebState};
use crate::{model::Server, network::PortState};
use serde::Serialize;
use std::{collections::VecDeque, time::Duration};
use tokio::task::JoinHandle;

fn state_snapshot(app: &App) -> axum::response::sse::Event {
    let states: std::collections::HashMap<_, _> = app
        .runtime
        .lock()
        .unwrap()
        .iter()
        .map(|(id, state)| (id.clone(), state.view.clone()))
        .collect();
    axum::response::sse::Event::default()
        .event("snapshot")
        .json_data(serde_json::json!({"instance": app.instance, "states": states}))
        .unwrap()
}

pub async fn events(
    axum::extract::State(app): axum::extract::State<WebState>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    // 先订阅再取快照；版本号让客户端合并快照与实时事件时不会回退进度。
    let receiver = app.events.subscribe();
    let initial = state_snapshot(&app);
    let stream = futures_util::stream::unfold(
        (
            receiver,
            Some(initial),
            tokio::time::interval(Duration::from_secs(30)),
        ),
        move |(mut receiver, mut initial, mut auth_check)| {
            let app = app.clone();
            let headers = headers.clone();
            async move {
                let event = if let Some(initial) = initial.take() {
                    initial
                } else {
                    loop {
                        tokio::select! {
                            message = receiver.recv() => match message {
                                Ok(update) => break axum::response::sse::Event::default().event("server").json_data(update).unwrap(),
                                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => break state_snapshot(&app),
                                Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
                            },
                            _ = auth_check.tick() => {
                                if super::auth::session(&app, &headers).await.is_err() { return None; }
                            }
                        }
                    }
                };
                Some((
                    Ok::<_, std::convert::Infallible>(event),
                    (receiver, initial, auth_check),
                ))
            }
        },
    );
    // 逐步转发，避免反向代理缓冲后让动画一次跳过多个节点。
    (
        [("x-accel-buffering", "no")],
        axum::response::Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::default()),
    )
        .into_response()
}

#[derive(Clone, Copy, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum KnockState {
    #[default]
    Idle,
    Queued,
    Sending,
    Sent,
    Failed,
}

#[derive(Clone, Serialize)]
pub struct ServerEvent {
    instance: String,
    id: String,
    state: ServerView,
}

#[derive(Clone, Default, Serialize)]
pub struct ServerView {
    pub version: u64,
    pub knock_id: u64,
    pub knock_state: KnockState,
    pub status: String,
    pub detail: String,
    pub checked_at: Option<i64>,
    pub busy: bool,
    pub knocking: bool,
    pub progress: usize,
    pub knock_message: String,
    pub history: VecDeque<Record>,
}
#[derive(Clone, Serialize)]
pub struct Record {
    at: i64,
    message: String,
}
#[derive(Default)]
pub struct ServerState {
    pub view: ServerView,
    generation: u64,
    task: Option<JoinHandle<()>>,
}

impl App {
    pub fn cancel(&self, id: &str) {
        if let Some(state) = self.runtime.lock().unwrap().get_mut(id) {
            if let Some(task) = state.task.take() {
                task.abort();
            }
            state.generation += 1;
            state.view = ServerView {
                version: state.view.version + 1,
                ..Default::default()
            };
            self.publish(id, &state.view);
        }
    }

    pub fn start(
        self: &std::sync::Arc<Self>,
        servers: Vec<Server>,
        knock: bool,
    ) -> Result<(), ApiError> {
        let mut runtime = self.runtime.lock().unwrap();
        // 批量操作先检查整组，避免重复请求只启动其中一部分服务器。
        if servers
            .iter()
            .any(|server| runtime.get(&server.id).is_some_and(|state| state.view.busy))
        {
            return Err(ApiError::bad("所选服务器正在执行任务，请稍后重试"));
        }
        for server in servers {
            let state = runtime.entry(server.id.clone()).or_default();
            state.generation += 1;
            state.view.version += 1;
            state.view.busy = true;
            state.view.knocking = knock;
            if knock {
                state.view.knock_id = state.generation;
                state.view.knock_state = KnockState::Queued;
                state.view.progress = 0;
                state.view.knock_message = "等待发送敲门序列".into();
            } else {
                state.view.status = "checking".into();
            }
            let generation = state.generation;
            self.publish(&server.id, &state.view);
            let app = self.clone();
            state.task = Some(tokio::spawn(async move {
                execute(app, server, generation, knock).await;
            }));
        }
        Ok(())
    }

    fn update(&self, id: &str, generation: u64, update: impl FnOnce(&mut ServerState)) {
        if let Some(state) = self
            .runtime
            .lock()
            .unwrap()
            .get_mut(id)
            .filter(|state| state.generation == generation)
        {
            update(state);
            state.view.version += 1;
            self.publish(id, &state.view);
        }
    }

    fn publish(&self, id: &str, state: &ServerView) {
        let _ = self.events.send(ServerEvent {
            instance: self.instance.clone(),
            id: id.to_owned(),
            state: state.clone(),
        });
    }
}

fn record(state: &mut ServerState, message: String) {
    state.view.history.push_front(Record {
        at: chrono::Utc::now().timestamp(),
        message,
    });
    state.view.history.truncate(10);
}

async fn execute(app: WebState, server: Server, generation: u64, knock: bool) {
    let Ok(_permit) = app.permits.clone().acquire_owned().await else {
        return;
    };
    let mut pinned = None;
    if knock {
        let result = app
            .client
            .execute(&server, |step| {
                app.update(&server.id, generation, |state| {
                    state.view.progress = step;
                    state.view.knock_state = KnockState::Sending;
                    state.view.knock_message =
                        format!("正在发送 · {step} / {} 步", server.open_sequence.len());
                })
            })
            .await;
        let (ip, elapsed) = match result {
            Ok(sent) => sent,
            Err(error) => {
                let message = format!("发送失败：{}", error.render("zh-CN"));
                app.update(&server.id, generation, |state| {
                    state.view.knocking = false;
                    state.view.busy = false;
                    state.view.knock_state = KnockState::Failed;
                    state.view.knock_message = message.clone();
                    state.view.status = "error".into();
                    state.view.detail = message.clone();
                    record(state, message);
                });
                return;
            }
        };
        pinned = Some(ip);
        app.update(&server.id, generation, |state| {
            state.view.knocking = false;
            state.view.knock_state = KnockState::Sent;
            state.view.knock_message =
                format!("敲门序列已发送 · {0} / {0} 步", server.open_sequence.len());
            record(
                state,
                format!(
                    "敲门序列已发送（{} 步，耗时 {:.1} 秒）",
                    server.open_sequence.len(),
                    elapsed.as_secs_f32()
                ),
            );
            state.view.status = "checking".into();
        });
    }
    let result = app.client.probe(&server, pinned).await;
    app.update(&server.id, generation, |state| {
        state.view.status = match result.state {
            PortState::Idle => "idle",
            PortState::Checking => "checking",
            PortState::Reachable => "reachable",
            PortState::Refused => "refused",
            PortState::TimedOut => "timeout",
            PortState::Unknown => "unknown",
            PortState::Error => "error",
        }
        .into();
        state.view.detail = result.detail.render("zh-CN");
        state.view.checked_at = Some(chrono::Utc::now().timestamp());
        state.view.busy = false;
        record(
            state,
            format!(
                "端口 {} / {} {}",
                server.service_port,
                server.service_protocol.label(),
                crate::i18n::Message::from(result.state.short_label(server.service_protocol))
                    .render("zh-CN")
            ),
        );
    });
}
