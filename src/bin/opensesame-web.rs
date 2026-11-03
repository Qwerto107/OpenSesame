#[tokio::main]
async fn main() {
    let result = if std::env::args().nth(1).as_deref() == Some("healthcheck") {
        healthcheck().await
    } else {
        opensesame::web::run().await
    };
    if let Err(error) = result {
        eprintln!("OpenSesame Web: {error}");
        std::process::exit(1);
    }
}

async fn healthcheck() -> Result<(), Box<dyn std::error::Error>> {
    let mut address: std::net::SocketAddr = std::env::var("OPENSESAME_WEB_LISTEN")
        .unwrap_or_else(|_| "127.0.0.1:18010".into())
        .parse()?;
    // 容器监听所有网卡时，健康检查仍通过本机回环访问。
    if address.ip().is_unspecified() {
        address.set_ip(if address.is_ipv4() {
            std::net::Ipv4Addr::LOCALHOST.into()
        } else {
            std::net::Ipv6Addr::LOCALHOST.into()
        });
    }
    reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(3))
        .build()?
        .get(format!("http://{address}/auth/status"))
        .send()
        .await?
        .error_for_status()?;
    Ok(())
}
