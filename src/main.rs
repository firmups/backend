use diesel_async::{
    AsyncPgConnection,
    pooled_connection::{AsyncDieselConnectionManager, bb8},
};
use dotenvy::dotenv;
use std::{net::SocketAddr, sync::Arc};

mod api;
mod codec;
mod crypto;
mod db;

type DbPool = bb8::Pool<AsyncPgConnection>;

#[tokio::main]
async fn main() {
    dotenv().ok();
    // initialize logging
    env_logger::init();

    // DB Pool setup
    let db_url = std::env::var("DATABASE_URL").unwrap();
    let config = AsyncDieselConnectionManager::<diesel_async::AsyncPgConnection>::new(db_url);
    let pool = DbPool::builder()
        .build(config)
        .await
        .expect("Failed to create pool");
    let shared_pool = Arc::new(pool);

    // CBOR API
    let cbor_addr: SocketAddr = "0.0.0.0:53585".parse().unwrap();
    let cbor_api_config = api::cbor::CborApiConfig {
        listen_address: cbor_addr,
        shared_pool: shared_pool.clone(),
    };
    let mut cbor_api = api::cbor::CborApi::new(cbor_api_config);
    cbor_api.start().await;

    // REST API
    let rest_addr: SocketAddr = "0.0.0.0:3000".parse().unwrap();
    let rest_api_config = api::rest::RestApiConfig {
        listen_address: rest_addr,
        shared_pool: shared_pool.clone(),
    };
    let mut rest_api = api::rest::RestApi::new(rest_api_config);
    rest_api.start_blocking().await;
    cbor_api.shutdown().await;
}
