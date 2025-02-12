#[derive(Debug, serde::Deserialize)]
struct MainConfig {
    bind_address: std::net::SocketAddr,
    appcfg: wildcat::AppConfig,
    log_level: log::LevelFilter,
}

#[tokio::main]
async fn main() {
    let settings = config::Config::builder()
        .add_source(config::File::with_name("wildcat.toml"))
        .add_source(config::Environment::with_prefix("WILDCAT"))
        .build()
        .expect("Failed to build wildcat config");

    let maincfg: MainConfig = settings
        .try_deserialize()
        .expect("Failed to parse wildcat config");

    env_logger::builder().filter_level(maincfg.log_level).init();

    // we keep seed separate from the app config
    let seed = [0u8; 32];
    let app = wildcat::AppController::new(&seed, maincfg.appcfg).await;
    let router = wildcat::credit_routes(app);

    axum::Server::bind(&maincfg.bind_address)
        .serve(router.into_make_service())
        .await
        .unwrap();
}
