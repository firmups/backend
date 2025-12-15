use crate::db::models::{
    Device, DeviceType, DeviceTypeFirmware, Firmware, NewDevice, NewDeviceType,
    NewDeviceTypeFirmware, NewFirmware,
};
use axum::body::Body;
use axum::extract::{Multipart, Path, State};
use axum::http::HeaderMap;
use axum::http::HeaderValue;
use axum::http::StatusCode;
use axum::response::Html;
use axum::response::IntoResponse;
use axum::{Json, routing};
use diesel::QueryDsl;
use diesel::SelectableHelper;
use diesel::dsl::count_star;
use diesel::prelude::*;
use diesel::result::DatabaseErrorKind;
use diesel_async::RunQueryDsl;
use log::info;
use log::{debug, warn};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::{net::SocketAddr, sync::Arc};
use tokio::fs;
use tokio::net::TcpListener;
use tokio::signal;
use tokio_util::io::ReaderStream;
use uuid::Uuid;

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

impl RestApi {
    pub fn new(config: RestApiConfig) -> Self {
        let router = axum::Router::new()
            .route("/", axum::routing::get(welcome_page))
            .route("/device_type", axum::routing::post(create_device_type))
            .route("/device_type", axum::routing::get(get_device_types))
            .route("/device_type/{id}", axum::routing::get(get_device_type))
            .route(
                "/device_type/{id}",
                axum::routing::delete(delete_device_type),
            )
            .route("/device", axum::routing::get(get_devices))
            .route("/device", axum::routing::post(create_device))
            .route("/device/{id}", axum::routing::get(get_device))
            .route("/device/{id}", axum::routing::delete(delete_device))
            .route("/firmware", axum::routing::get(get_firmwares))
            .route(
                "/firmware",
                axum::routing::post(create_firmware).route_layer(
                    axum::extract::DefaultBodyLimit::max(config.max_firmware_size),
                ),
            )
            .route("/firmware/{id}", axum::routing::get(get_firmware))
            .route("/firmware/{id}", axum::routing::delete(delete_firmware))
            .route(
                "/firmware/{id}/download",
                axum::routing::get(get_firmware_file),
            )
            .route(
                "/firmware/{id}/download",
                axum::routing::head(get_firmware_file_metadata),
            )
            .route(
                "/device_type_firmware",
                axum::routing::get(get_device_type_firmwares),
            )
            .route(
                "/device_type_firmware",
                axum::routing::post(create_device_type_firmware),
            )
            .route(
                "/device_type_firmware/{id}",
                axum::routing::get(get_device_type_firmware),
            )
            .route(
                "/device_type_firmware/{id}",
                axum::routing::delete(delete_device_type_firmware),
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

#[axum::debug_handler]
pub async fn create_device_type(
    State(api_config): State<RestApiConfig>,
    Json(payload): Json<NewDeviceType>,
) -> impl IntoResponse {
    use crate::db::schema::device_type::dsl::*;
    // Basic validation
    let name_trimmed = payload.name.trim();
    if name_trimmed.is_empty() {
        return (StatusCode::BAD_REQUEST, "name cannot be empty").into_response();
    }
    if name_trimmed.len() > 100 {
        return (StatusCode::BAD_REQUEST, "name too long (max 100)").into_response();
    }

    // Insert
    let mut conn = match api_config.shared_pool.get().await {
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
    State(api_config): State<RestApiConfig>,
) -> Result<Json<Vec<DeviceType>>, (axum::http::StatusCode, String)> {
    use crate::db::schema::device_type::dsl::*;
    debug!("get_device_types called");

    let mut conn = api_config
        .shared_pool
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
async fn get_device_type(
    State(api_config): State<RestApiConfig>,
    Path(path_id): Path<i32>,
) -> Result<Json<DeviceType>, (axum::http::StatusCode, String)> {
    use crate::db::schema::device_type::dsl::*;
    debug!("get_device_type called");

    let mut conn = api_config
        .shared_pool
        .clone()
        .get_owned()
        .await
        .map_err(internal_error)?;
    let result = device_type
        .select(DeviceType::as_select())
        .filter(id.eq(path_id))
        .first(&mut conn)
        .await
        .map_err(internal_error)?;

    Ok(Json(result))
}

#[axum::debug_handler]
pub async fn delete_device_type(
    State(api_config): State<RestApiConfig>,
    Path(path_id): Path<i32>,
) -> Result<Json<DeviceType>, (axum::http::StatusCode, String)> {
    use crate::db::schema::device_type::dsl::*;
    debug!("delete_device_type called: id={}", path_id);

    let mut conn = api_config
        .shared_pool
        .clone()
        .get_owned()
        .await
        .map_err(internal_error)?;

    let deleted: Result<DeviceType, diesel::result::Error> =
        diesel::delete(device_type.filter(id.eq(path_id)))
            .returning(DeviceType::as_returning())
            .get_result(&mut conn)
            .await;

    match deleted {
        Ok(row) => Ok(Json(row)),
        Err(diesel::result::Error::NotFound) => Err((
            axum::http::StatusCode::NOT_FOUND,
            format!("device_type {} not found", path_id),
        )),
        Err(e) => Err(internal_error(e)),
    }
}

#[axum::debug_handler]
async fn get_devices(
    State(api_config): State<RestApiConfig>,
) -> Result<Json<Vec<Device>>, (axum::http::StatusCode, String)> {
    use crate::db::schema::device::dsl::*;
    debug!("get_devices called");

    let mut conn = api_config
        .shared_pool
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

#[axum::debug_handler]
pub async fn create_device(
    State(api_config): State<RestApiConfig>,
    Json(payload): Json<NewDevice>,
) -> impl IntoResponse {
    use crate::db::schema::device::dsl as device_dsl;
    use crate::db::schema::device_type::dsl as device_type_dsl;
    use crate::db::schema::device_type_firmware::dsl as device_type_firmware_dsl;
    // Basic validation
    let name_trimmed = payload.name;
    if name_trimmed.is_empty() {
        return (StatusCode::BAD_REQUEST, "name cannot be empty").into_response();
    }
    if name_trimmed.len() > 100 {
        return (StatusCode::BAD_REQUEST, "name too long (max 100)").into_response();
    }

    let mut conn = match api_config.shared_pool.get().await {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                format!("db pool error: {e}"),
            )
                .into_response();
        }
    };

    let mut count: i64 = match device_type_dsl::device_type
        .filter(device_type_dsl::id.eq(payload.type_))
        .select(count_star())
        .first(&mut conn)
        .await
    {
        Ok(res) => res,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, format!("db error: {}", e)).into_response();
        }
    };
    if count == 0 {
        return (StatusCode::BAD_REQUEST, "invalid device type").into_response();
    }

    if !payload.firmware.is_none() {
        count = match device_type_firmware_dsl::device_type_firmware
            .filter(device_type_firmware_dsl::device_type.eq(payload.type_))
            .filter(device_type_firmware_dsl::firmware.eq(payload.firmware.unwrap()))
            .select(count_star())
            .first(&mut conn)
            .await
        {
            Ok(res) => res,
            Err(e) => {
                return (StatusCode::BAD_REQUEST, format!("db error: {}", e)).into_response();
            }
        };
        if count == 0 {
            return (StatusCode::BAD_REQUEST, "invalid firmware for device_type").into_response();
        }
    }

    count = match device_type_firmware_dsl::device_type_firmware
        .filter(device_type_firmware_dsl::device_type.eq(payload.type_))
        .filter(device_type_firmware_dsl::firmware.eq(payload.desired_firmware))
        .select(count_star())
        .first(&mut conn)
        .await
    {
        Ok(res) => res,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, format!("db error: {}", e)).into_response();
        }
    };
    if count == 0 {
        return (
            StatusCode::BAD_REQUEST,
            "invalid desired_firmware for device_type",
        )
            .into_response();
    }

