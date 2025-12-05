use crate::db::models::{DeviceType, NewDeviceType};
use crate::db::schema::device_type::dsl::device_type;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::http::HeaderValue;
use axum::http::StatusCode;
use axum::response::Html;
use axum::response::IntoResponse;
use axum::{Json, routing};
use diesel::QueryDsl;
use diesel::SelectableHelper;
use diesel::result::DatabaseErrorKind;
use diesel_async::RunQueryDsl;
use log::debug;
use log::info;
use std::{net::SocketAddr, sync::Arc};
use tokio::net::TcpListener;
use tokio::signal;

use crate::{DbPool, db::models::Device};

pub struct RestApiConfig {
    pub listen_address: SocketAddr,
    pub shared_pool: Arc<crate::DbPool>,
}

pub struct RestApi {
    config: RestApiConfig,
    router: axum::Router,
}

impl RestApi {
    pub fn new(config: RestApiConfig) -> Self {
        let router = axum::Router::new()
            .route("/", axum::routing::get(welcome_page))
            .route("/device", axum::routing::get(get_devices))
            .route("/device_type", axum::routing::post(create_device_type))
            .route("/device_type", axum::routing::get(get_device_types))
            .with_state(config.shared_pool.clone());
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

#[axum::debug_handler]
pub async fn create_device_type(
    State(pool): State<Arc<DbPool>>,
    Json(payload): Json<crate::db::models::NewDeviceType>,
) -> impl IntoResponse {
    // Basic validation
    let name_trimmed = payload.name.trim();
    if name_trimmed.is_empty() {
        return (StatusCode::BAD_REQUEST, "name cannot be empty").into_response();
    }
    if name_trimmed.len() > 100 {
        return (StatusCode::BAD_REQUEST, "name too long (max 100)").into_response();
    }

    // Insert
    let mut conn = match pool.get().await {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                format!("db pool error: {e}"),
            )
                .into_response();
        }
    };

    // Build insertable struct (in case you trimmed or normalized)
    let new_row = NewDeviceType {
        name: name_trimmed.to_string(),
    };

    // Perform the insert and return the created row
    let result: Result<DeviceType, diesel::result::Error> = diesel::insert_into(device_type)
        .values(&new_row)
        .returning(DeviceType::as_returning())
        .get_result(&mut conn)
        .await;

    match result {
        Ok(created) => (
            StatusCode::CREATED,
            [(
                axum::http::header::LOCATION,
                HeaderValue::from_str(&format!("/device-types/{}", created.id)).unwrap(),
            )],
            Json(created),
        )
            .into_response(),
        Err(diesel::result::Error::DatabaseError(kind, info)) => {
            // Handle uniqueness violation nicely (if you have a unique index on name)
            if kind == DatabaseErrorKind::UniqueViolation {
                (
                    StatusCode::CONFLICT,
                    format!("device type '{}' already exists", name_trimmed),
                )
                    .into_response()
            } else {
                (
                    StatusCode::BAD_REQUEST,
                    format!("db error: {}", info.message()),
                )
                    .into_response()
            }
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("insert failed: {e}"),
        )
            .into_response(),
    }
}

#[axum::debug_handler]
async fn get_device_types(
    State(shared_pool): State<Arc<DbPool>>,
) -> Result<Json<Vec<DeviceType>>, (axum::http::StatusCode, String)> {
    use crate::db::schema::device::dsl::*;
    debug!("get_device_types called");

    let mut conn = shared_pool
        .clone()
        .get_owned()
        .await
        .map_err(internal_error)?;
    let result = device_type
        .select(DeviceType::as_select())
        .load(&mut conn)
        .await
        .map_err(internal_error)?;

    Ok(Json(result))
}

#[axum::debug_handler]
async fn get_devices(
    State(shared_pool): State<Arc<DbPool>>,
) -> Result<Json<Vec<Device>>, (axum::http::StatusCode, String)> {
    use crate::db::schema::device::dsl::*;
    debug!("get_devices called");

    let mut conn = shared_pool
        .clone()
        .get_owned()
        .await
        .map_err(internal_error)?;
    let result = device
        .select(Device::as_select())
        .load(&mut conn)
        .await
        .map_err(internal_error)?;

    Ok(Json(result))
}

fn internal_error<E>(err: E) -> (StatusCode, String)
where
    E: std::error::Error,
{
    (StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
}
