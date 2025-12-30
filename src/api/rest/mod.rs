use axum::Json;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse};
use log::{error, info, warn};
use serde::Serialize;
use std::fmt;
use std::path::PathBuf;
use std::{net::SocketAddr, sync::Arc};
use thiserror::Error;
use tokio::net::TcpListener;
use tokio::signal;
use uuid::Uuid;

mod device;
mod device_key;
mod device_type;
mod device_type_firmware;
mod firmware;
mod serde_helpers;

#[derive(Clone)]
pub struct RestApiConfig {
    pub listen_address: SocketAddr,
    pub shared_pool: Arc<crate::DbPool>,
    pub max_firmware_size: usize,
    pub data_storage_location: PathBuf,
    pub api_key: String,
}

pub struct RestApi {
    config: RestApiConfig,
    router: axum::Router,
}

#[derive(Serialize, Debug)]
pub struct InternalErrorBody {
    error_id: String,
}

#[derive(Serialize, Debug)]
pub struct ErrorBody {
    error: String,
}

#[derive(Error, Debug)]
pub enum TransactionError {
    #[error(transparent)]
    Db(#[from] diesel::result::Error),

    #[error(transparent)]
    Api(#[from] ApiError),
}

#[derive(Error, Debug)]
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

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ApiError::Client { body, .. } => write!(f, "{body}"),
            ApiError::Internal { body, .. } => write!(f, "{body}"),
        }
    }
}

impl fmt::Display for ErrorBody {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "error: {}", self.error)
    }
}
impl fmt::Display for InternalErrorBody {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "error_id: {}", self.error_id)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        match self {
            ApiError::Client { status, body } => (status, Json(body)).into_response(),
            ApiError::Internal { status, body } => (status, Json(body)).into_response(),
        }
    }
}

async fn api_key_mw(
    axum::extract::State(state): axum::extract::State<RestApiConfig>,
    mut req: axum::http::Request<axum::body::Body>,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let unauthorized = || {
        (
            StatusCode::UNAUTHORIZED,
            [("www-authenticate", r#"ApiKey realm="api""#)],
            "missing or invalid x-api-key",
        )
            .into_response()
    };

    let key = req.headers().get("x-api-key").and_then(|v| v.to_str().ok());
    match key {
        Some(k) if state.api_key == k => next.run(req).await,
        _ => {
            let peer_opt: Option<SocketAddr> = req
                .extensions()
                .get::<axum::extract::ConnectInfo<SocketAddr>>()
                .map(|ci| ci.0);
            if let Some(peer) = peer_opt {
                warn!(
                    "unauthorized access to endpoint \"{}\" from \"{:?}\"",
                    req.uri().path().to_string(),
                    peer
                );
            } else {
                warn!(
                    "unauthorized access to endpoint \"{}\"",
                    req.uri().path().to_string()
                );
            }

            unauthorized()
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
            .route(
                "/device/{id}/key",
                axum::routing::get(device_key::list_device_keys),
            )
            .route(
                "/device/{id}/key",
                axum::routing::post(device_key::create_device_key),
            )
            .route(
                "/device/{id}/key/{id}",
                axum::routing::get(device_key::get_device_key),
            )
            .route(
                "/device/{id}/key/{id}",
                axum::routing::delete(device_key::delete_device_key),
            )
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
            .with_state(config.clone())
            .layer(axum::middleware::from_fn_with_state(
                config.clone(),
                api_key_mw,
            )); // apply globally
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
        axum::serve(
            tcp,
            self.router
                .clone()
                .into_make_service_with_connect_info::<SocketAddr>(),
        )
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
