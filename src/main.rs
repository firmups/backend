use diesel_async::{
    AsyncPgConnection,
    pooled_connection::{AsyncDieselConnectionManager, bb8},
};
use dotenvy::dotenv;
use log::info;
use std::{net::SocketAddr, path::PathBuf, sync::Arc};

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
    let db_url = std::env::var("FIRMUPS_DATABASE_URL").expect("FIRMUPS_DATABASE_URL environment variable is missing. Please set it before running the app.");
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

    let data_path_env = std::env::var("FIRMUPS_DATA_PATH");
    let data_path: PathBuf = match data_path_env {
        Ok(path) => PathBuf::from(path),
        Err(_) => {
            info!("FIRMUPS_DATA_PATH not set using default: ./data/");
            PathBuf::from("./data")
        }
    };

    let max_firmware_size_env = std::env::var("FIRMUPS_FIRMWARE_MAX_SIZE_BYTES");
    let max_firmware_size: usize = match max_firmware_size_env {
        Ok(size) => {
            let max_size: usize = size
                .parse::<usize>()
                .map_err(|_| format!("Invalid number in MAX_UPLOAD_SIZE: '{}'", size))
                .unwrap();
            max_size
        }
        Err(_) => {
            info!("FIRMUPS_FIRMWARE_MAX_SIZE_BYTES not set using default: 1Gb");
            1024 * 1024 * 1024 //1Gb
        }
    };

    let rest_api_config = api::rest::RestApiConfig {
        listen_address: rest_addr,
        shared_pool: shared_pool.clone(),
        data_storage_location: data_path,
        max_firmware_size: max_firmware_size,
    };
    let mut rest_api = api::rest::RestApi::new(rest_api_config);
    rest_api.start_blocking().await;
    cbor_api.shutdown().await;
}
