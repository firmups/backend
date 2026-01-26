use axum::http::StatusCode;
use axum::response::{Html, IntoResponse};
use log::{info, warn};
use std::path::PathBuf;
use std::{net::SocketAddr, sync::Arc};
use tokio::net::TcpListener;
use tokio::signal;

mod device;
mod device_key;
mod device_type;
mod device_type_firmware;
mod error;
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

async fn api_key_mw(
    axum::extract::State(state): axum::extract::State<RestApiConfig>,
    req: axum::http::Request<axum::body::Body>,
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
                    req.uri().path(),
                    peer
                );
            } else {
                warn!("unauthorized access to endpoint \"{}\"", req.uri().path());
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
        RestApi { config, router }
    }

    pub async fn start_blocking(&mut self) {
        let tcp = TcpListener::bind(self.config.listen_address)
            .await
            .expect("Failed to bind TCP listener");
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
        .expect("Server error");
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
