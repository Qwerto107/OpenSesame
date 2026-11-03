use super::{ApiError, WebState};
use argon2::{
    Argon2, PasswordHasher, PasswordVerifier,
    password_hash::{PasswordHash, SaltString},
};
use axum::{
    Json,
    extract::{ConnectInfo, Query, State},
    http::{HeaderMap, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Redirect, Response},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::{RngCore, rngs::OsRng};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::Mutex,
    time::{Duration, Instant},
};

pub struct Auth {
    pub origin: String,
    password_hash: String,
    credential: String,
    secure: bool,
    google: Option<Google>,
    attempts: Mutex<HashMap<IpAddr, (Instant, u32)>>,
    pending: Mutex<HashMap<String, Pending>>,
    http: reqwest::Client,
}
struct Google {
    client_id: String,
    secret: String,
    emails: Vec<String>,
}
struct Pending {
    nonce: String,
    verifier: String,
    created: Instant,
}

fn env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.trim().is_empty())
}
pub fn token() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}
pub fn hash(value: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(value.as_bytes()))
}
pub fn salt() -> String {
    SaltString::generate(&mut OsRng).to_string()
}
fn now() -> i64 {
    chrono::Utc::now().timestamp()
}
fn cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|s| s.split(';'))
        .find_map(|part| {
            let (key, value) = part.trim().split_once('=')?;
            (key == name).then(|| value.to_owned())
        })
}
impl Auth {
    #[cfg(test)]
    pub(super) fn for_test(origin: String) -> Self {
        let password = "test-password-only";
        Self {
            origin,
            password_hash: Argon2::default()
                .hash_password(password.as_bytes(), &SaltString::generate(&mut OsRng))
                .unwrap()
                .to_string(),
            credential: hash(password),
            secure: false,
            google: None,
            attempts: Mutex::new(HashMap::new()),
            pending: Mutex::new(HashMap::new()),
            http: reqwest::Client::new(),
        }
    }
    pub fn from_env(salt: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let password = if let Some(file) = env("OPENSESAME_WEB_PASSWORD_FILE") {
            std::fs::read_to_string(file)?
                .trim_end_matches(['\r', '\n'])
                .to_owned()
        } else {
            env("OPENSESAME_WEB_PASSWORD")
                .ok_or("必须配置 OPENSESAME_WEB_PASSWORD_FILE 或 OPENSESAME_WEB_PASSWORD")?
        };
        if password.len() < 12 || password.len() > 1024 {
            return Err("访问密码需要 12～1024 字节".into());
        }
        let password_hash = Argon2::default()
            .hash_password(
                password.as_bytes(),
                &SaltString::from_b64(salt).map_err(|_| "数据库认证盐值无效")?,
            )
            .map_err(|_| "无法初始化密码验证器")?
            .to_string();
        let origin =
            env("OPENSESAME_WEB_ORIGIN").unwrap_or_else(|| "http://127.0.0.1:18010".into());
        let url = reqwest::Url::parse(&origin)?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || url.path() != "/"
            || url.query().is_some()
            || url.fragment().is_some()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(
                "OPENSESAME_WEB_ORIGIN 必须是完整的站点来源，例如 https://knock.example.com".into(),
            );
        }
        let origin = url.origin().ascii_serialization();
        let secure = url.scheme() == "https";
        let credentials = [
            env("OPENSESAME_GOOGLE_CLIENT_ID"),
            env("OPENSESAME_GOOGLE_CLIENT_SECRET"),
            env("OPENSESAME_GOOGLE_EMAILS"),
        ];
        let google = if credentials.iter().any(Option::is_some) {
            let [Some(client_id), Some(secret), Some(emails)] = credentials else {
                return Err("Google 登录需要同时配置 CLIENT_ID、CLIENT_SECRET 和 EMAILS".into());
            };
            if !secure && !matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]")) {
                return Err("Google 登录远程回调必须使用 HTTPS".into());
            }
            let emails: Vec<_> = emails
                .split(',')
                .map(|e| e.trim().to_lowercase())
                .filter(|e| !e.is_empty())
                .collect();
            if emails.is_empty() {
                return Err("Google 邮箱白名单不能为空".into());
            }
            Some(Google {
                client_id,
                secret,
                emails,
            })
        } else {
            None
        };
        // 凭据改变会使旧会话失效；数据库中只存令牌摘要，不存访问密码。
        let credential = hash(&format!(
            "{password_hash}\0{origin}\0{}",
            google
                .as_ref()
                .map(|g| format!("{}:{}:{}", g.client_id, g.secret, g.emails.join(",")))
                .unwrap_or_default()
        ));
        Ok(Self {
            origin,
            password_hash,
            credential,
            secure,
            google,
            attempts: Mutex::new(HashMap::new()),
            pending: Mutex::new(HashMap::new()),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(15))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
        })
    }
    fn session_cookie(&self, value: &str, max_age: Option<i64>) -> String {
        format!(
            "os_session={value}; HttpOnly; SameSite=Lax; Path=/{}{}",
            if self.secure { "; Secure" } else { "" },
            max_age
                .map(|age| format!("; Max-Age={age}"))
                .unwrap_or_default()
        )
    }
    fn oauth_cookie(&self, value: &str, age: i64) -> String {
        format!(
            "os_oauth={value}; HttpOnly; SameSite=Lax; Path=/auth/google; Max-Age={age}{}",
            if self.secure { "; Secure" } else { "" }
        )
    }
    fn rate_limit(&self, ip: IpAddr) -> Result<(), ApiError> {
        let mut attempts = self.attempts.lock().unwrap();
        attempts.retain(|_, (time, _)| time.elapsed() < Duration::from_secs(60));
        if attempts.len() >= 4096 && !attempts.contains_key(&ip) {
            return Err(ApiError(
                StatusCode::TOO_MANY_REQUESTS,
                "请求过多，请稍后重试".into(),
            ));
        }
        let (_, count) = attempts.entry(ip).or_insert((Instant::now(), 0));
        *count += 1;
        if *count > 10 {
            return Err(ApiError(
                StatusCode::TOO_MANY_REQUESTS,
                "登录尝试过多，请一分钟后重试".into(),
            ));
        }
        Ok(())
    }
}

