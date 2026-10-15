// greenhouse-body-double — systemd-owned in production: binds its provisioned
// loopback port, registers with its app-bound credential, and answers
// kernel-dispatched tool calls and the check-in schedule.

use body_double_app::{build_app, config::AppConfig, register, APP_VERSION};
use std::time::Duration;

#[tokio::main]
async fn main() {
    println!("=== BODY DOUBLE {APP_VERSION} ===");
    let cfg = match AppConfig::from_env() {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("[body-double] configuration refused: {e}");
            std::process::exit(78);
        }
    };
    if cfg.service_token.is_empty() {
        eprintln!("[body-double] no registration credential; refusing to start");
        std::process::exit(78);
    }
    let addr = format!("127.0.0.1:{}", cfg.port);
    println!(
        "[body-double] default zone {}; notices for {}",
        cfg.timezone,
        cfg.notices_for.as_deref().unwrap_or("nobody")
    );
    let (router, app) = match build_app(cfg) {
        Ok(built) => built,
        Err(e) => {
            eprintln!("[body-double] the journal could not be opened: {e}");
            std::process::exit(78);
        }
    };
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| panic!("bind {addr}: {e}"));
    println!("[body-double] internal app API on http://{addr}");

    let reg_app = app.clone();
    tokio::spawn(async move {
        let mut delay = 1;
        loop {
            match register(&reg_app).await {
                Ok(()) => {
                    println!("[body-double] registered with kernel");
                    break;
                }
                Err(e) => {
                    eprintln!("[body-double] registration failed ({e}) — retry in {delay}s");
                    tokio::time::sleep(Duration::from_secs(delay)).await;
                    delay = (delay * 2).min(60);
                }
            }
        }
    });

    axum::serve(listener, router).await.expect("serve");
}