    let new_row = NewDevice {
        name: name_trimmed.to_string(),
        type_: payload.type_,
        firmware: payload.firmware,
        desired_firmware: payload.desired_firmware,
        status: payload.status,
    };

    // Perform the insert and return the created row
    let result: Result<Device, diesel::result::Error> = diesel::insert_into(device_dsl::device)
        .values(&new_row)
        .returning(Device::as_returning())
        .get_result(&mut conn)
        .await;

    match result {
        Ok(created) => (
            StatusCode::CREATED,
            [(
                axum::http::header::LOCATION,
                HeaderValue::from_str(&format!("/device/{}", created.id)).unwrap(),
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
async fn get_device(
    State(api_config): State<RestApiConfig>,
    Path(path_id): Path<i32>,
) -> Result<Json<Device>, (axum::http::StatusCode, String)> {
    use crate::db::schema::device::dsl::*;
    debug!("get_device called: id={}", path_id);

    let mut conn = api_config
        .shared_pool
        .clone()
        .get_owned()
        .await
        .map_err(internal_error)?;
    let result = device
        .select(Device::as_select())
        .filter(id.eq(path_id))
        .first(&mut conn)
        .await
        .map_err(internal_error)?;

    Ok(Json(result))
}

#[axum::debug_handler]
pub async fn delete_device(
    State(api_config): State<RestApiConfig>,
    Path(path_id): Path<i32>,
) -> Result<Json<Device>, (axum::http::StatusCode, String)> {
    use crate::db::schema::device::dsl::*;
    debug!("delete_device called: id={}", path_id);

    let mut conn = api_config
        .shared_pool
        .clone()
        .get_owned()
        .await
        .map_err(internal_error)?;

    let deleted: Result<Device, diesel::result::Error> =
        diesel::delete(device.filter(id.eq(path_id)))
            .returning(Device::as_returning())
            .get_result(&mut conn)
            .await;

    match deleted {
        Ok(row) => Ok(Json(row)),
        Err(diesel::result::Error::NotFound) => Err((
            axum::http::StatusCode::NOT_FOUND,
            format!("device {} not found", path_id),
        )),
        Err(e) => Err(internal_error(e)),
    }
}

#[axum::debug_handler]
async fn get_firmwares(
    State(api_config): State<RestApiConfig>,
) -> Result<Json<Vec<Firmware>>, (axum::http::StatusCode, String)> {
    use crate::db::schema::firmware::dsl::*;
    debug!("get_devices called");

    let mut conn = api_config
        .shared_pool
        .clone()
        .get_owned()
        .await
        .map_err(internal_error)?;
    let result = firmware
        .select(Firmware::as_select())
        .load(&mut conn)
        .await
        .map_err(internal_error)?;

    Ok(Json(result))
}

#[axum::debug_handler]
pub async fn create_firmware(
    State(api_config): State<RestApiConfig>,
    mut multipart: Multipart,
) -> impl IntoResponse {
    use crate::db::schema::firmware::dsl::*;

    let mut in_name: Option<String> = None;
    let mut in_version: Option<String> = None;
    let mut in_file_name: Option<String> = None;
    let mut in_file_bytes: Option<Vec<u8>> = None;

    while let Some(field) = multipart.next_field().await.unwrap_or(None) {
        let field_name = field.name().unwrap_or("").to_string();
        match field_name.as_str() {
            "name" => {
                in_name = Some(field.text().await.unwrap_or_default());
            }
            "version" => {
                in_version = Some(field.text().await.unwrap_or_default());
            }
            "file" => {
                // Use original filename if available
                in_file_name = field.file_name().map(|s| s.to_string());
                in_file_bytes = Some(field.bytes().await.unwrap_or_default().to_vec());
            }
            _ => {}
        }
    }

    // Basic validation

    let in_name = match in_name {
        Some(v) if !v.is_empty() => v,
        _ => return (StatusCode::BAD_REQUEST, "name cannot be empty").into_response(),
    };

    if in_name.len() > 100 {
        return (StatusCode::BAD_REQUEST, "name too long (max 100)").into_response();
    }

    let in_version = match in_version {
        Some(v) if !v.is_empty() => v,
        _ => return (StatusCode::BAD_REQUEST, "version cannot be empty").into_response(),
    };

    if in_version.len() > 100 {
        return (StatusCode::BAD_REQUEST, "version too long (max 100)").into_response();
    }

    let file = match in_file_bytes {
        Some(f) if !f.is_empty() => f,
        _ => return (StatusCode::BAD_REQUEST, "firmware file required").into_response(),
    };

    // Compute metadata
    let in_size = file.len() as i64;
    let in_sha256 = {
        let mut hasher = Sha256::new();
        hasher.update(&file);
        Some(format!("{:x}", hasher.finalize()))
    };

    let new_firmware = NewFirmware {
        name: in_name,
        version: in_version,
        file_id: Uuid::new_v4().to_string(),
        size: in_size,
        sha256: in_sha256.unwrap(),
    };

    let safe_name = format!("{}.bin", new_firmware.file_id);
    let mut path = api_config.data_storage_location;
    path.push("firmware");
    fs::create_dir_all(&path)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "mkdir failed"))
        .unwrap();
    path.push(&safe_name);
    fs::write(&path, &file)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "write failed"))
        .unwrap();

    let mut conn = match api_config.shared_pool.get().await {
        Ok(c) => c,
        Err(e) => {
            let _ = fs::remove_file(&path).await;
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                format!("db pool error: {e}"),
            )
                .into_response();
        }
    };

