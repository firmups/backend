use diesel_async::{
    AsyncPgConnection,
    pooled_connection::{AsyncDieselConnectionManager, bb8},
};
use dotenvy::dotenv;
use log::{error, info};
use std::fs;
use std::{net::SocketAddr, path::PathBuf, sync::Arc};
use storage::Storage;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

mod api;
mod db;
mod storage;

type DbPool = bb8::Pool<AsyncPgConnection>;

#[tokio::main]
async fn main() {
    dotenv().ok();

    // DB Pool setup
    let db_url = std::env::var("FIRMUPS_DATABASE_URL").expect("FIRMUPS_DATABASE_URL environment variable is missing. Please set it before running the app.");
    let config = AsyncDieselConnectionManager::<diesel_async::AsyncPgConnection>::new(db_url);
    let pool = DbPool::builder()
        .build(config)
        .await
        .expect("Failed to create pool");
    let shared_pool = Arc::new(pool);

    // Storage setup
    let storage_config: storage::StorageConfig;
    let storage_type_env = std::env::var("FIRMUPS_STORAGE_TYPE").expect("FIRMUPS_STORAGE_TYPE environment variable is missing. Please set it before running the app.");
    if storage_type_env.to_lowercase() == "s3" {
        info!("Using S3 storage backend.");
        storage_config = storage::StorageConfig::S3 {
            endpoint: std::env::var("FIRMUPS_STORAGE_S3_ENDPOINT").expect("FIRMUPS_STORAGE_S3_ENDPOINT environment variable is missing. Please set it before running the app."),
            bucket: std::env::var("FIRMUPS_STORAGE_S3_BUCKET").expect("FIRMUPS_STORAGE_S3_BUCKET environment variable is missing. Please set it before running the app."),
            access_key: std::env::var("FIRMUPS_STORAGE_S3_KEY_ID").expect("FIRMUPS_STORAGE_S3_KEY_ID environment variable is missing. Please set it before running the app."),
            secret_key: std::env::var("FIRMUPS_STORAGE_S3_ACCESS_KEY").expect("FIRMUPS_STORAGE_S3_ACCESS_KEY environment variable is missing. Please set it before running the app."),
        };
    } else if storage_type_env.to_lowercase() == "local" {
        info!("Using local storage backend.");
        storage_config = storage::StorageConfig::Local {
            path: std::env::var("FIRMUPS_STORAGE_LOCAL_PATH").expect("FIRMUPS_STORAGE_LOCAL_PATH environment variable is missing. Please set it before running the app."),
        };
    } else {
        error!(
            "Invalid FIRMUPS_STORAGE_TYPE '{}'. Must be 's3' or 'local'.",
            storage_type_env
        );
        std::process::exit(1);
    }

    let storage: Arc<Box<dyn Storage>> = storage::create_storage(storage_config.clone())
        .await
        .expect("Failed to create storage backend")
        .into();

    if let storage::StorageConfig::Local { path: src_path } = &storage_config.clone() {
        let data_path = PathBuf::from(src_path);
        // Ensure data directory exists
        if let Err(e) = fs::create_dir_all(&data_path) {
            eprintln!("Failed to create data directory {:?}: {}", data_path, e);
            std::process::exit(1);
        }

        let log_path_env = std::env::var("FIRMUPS_LOG_PATH");
        let log_path: PathBuf = match log_path_env {
            Ok(path) => PathBuf::from(path),
            Err(_) => {
                println!("FIRMUPS_LOG_PATH not set using default: ${{FIRMUPS_DATA_PATH}}/logs");
                data_path.join("logs")
            }
        };
        // Ensure log directory exists
        if let Err(e) = fs::create_dir_all(&log_path) {
            eprintln!("Failed to create log directory {:?}: {}", log_path, e);
            std::process::exit(1);
        }

        let max_log_days: usize = std::env::var("FIRMUPS_MAX_LOG_DAYS")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(7);
        let file_appender = RollingFileAppender::builder()
            .rotation(Rotation::DAILY)
            .filename_prefix("firmups")
            .filename_suffix("log")
            .max_log_files(max_log_days)
            .build(log_path)
            .expect("create rolling file");

        tracing_subscriber::registry()
            .with(EnvFilter::from_default_env())
            .with(fmt::layer().with_writer(std::io::stderr)) // console
            .with(fmt::layer().with_ansi(false).with_writer(file_appender)) // file
            .init();

        info!("Logging initialized.");
    } else {
        error!(
            "Logging to file is only supported for local storage. Please set FIRMUPS_STORAGE_TYPE to 'local' if you want file logging."
        );
        std::process::exit(1);
    }

    // CBOR API
    let cbor_addr: SocketAddr = "0.0.0.0:53585".parse().unwrap();
    let cbor_api_config = api::cbor::CborApiConfig {
        listen_address: cbor_addr,
        shared_pool: shared_pool.clone(),
        storage: storage.clone(),
    };
    let mut cbor_api = api::cbor::CborApi::new(cbor_api_config);
    cbor_api.start().await;

    // REST API
    let rest_addr: SocketAddr = "0.0.0.0:3000".parse().unwrap();

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

    let api_key_env = std::env::var("FIRMUPS_API_KEY");
    let api_key: String = match api_key_env {
        Ok(key) => {
            if key.len() < 16 {
                error!("API key to short. Needs to be at least 16 characters");
                return;
            };
            key
        }
        Err(_) => {
            info!("FIRMUPS_API_KEY not set generating random key...");
            const CHARSET: &[u8; 62] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ\
                              abcdefghijklmnopqrstuvwxyz\
                              0123456789";
            const LENGTH: usize = 32;
            let mut buf = [0u8; LENGTH];
            getrandom::fill(&mut buf).expect("Failed to get random bytes");

            let key: String = buf
                .iter()
                .map(|&b| {
                    let idx = (b as usize) % CHARSET.len();
                    CHARSET[idx] as char
                })
                .collect();

            info!("Generated api_key {}", key);
            key
        }
    };

    let rest_api_config = api::rest::RestApiConfig {
        listen_address: rest_addr,
        shared_pool: shared_pool.clone(),
        storage: storage.clone(),
        max_firmware_size,
        api_key,
    };
    let mut rest_api = api::rest::RestApi::new(rest_api_config);
    rest_api.start_blocking().await;
    cbor_api.shutdown().await;
}