pub(super) async fn session(app: &WebState, headers: &HeaderMap) -> Result<bool, ApiError> {
    let raw = cookie(headers, "os_session").ok_or_else(ApiError::unauthorized)?;
    let digest = hash(&raw);
    let credential = app.auth.credential.clone();
    app.db(move |db| {
        let remembered = db.connection.query_row("SELECT remembered FROM sessions WHERE token_hash=?1 AND expires>?2 AND credential=?3", params![digest, now(), credential], |r| r.get::<_, bool>(0)).optional()?;
        remembered.ok_or_else(ApiError::unauthorized)
    }).await
}
pub async fn require_session(
    State(app): State<WebState>,
    request: axum::extract::Request,
    next: Next,
) -> Response {
    match session(&app, request.headers()).await {
        Ok(_) => next.run(request).await,
        Err(error) => error.into_response(),
    }
}
#[derive(Serialize)]
pub struct Status {
    authenticated: bool,
    remembered: bool,
    google: bool,
}
pub async fn status(State(app): State<WebState>, headers: HeaderMap) -> Json<Status> {
    let result = session(&app, &headers).await;
    Json(Status {
        authenticated: result.is_ok(),
        remembered: result.unwrap_or(false),
        google: app.auth.google.is_some(),
    })
}
#[derive(Deserialize)]
pub struct Login {
    password: String,
    remember: bool,
}
pub async fn login(
    State(app): State<WebState>,
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
    Json(input): Json<Login>,
) -> Result<Response, ApiError> {
    app.auth.rate_limit(remote.ip())?;
    if input.password.len() > 1024 {
        return Err(ApiError::unauthorized());
    }
    let stored = app.auth.password_hash.clone();
    let valid = tokio::task::spawn_blocking(move || {
        PasswordHash::new(&stored).is_ok_and(|encoded| {
            Argon2::default()
                .verify_password(input.password.as_bytes(), &encoded)
                .is_ok()
        })
    })
    .await
    .map_err(ApiError::internal)?;
    if !valid {
        return Err(ApiError(StatusCode::UNAUTHORIZED, "访问密码错误".into()));
    }
    new_session(&app, input.remember).await
}
async fn new_session(app: &WebState, remember: bool) -> Result<Response, ApiError> {
    let raw = token();
    let digest = hash(&raw);
    let credential = app.auth.credential.clone();
    let age = if remember { 30 * 86400 } else { 8 * 3600 };
    app.db(move |db| {
        db.connection.execute(
            "DELETE FROM sessions WHERE expires<=?1 OR credential<>?2",
            params![now(), credential],
        )?;
        db.connection.execute(
            "INSERT INTO sessions(token_hash,expires,remembered,credential) VALUES(?1,?2,?3,?4)",
            params![digest, now() + age, remember, credential],
        )?;
        Ok(())
    })
    .await?;
    Ok((
        [(
            header::SET_COOKIE,
            app.auth.session_cookie(&raw, remember.then_some(age)),
        )],
        Json(serde_json::json!({"ok":true})),
    )
        .into_response())
}
pub async fn logout(State(app): State<WebState>, headers: HeaderMap) -> Result<Response, ApiError> {
    let digest = hash(&cookie(&headers, "os_session").ok_or_else(ApiError::unauthorized)?);
    app.db(move |db| {
        db.connection
            .execute("DELETE FROM sessions WHERE token_hash=?1", [digest])?;
        Ok(())
    })
    .await?;
    Ok((
        [(header::SET_COOKIE, app.auth.session_cookie("", Some(0)))],
        Json(serde_json::json!({"ok":true})),
    )
        .into_response())
}
pub async fn forget(State(app): State<WebState>, headers: HeaderMap) -> Result<Response, ApiError> {
    let digest = hash(&cookie(&headers, "os_session").ok_or_else(ApiError::unauthorized)?);
    app.db(move |db| {
        db.connection
            .execute("DELETE FROM sessions WHERE token_hash=?1", [digest])?;
        Ok(())
    })
    .await?;
    new_session(&app, false).await
}