    let inserted: Result<Firmware, diesel::result::Error> = diesel::insert_into(firmware)
        .values(&new_firmware)
        .returning(Firmware::as_returning())
        .get_result(&mut conn)
        .await;
    match inserted {
        Ok(record) => (StatusCode::CREATED, axum::Json(record)).into_response(),
        Err(_) => {
            let _ = fs::remove_file(&path).await;
            (StatusCode::INTERNAL_SERVER_ERROR, "insert failed").into_response()
        }
    }
}

#[axum::debug_handler]
async fn get_firmware(
    State(api_config): State<RestApiConfig>,
    Path(path_id): Path<i32>,
) -> Result<Json<Firmware>, (axum::http::StatusCode, String)> {
    use crate::db::schema::firmware::dsl::*;
    debug!("get_firmware called: id={}", path_id);

    let mut conn = api_config
        .shared_pool
        .clone()
        .get_owned()
        .await
        .map_err(internal_error)?;
    let result = firmware
        .select(Firmware::as_select())
        .filter(id.eq(path_id))
        .first(&mut conn)
        .await
        .map_err(internal_error)?;

    Ok(Json(result))
}

#[axum::debug_handler]
pub async fn delete_firmware(
    State(api_config): State<RestApiConfig>,
    Path(path_id): Path<i32>,
) -> Result<Json<Firmware>, (axum::http::StatusCode, String)> {
    use crate::db::schema::firmware::dsl::*;
    debug!("delete_firmware called: id={}", path_id);

    let mut conn = api_config
        .shared_pool
        .clone()
        .get_owned()
        .await
        .map_err(internal_error)?;

    let deleted: Result<Firmware, diesel::result::Error> =
        diesel::delete(firmware.filter(id.eq(path_id)))
            .returning(Firmware::as_returning())
            .get_result(&mut conn)
            .await;

    match deleted {
        Ok(row) => {
            let mut path = api_config.data_storage_location;
            let safe_name = format!("{}.bin", row.file_id);
            path.push("firmware");
            path.push(&safe_name);
            let file_removal = fs::remove_file(path).await;
            match file_removal {
                Err(_) => {
                    warn!(
                        "File {} of firmware {} could not be removed",
                        safe_name, row.id
                    )
                }
                _ => (),
            }
            Ok(Json(row))
        }
        Err(diesel::result::Error::NotFound) => Err((
            axum::http::StatusCode::NOT_FOUND,
            format!("firmware {} not found", path_id),
        )),
        Err(e) => Err(internal_error(e)),
    }
}

