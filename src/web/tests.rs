use super::*;
use serde_json::{Value, json};
use std::time::Duration;

#[tokio::test]
async fn failed_knock_finishes_without_probing_service_port() {
    let tcp = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let store = store::Store::open(std::path::Path::new(":memory:")).unwrap();
    let app = Arc::new(App {
        store: Arc::new(Mutex::new(store)),
        runtime: Default::default(),
        client: Default::default(),
        permits: Arc::new(tokio::sync::Semaphore::new(16)),
        operation: Default::default(),
        auth: auth::Auth::for_test("http://127.0.0.1".into()),
        instance: crate::model::new_id(),
        events: tokio::sync::broadcast::channel(256).0,
    });
    let mut events = app.events.subscribe();
    // 空序列在网络层确定性失败，验证错误分支不会发送业务端口探测包。
    let server = Server {
        host: "127.0.0.1".into(),
        open_sequence: vec![],
        service_port: tcp.local_addr().unwrap().port(),
        ..Server::default()
    };
    app.start(vec![server], true).unwrap();
    let finished = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let update = serde_json::to_value(events.recv().await.unwrap()).unwrap();
            let state = &update["state"];
            assert_ne!(state["status"], "checking");
            if state["busy"] == false {
                break state.clone();
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(finished["knock_state"], "failed");
    assert_eq!(finished["status"], "error");
    assert_eq!(finished["knocking"], false);
    assert!(finished["checked_at"].is_null());
    assert_eq!(finished["history"].as_array().unwrap().len(), 1);
    assert_eq!(finished["detail"], finished["knock_message"]);
    assert!(
        tokio::time::timeout(Duration::from_millis(100), tcp.accept())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn http_auth_sqlite_crud_conflicts_batch_and_loopback_knock() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let store = store::Store::open(std::path::Path::new(":memory:")).unwrap();
    let app = Arc::new(App {
        store: Arc::new(Mutex::new(store)),
        runtime: Default::default(),
        client: Default::default(),
        permits: Arc::new(tokio::sync::Semaphore::new(16)),
        operation: Default::default(),
        auth: auth::Auth::for_test(base.clone()),
        instance: crate::model::new_id(),
        events: tokio::sync::broadcast::channel(256).0,
    });
    let task = tokio::spawn(
        axum::serve(
            listener,
            router(app.clone()).into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .into_future(),
    );
    let client = reqwest::Client::new();
    let browser_script = client
        .get(format!("{base}/app.js"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(browser_script.contains(&format!(
        "const BUILD_VERSION = \"v{}_{}\";",
        env!("CARGO_PKG_VERSION"),
        env!("OPENSESAME_BUILD_COMMIT")
    )));
    assert_eq!(
        client
            .get(format!("{base}/api/state"))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        client
            .post(format!("{base}/auth/login"))
            .json(&json!({"password":"test-password-only","remember":true}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        client
            .post(format!("{base}/auth/login"))
            .header("X-Requested-With", "OpenSesame")
            .header("Origin", "https://other.example")
            .json(&json!({"password":"test-password-only","remember":true}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    let wrong = client
        .post(format!("{base}/auth/login"))
        .header("X-Requested-With", "OpenSesame")
        .json(&json!({"password":"incorrect","remember":false}))
        .send()
        .await
        .unwrap();
    assert_eq!(wrong.status(), StatusCode::UNAUTHORIZED);
    let login = client
        .post(format!("{base}/auth/login"))
        .header("X-Requested-With", "OpenSesame")
        .json(&json!({"password":"test-password-only","remember":true}))
        .send()
        .await
        .unwrap();
    assert_eq!(login.status(), StatusCode::OK);
    let cookie = login.headers()[header::SET_COOKIE].to_str().unwrap();
    assert!(cookie.contains("HttpOnly") && cookie.contains("Max-Age=2592000"));
    let cookie = cookie.split(';').next().unwrap().to_owned();
    let request = |method, path: &str| {
        client
            .request(method, format!("{base}{path}"))
            .header(header::COOKIE, &cookie)
            .header("X-Requested-With", "OpenSesame")
    };
    let initial: Value = request(reqwest::Method::GET, "/api/state")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(initial["config"]["servers"], json!([]));
    let udp = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let tcp = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let local = Server {
        name: "本地测试".into(),
        host: "127.0.0.1".into(),
        open_sequence: vec![
            KnockStep {
                port: udp.local_addr().unwrap().port(),
                protocol: Protocol::Udp,
            };
            4
        ],
        interval_ms: 30,
        service_port: tcp.local_addr().unwrap().port(),
        ..Server::default()
    };
    let response = request(reqwest::Method::POST, "/api/servers")
        .json(&json!({"revision":0,"value":local}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let conflict = request(reqwest::Method::POST, "/api/servers")
        .json(&json!({"revision":0,"value":local}))
        .send()
        .await
        .unwrap();
    assert_eq!(conflict.status(), StatusCode::CONFLICT);
    let snapshot: Value = request(reqwest::Method::GET, "/api/state")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let id = snapshot["config"]["servers"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let selected = request(reqwest::Method::PUT, "/api/selection")
        .json(&json!({"revision":1,"value":[id]}))
        .send()
        .await
        .unwrap();
    assert_eq!(selected.status(), StatusCode::OK);
    let invalid = request(reqwest::Method::PUT, "/api/batch")
        .json(&json!({"revision":2,"value":{"ids":[id],"service":[0,"tcp"],"sequence":null}}))
        .send()
        .await
        .unwrap();
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        client
            .get(format!("{base}/api/events"))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let mut events = request(reqwest::Method::GET, "/api/events")
        .send()
        .await
        .unwrap();
    assert_eq!(events.status(), StatusCode::OK);
    assert!(
        events.headers()[header::CONTENT_TYPE]
            .to_str()
            .unwrap()
            .starts_with("text/event-stream")
    );
    let initial_event = events.chunk().await.unwrap().unwrap();
    assert!(String::from_utf8_lossy(&initial_event).contains("event: snapshot"));
    let knock = request(reqwest::Method::POST, "/api/knock")
        .json(&json!({"ids":[id]}))
        .send()
        .await
        .unwrap();
    assert_eq!(knock.status(), StatusCode::OK);
    let mut buffer = [0u8; 8];
    tokio::time::timeout(Duration::from_secs(3), udp.recv_from(&mut buffer))
        .await
        .unwrap()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), tcp.accept())
        .await
        .unwrap()
        .unwrap();
    let mut completed = false;
    for _ in 0..50 {
        let snapshot: Value = request(reqwest::Method::GET, "/api/state")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(snapshot["config"]["checked_servers"], json!([id]));
        if snapshot["states"][&id]["busy"] == false {
            assert_eq!(snapshot["states"][&id]["status"], "reachable");
            assert_eq!(snapshot["states"][&id]["progress"], 4);
            completed = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(completed);
    let mut buffer = String::new();
    let mut steps = vec![];
    let mut sent = false;
    let mut previous_version = 0;
    tokio::time::timeout(Duration::from_secs(3), async {
        while !sent {
            let chunk = events.chunk().await.unwrap().unwrap();
            buffer.push_str(&String::from_utf8_lossy(&chunk));
            while let Some(end) = buffer.find("\n\n") {
                let message: String = buffer.drain(..end + 2).collect();
                for line in message
                    .lines()
                    .filter_map(|line| line.strip_prefix("data: "))
                {
                    let update: Value = serde_json::from_str(line).unwrap();
                    if update["id"] != id {
                        continue;
                    }
                    let state = &update["state"];
                    let version = state["version"].as_u64().unwrap();
                    assert!(version > previous_version);
                    previous_version = version;
                    if state["knock_state"] == "sending" {
                        steps.push(state["progress"].as_u64().unwrap());
                    }
                    if state["knock_state"] == "sent" {
                        sent = true;
                    }
                }
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(steps, vec![1, 2, 3, 4]);
    drop(events);
    let forget = request(reqwest::Method::POST, "/api/forget")
        .send()
        .await
        .unwrap();
    assert_eq!(forget.status(), StatusCode::OK);
    assert!(
        !forget.headers()[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .contains("Max-Age")
    );
    assert_eq!(
        request(reqwest::Method::GET, "/api/state")
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let cookie = forget.headers()[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap();
    let response = client
        .post(format!("{base}/api/logout"))
        .header("Cookie", cookie)
        .header("X-Requested-With", "OpenSesame")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        client
            .get(format!("{base}/api/state"))
            .header("Cookie", cookie)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        client
            .get(format!("{base}/auth/google"))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST
    );
    task.abort();
}