pub async fn google_start(
    State(app): State<WebState>,
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
) -> Result<Response, ApiError> {
    app.auth.rate_limit(remote.ip())?;
    let google = app
        .auth
        .google
        .as_ref()
        .ok_or_else(|| ApiError::bad("Google 登录尚未配置"))?;
    let state = token();
    let nonce = token();
    let verifier = token();
    let mut url = reqwest::Url::parse("https://accounts.google.com/o/oauth2/v2/auth").unwrap();
    url.query_pairs_mut().extend_pairs([
        ("client_id", google.client_id.as_str()),
        (
            "redirect_uri",
            &format!("{}/auth/google/callback", app.auth.origin),
        ),
        ("response_type", "code"),
        ("scope", "openid email"),
        ("state", &state),
        ("nonce", &nonce),
        ("code_challenge", &hash(&verifier)),
        ("code_challenge_method", "S256"),
    ]);
    let mut pending = app.auth.pending.lock().unwrap();
    pending.retain(|_, pending| pending.created.elapsed() < Duration::from_secs(300));
    if pending.len() >= 1024 {
        return Err(ApiError::bad("登录请求过多，请稍后重试"));
    }
    pending.insert(
        hash(&state),
        Pending {
            nonce,
            verifier,
            created: Instant::now(),
        },
    );
    Ok((
        [(header::SET_COOKIE, app.auth.oauth_cookie(&state, 300))],
        Redirect::to(url.as_str()),
    )
        .into_response())
}
#[derive(Deserialize)]
pub struct Callback {
    state: Option<String>,
    code: Option<String>,
}
#[derive(Deserialize)]
struct TokenResponse {
    id_token: String,
}
#[derive(Deserialize)]
struct Claims {
    nonce: String,
    email: String,
    email_verified: bool,
    sub: String,
}