#[axum::debug_handler]
pub async fn get_firmware_file_metadata(
    State(api_config): State<RestApiConfig>,
    Path(path_id): Path<i32>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    use crate::db::schema::firmware::dsl::*;

    let mut conn = api_config
        .shared_pool
        .clone()
        .get_owned()
        .await
        .map_err(internal_error)?;
    let fw = firmware
        .select(Firmware::as_select())
        .filter(id.eq(path_id))
        .first(&mut conn)
        .await
        .map_err(internal_error)?;

    // Prepare headers
    let mut headers = HeaderMap::new();
    headers.insert(
        "Content-Type",
        HeaderValue::from_static("application/octet-stream"),
    );
    // Suggest a filename (customize as needed)
    let filename = format!("{}-{}-{}.bin", fw.name, fw.version, fw.id);
    headers.insert(
        "Content-Disposition",
        HeaderValue::from_str(&format!("attachment; filename=\"{}\"", filename)).unwrap(),
    );
    headers.insert(
        "ETag",
        HeaderValue::from_str(&format!("\"{}\"", fw.sha256)).unwrap(),
    );
    headers.insert("Accept-Ranges", HeaderValue::from_static("bytes"));
    headers.insert(
        "Content-Length",
        HeaderValue::from_str(&fw.size.to_string()).unwrap(),
    );

    Ok((headers, Body::empty()))
}

#[axum::debug_handler]
pub async fn get_firmware_file(
    State(api_config): State<RestApiConfig>,
    Path(path_id): Path<i32>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    use crate::db::schema::firmware::dsl::*;

    let mut conn = api_config
        .shared_pool
        .clone()
        .get_owned()
        .await
        .map_err(internal_error)?;
    let fw = firmware
        .select(Firmware::as_select())
        .filter(id.eq(path_id))
        .first(&mut conn)
        .await
        .map_err(internal_error)?;

    let mut path = api_config.data_storage_location;
    let safe_name = format!("{}.bin", fw.file_id);
    path.push("firmware");
    path.push(&safe_name);

    // Open file
    let file = fs::File::open(&path).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("cannot open file: {}", e),
        )
    })?;

    // Stream the file to the client
    let stream = ReaderStream::new(file);
    let body = Body::from_stream(stream);

    // Prepare headers
    let mut headers = HeaderMap::new();
    headers.insert(
        "Content-Type",
        HeaderValue::from_static("application/octet-stream"),
    );
    // Suggest a filename (customize as needed)
    let filename = format!("{}-{}-{}.bin", fw.name, fw.version, fw.id);
    headers.insert(
        "Content-Disposition",
        HeaderValue::from_str(&format!("attachment; filename=\"{}\"", filename)).unwrap(),
    );
    headers.insert(
        "ETag",
        HeaderValue::from_str(&format!("\"{}\"", fw.sha256)).unwrap(),
    );
    headers.insert("Accept-Ranges", HeaderValue::from_static("bytes"));
    headers.insert(
        "Content-Length",
        HeaderValue::from_str(&fw.size.to_string()).unwrap(),
    );

    Ok((headers, body))
}

