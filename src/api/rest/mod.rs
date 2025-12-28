use aes_gcm::Error;
use axum::Json;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse};
use log::{error, info};
use serde::Serialize;
use std::path::PathBuf;
use std::{net::SocketAddr, sync::Arc};
use tokio::net::TcpListener;
use tokio::signal;
use uuid::Uuid;

mod device;
mod device_type;
mod device_type_firmware;
mod firmware;

#[derive(Clone)]
pub struct RestApiConfig {
    pub listen_address: SocketAddr,
    pub shared_pool: Arc<crate::DbPool>,
    pub max_firmware_size: usize,
    pub data_storage_location: PathBuf,
}

pub struct RestApi {
    config: RestApiConfig,
    router: axum::Router,
}

#[derive(Serialize)]
struct InternalErrorBody {
    error_id: String,
}

#[derive(Serialize)]
struct ErrorBody {
    error: String,
}

pub enum ApiError {
    Client {
        status: StatusCode,
        body: ErrorBody,
    },
    Internal {
        status: StatusCode,
        body: InternalErrorBody,
    },
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        match self {
            ApiError::Client { status, body } => (status, Json(body)).into_response(),
            ApiError::Internal { status, body } => (status, Json(body)).into_response(),
        }
    }
}

impl RestApi {
    pub fn new(config: RestApiConfig) -> Self {
        let router = axum::Router::new()
            .route("/", axum::routing::get(welcome_page))
            .route(
                "/device_type",
                axum::routing::post(device_type::create_device_type),
            )
            .route(
                "/device_type",
                axum::routing::get(device_type::list_device_types),
            )
            .route(
                "/device_type/{id}",
                axum::routing::get(device_type::get_device_type),
            )
            .route(
                "/device_type/{id}",
                axum::routing::patch(device_type::update_device_type),
            )
            .route(
                "/device_type/{id}",
                axum::routing::delete(device_type::delete_device_type),
            )
            .route("/device", axum::routing::get(device::list_devices))
            .route("/device", axum::routing::post(device::create_device))
            .route("/device/{id}", axum::routing::get(device::get_device))
            .route("/device/{id}", axum::routing::patch(device::update_device))
            .route("/device/{id}", axum::routing::delete(device::delete_device))
            .route("/firmware", axum::routing::get(firmware::list_firmwares))
            .route(
                "/firmware",
                axum::routing::post(firmware::create_firmware).route_layer(
                    axum::extract::DefaultBodyLimit::max(config.max_firmware_size),
                ),
            )
            .route("/firmware/{id}", axum::routing::get(firmware::get_firmware))
            .route(
                "/firmware/{id}",
                axum::routing::delete(firmware::delete_firmware),
            )
            .route(
                "/firmware/{id}/download",
                axum::routing::get(firmware::get_firmware_file),
            )
            .route(
                "/firmware/{id}/download",
                axum::routing::head(firmware::get_firmware_file_metadata),
            )
            .route(
                "/device_type_firmware",
                axum::routing::get(device_type_firmware::list_device_type_firmwares),
            )
            .route(
                "/device_type_firmware",
                axum::routing::post(device_type_firmware::create_device_type_firmware),
            )
            .route(
                "/device_type_firmware/{id}",
                axum::routing::get(device_type_firmware::get_device_type_firmware),
            )
            .route(
                "/device_type_firmware/{id}",
                axum::routing::delete(device_type_firmware::delete_device_type_firmware),
            )
            .with_state(config.clone());
        RestApi {
            config: config,
            router: router,
        }
    }

    pub async fn start_blocking(&mut self) {
        let tcp = TcpListener::bind(self.config.listen_address).await.unwrap();
        info!(
            "HTTP listening on {}:{}",
            self.config.listen_address.ip(),
            self.config.listen_address.port()
        );
        axum::serve(tcp, self.router.clone().into_make_service())
            .with_graceful_shutdown(async {
                let _ = signal::ctrl_c().await;
                info!("CTRL+C received; shutting down");
            })
            .await
            .unwrap();
    }
}

#[axum::debug_handler]
async fn welcome_page() -> Html<&'static str> {
    Html(
        r#"
        <!DOCTYPE html>
        <html>
        <head>
            <title>Welcome</title>
            <style>
                body { font-family: Arial, sans-serif; text-align: center; margin-top: 50px; }
                h1 { color: #004F31; }
            </style>
        </head>
        <body>
            <h1>Welcome to the FIRMUPS backend!</h1>
            <p>Please use the REST-API to interract with it.</p>
        </body>
        </html>
    "#,
    )
}

fn client_error(status_code: StatusCode, err: String) -> ApiError {
    error!("Client Error: {err}");
    ApiError::Client {
        status: status_code,
        body: ErrorBody { error: err },
    }
}

fn internal_error<E>(err: E) -> ApiError
where
    E: std::error::Error,
{
    let error_id = Uuid::new_v4();

    // Log the UUID and the error message for correlation
    let error_id_string = error_id.to_string();
    let error_string = err.to_string();
    error!("[{error_id_string}] Internal error: {error_string}");

    ApiError::Internal {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        body: InternalErrorBody {
            error_id: error_id_string,
        },
    }
}