pub async fn google_callback(
    State(app): State<WebState>,
    headers: HeaderMap,
    Query(input): Query<Callback>,
) -> Result<Response, ApiError> {
    let google = app
        .auth
        .google
        .as_ref()
        .ok_or_else(|| ApiError::bad("Google 登录尚未配置"))?;
    let state = input
        .state
        .ok_or_else(|| ApiError::bad("Google 登录已取消"))?;
    if cookie(&headers, "os_oauth").as_ref() != Some(&state) {
        return Err(ApiError::bad("Google 登录状态无效，请重新登录"));
    }
    // state 绑定浏览器且仅消费一次；nonce 与 PKCE 分别校验 ID Token 和授权码。
    let pending = app
        .auth
        .pending
        .lock()
        .unwrap()
        .remove(&hash(&state))
        .filter(|p| p.created.elapsed() < Duration::from_secs(300))
        .ok_or_else(|| ApiError::bad("Google 登录已过期，请重新登录"))?;
    let code = input
        .code
        .ok_or_else(|| ApiError::bad("Google 登录已取消"))?;
    let tokens: TokenResponse = app
        .auth
        .http
        .post("https://oauth2.googleapis.com/token")
        .form(&[
            ("client_id", google.client_id.as_str()),
            ("client_secret", google.secret.as_str()),
            (
                "redirect_uri",
                &format!("{}/auth/google/callback", app.auth.origin),
            ),
            ("code", &code),
            ("code_verifier", &pending.verifier),
            ("grant_type", "authorization_code"),
        ])
        .send()
        .await
        .map_err(|_| ApiError::bad("无法连接 Google，请稍后重试"))?
        .error_for_status()
        .map_err(|_| ApiError::bad("Google 授权码无效"))?
        .json()
        .await
        .map_err(|_| ApiError::bad("Google 登录响应无效"))?;
    let jwks: jsonwebtoken::jwk::JwkSet = app
        .auth
        .http
        .get("https://www.googleapis.com/oauth2/v3/certs")
        .send()
        .await
        .map_err(|_| ApiError::bad("无法获取 Google 公钥"))?
        .error_for_status()
        .map_err(|_| ApiError::bad("无法获取 Google 公钥"))?
        .json()
        .await
        .map_err(|_| ApiError::bad("Google 公钥响应无效"))?;
    verify_google(&tokens.id_token, &jwks, google, &pending.nonce)?;
    let mut response = new_session(&app, false).await?;
    response.headers_mut().append(
        header::SET_COOKIE,
        app.auth.oauth_cookie("", 0).parse().unwrap(),
    );
    *response.status_mut() = StatusCode::SEE_OTHER;
    response
        .headers_mut()
        .insert(header::LOCATION, "/".parse().unwrap());
    Ok(response)
}

fn verify_google(
    id_token: &str,
    jwks: &jsonwebtoken::jwk::JwkSet,
    google: &Google,
    nonce: &str,
) -> Result<(), ApiError> {
    let header = jsonwebtoken::decode_header(id_token).map_err(|_| ApiError::unauthorized())?;
    let key = header
        .kid
        .as_ref()
        .and_then(|kid| jwks.find(kid))
        .ok_or_else(ApiError::unauthorized)?;
    let key = jsonwebtoken::DecodingKey::from_jwk(key).map_err(|_| ApiError::unauthorized())?;
    let mut validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::RS256);
    validation.set_audience(&[&google.client_id]);
    validation.set_issuer(&["https://accounts.google.com", "accounts.google.com"]);
    let claims = jsonwebtoken::decode::<Claims>(id_token, &key, &validation)
        .map_err(|_| ApiError::unauthorized())?
        .claims;
    if claims.nonce != nonce
        || claims.sub.is_empty()
        || !claims.email_verified
        || !google.emails.contains(&claims.email.to_lowercase())
    {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "此 Google 账号没有访问权限".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    #[test]
    fn oidc_checks_signature_issuer_audience_expiry_nonce_and_allowlist() {
        // 此密钥专用于离线测试，不用于任何真实账号或部署。
        let key =
            jsonwebtoken::EncodingKey::from_rsa_pem(include_bytes!("fixtures/test-only-rsa.pem"))
                .unwrap();
        let jwks = serde_json::from_str(include_str!("fixtures/test-only-jwks.json")).unwrap();
        let google = Google {
            client_id: "test-client".into(),
            secret: "test-secret".into(),
            emails: vec!["allowed@example.invalid".into()],
        };
        let claims = json!({"iss":"https://accounts.google.com","aud":"test-client","sub":"test-user","exp":now()+300,"nonce":"test-nonce","email":"allowed@example.invalid","email_verified":true});
        let encode = |claims: &Value| {
            let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256);
            header.kid = Some("test-only".into());
            jsonwebtoken::encode(&header, claims, &key).unwrap()
        };
        let valid = encode(&claims);
        assert!(verify_google(&valid, &jwks, &google, "test-nonce").is_ok());
        assert!(verify_google(&valid, &jwks, &google, "other-nonce").is_err());
        for (field, value) in [
            ("iss", json!("https://other.example")),
            ("aud", json!("other-client")),
            ("exp", json!(now() - 300)),
            ("email", json!("other@example.invalid")),
            ("email_verified", json!(false)),
            ("sub", json!("")),
        ] {
            let mut altered = claims.clone();
            altered[field] = value;
            assert!(
                verify_google(&encode(&altered), &jwks, &google, "test-nonce").is_err(),
                "{field}"
            );
        }
        let forged = jsonwebtoken::encode(
            &jsonwebtoken::Header::default(),
            &claims,
            &jsonwebtoken::EncodingKey::from_secret(b"forged"),
        )
        .unwrap();
        assert!(verify_google(&forged, &jwks, &google, "test-nonce").is_err());
    }
}