#[axum::debug_handler]
pub async fn create_device_type_firmware(
    State(api_config): State<RestApiConfig>,
    Json(payload): Json<NewDeviceTypeFirmware>,
) -> impl IntoResponse {
    use crate::db::schema::device_type_firmware::dsl::*;
    let mut conn = match api_config.shared_pool.get().await {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                format!("db pool error: {e}"),
            )
                .into_response();
        }
    };

    let result: Result<DeviceTypeFirmware, diesel::result::Error> =
        diesel::insert_into(device_type_firmware)
            .values(&payload)
            .returning(DeviceTypeFirmware::as_returning())
            .get_result(&mut conn)
            .await;
    match result {
        Ok(created) => (
            StatusCode::CREATED,
            [(
                axum::http::header::LOCATION,
                HeaderValue::from_str(&format!("/device-type-firmware/{}", created.id)).unwrap(),
            )],
            Json(created),
        )
            .into_response(),
        Err(diesel::result::Error::DatabaseError(kind, info)) => {
            // Handle uniqueness violation nicely (if you have a unique index on name)
            if kind == DatabaseErrorKind::UniqueViolation {
                (
                    StatusCode::CONFLICT,
                    format!("device type firmware already exists"),
                )
                    .into_response()
            } else if kind == DatabaseErrorKind::ForeignKeyViolation {
                (
                    StatusCode::BAD_REQUEST,
                    "invalid device_type_id or firmware_id",
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
async fn get_device_type_firmwares(
    State(api_config): State<RestApiConfig>,
) -> Result<Json<Vec<DeviceTypeFirmware>>, (axum::http::StatusCode, String)> {
    use crate::db::schema::device_type_firmware::dsl::*;
    debug!("get_device_type_firmwares called");

    let mut conn = api_config
        .shared_pool
        .clone()
        .get_owned()
        .await
        .map_err(internal_error)?;
    let result = device_type_firmware
        .select(DeviceTypeFirmware::as_select())
        .load(&mut conn)
        .await
        .map_err(internal_error)?;

    Ok(Json(result))
}

#[axum::debug_handler]
async fn get_device_type_firmware(
    State(api_config): State<RestApiConfig>,
    Path(path_id): Path<i32>,
) -> Result<Json<DeviceTypeFirmware>, (axum::http::StatusCode, String)> {
    use crate::db::schema::device_type_firmware::dsl::*;
    debug!("get_device_type_firmware called");

    let mut conn = api_config
        .shared_pool
        .clone()
        .get_owned()
        .await
        .map_err(internal_error)?;
    let result = device_type_firmware
        .select(DeviceTypeFirmware::as_select())
        .filter(id.eq(path_id))
        .first(&mut conn)
        .await
        .map_err(internal_error)?;

    Ok(Json(result))
}

#[axum::debug_handler]
pub async fn delete_device_type_firmware(
    State(api_config): State<RestApiConfig>,
    Path(path_id): Path<i32>,
) -> Result<Json<DeviceTypeFirmware>, (axum::http::StatusCode, String)> {
    use crate::db::schema::device_type_firmware::dsl::*;
    debug!("delete_device_type called: id={}", path_id);

    let mut conn = api_config
        .shared_pool
        .clone()
        .get_owned()
        .await
        .map_err(internal_error)?;

    let deleted: Result<DeviceTypeFirmware, diesel::result::Error> =
        diesel::delete(device_type_firmware.filter(id.eq(path_id)))
            .returning(DeviceTypeFirmware::as_returning())
            .get_result(&mut conn)
            .await;

    match deleted {
        Ok(row) => Ok(Json(row)),
        Err(diesel::result::Error::NotFound) => Err((
            axum::http::StatusCode::NOT_FOUND,
            format!("device_type_firmware {} not found", path_id),
        )),
        Err(e) => Err(internal_error(e)),
    }
}

fn internal_error<E>(err: E) -> (StatusCode, String)
where
    E: std::error::Error,
{
    (StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
}
