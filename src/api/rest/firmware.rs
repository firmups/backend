use crate::api::rest::{self, ApiError, client_error, internal_error};
use crate::db::models::{Firmware, NewFirmware};
use axum::Json;
use axum::body::Body;
use axum::extract::Multipart;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::IntoResponse;
use diesel::ExpressionMethods;
use diesel::SelectableHelper;
use diesel::query_dsl::methods::{FilterDsl, SelectDsl};
use diesel::result::DatabaseErrorKind;
use diesel_async::RunQueryDsl;
use log::{debug, warn};
use sha2::{Digest, Sha256};
use tokio::fs;
use tokio_util::io::ReaderStream;
use uuid::Uuid;

#[axum::debug_handler]
pub async fn list_firmwares(
    State(api_config): State<rest::RestApiConfig>,
) -> Result<Json<Vec<Firmware>>, ApiError> {
    use crate::db::schema::firmware::dsl::*;

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
    State(api_config): State<rest::RestApiConfig>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<Firmware>), rest::ApiError> {
    use crate::db::schema::firmware::dsl::*;

    let mut in_name: Option<String> = None;
    let mut in_version: Option<String> = None;
    let mut in_file_bytes: Option<Vec<u8>> = None;

    while let Some(field) = multipart.next_field().await.unwrap_or(None) {
        let field_name = field.name().unwrap_or("").to_string();
        match field_name.as_str() {
            "name" => {
                in_name = match field.text().await {
                    Ok(opt) => Some(opt),
                    Err(_) => None,
                };
            }
            "version" => {
                in_version = match field.text().await {
                    Ok(opt) => Some(opt),
                    Err(_) => None,
                };
            }
            "file" => {
                in_file_bytes = match field.bytes().await {
                    Ok(opt) => Some(opt.to_vec()),
                    Err(_) => None,
                };
            }
            _ => {}
        }
    }

    // Basic validation

    let in_name = match in_name {
        Some(v) if !v.is_empty() => v,
        _ => {
            return Err(client_error(
                StatusCode::BAD_REQUEST,
                "name cannot be empty".to_string(),
            ));
        }
    };

    if in_name.len() > 100 {
        return Err(client_error(
            StatusCode::BAD_REQUEST,
            "name too long (max 100)".to_string(),
        ));
    }

    let in_version = match in_version {
        Some(v) if !v.is_empty() => v,
        _ => {
            return Err(client_error(
                StatusCode::BAD_REQUEST,
                "version cannot be empty".to_string(),
            ));
        }
    };

    if in_version.len() > 100 {
        return Err(client_error(
            StatusCode::BAD_REQUEST,
            "version too long (max 100)".to_string(),
        ));
    }

    let file = match in_file_bytes {
        Some(f) if !f.is_empty() => f,
        _ => {
            return Err(client_error(
                StatusCode::BAD_REQUEST,
                "firmware file required".to_string(),
            ));
        }
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
        .map_err(rest::internal_error)?;
    path.push(&safe_name);
    fs::write(&path, &file)
        .await
        .map_err(rest::internal_error)?;

    let mut conn = match api_config.shared_pool.get().await {
        Ok(c) => c,
        Err(e) => {
            let _ = fs::remove_file(&path).await;
            return Err(internal_error(e));
        }
    };

    let inserted: Result<Firmware, diesel::result::Error> = diesel::insert_into(firmware)
        .values(&new_firmware)
        .returning(Firmware::as_returning())
        .get_result(&mut conn)
        .await;
    match inserted {
        Ok(record) => return Ok((StatusCode::CREATED, axum::Json(record))),
        Err(diesel::result::Error::DatabaseError(kind, info)) => {
            if kind == DatabaseErrorKind::UniqueViolation {
                return Err(client_error(
                    StatusCode::CONFLICT,
                    format!(
                        "firmware '{}:{}' already exists",
                        new_firmware.name, new_firmware.version
                    ),
                ));
            } else {
                let error = diesel::result::Error::DatabaseError(kind, info);
                return Err(internal_error(error));
            }
        }
        Err(err) => {
            let _ = fs::remove_file(&path).await;
            return Err(internal_error(err));
        }
    }
}

#[axum::debug_handler]
pub async fn get_firmware(
    State(api_config): State<rest::RestApiConfig>,
    Path(path_id): Path<i32>,
) -> Result<Json<Firmware>, rest::ApiError> {
    use crate::db::schema::firmware::dsl::*;

    let mut conn = api_config
        .shared_pool
        .clone()
        .get_owned()
        .await
        .map_err(rest::internal_error)?;
    let result = match firmware
        .select(Firmware::as_select())
        .filter(id.eq(path_id))
        .first(&mut conn)
        .await
    {
        Ok(fw) => fw,
        Err(diesel::result::Error::NotFound) => {
            return Err(rest::client_error(
                StatusCode::NOT_FOUND,
                format!("firmware {} not found", path_id),
            ));
        }
        Err(e) => {
            return Err(rest::internal_error(e));
        }
    };

    Ok(Json(result))
}

#[axum::debug_handler]
pub async fn delete_firmware(
    State(api_config): State<rest::RestApiConfig>,
    Path(path_id): Path<i32>,
) -> Result<Json<Firmware>, ApiError> {
    use crate::db::schema::firmware::dsl::*;

    let mut conn = api_config
        .shared_pool
        .clone()
        .get_owned()
        .await
        .map_err(rest::internal_error)?;

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
        Err(diesel::result::Error::NotFound) => {
            return Err(client_error(
                axum::http::StatusCode::NOT_FOUND,
                format!("firmware {} not found", path_id),
            ));
        }
        Err(e) => Err(internal_error(e)),
    }
}

#[axum::debug_handler]
pub async fn get_firmware_file_metadata(
    State(api_config): State<rest::RestApiConfig>,
    Path(path_id): Path<i32>,
) -> Result<impl IntoResponse, ApiError> {
    use crate::db::schema::firmware::dsl::*;

    let mut conn = api_config
        .shared_pool
        .clone()
        .get_owned()
        .await
        .map_err(rest::internal_error)?;
    let fw = match firmware
        .select(Firmware::as_select())
        .filter(id.eq(path_id))
        .first(&mut conn)
        .await
    {
        Ok(fw) => fw,
        Err(diesel::result::Error::NotFound) => {
            return Err(rest::client_error(
                StatusCode::NOT_FOUND,
                format!("firmware {} not found", path_id),
            ));
        }
        Err(e) => {
            return Err(rest::internal_error(e));
        }
    };

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
    //headers.insert("Accept-Ranges", HeaderValue::from_static("bytes"));
    headers.insert(
        "Content-Length",
        HeaderValue::from_str(&fw.size.to_string()).unwrap(),
    );

    Ok((headers, Body::empty()))
}

#[axum::debug_handler]
pub async fn get_firmware_file(
    State(api_config): State<rest::RestApiConfig>,
    Path(path_id): Path<i32>,
) -> Result<impl IntoResponse, ApiError> {
    use crate::db::schema::firmware::dsl::*;

    let mut conn = api_config
        .shared_pool
        .clone()
        .get_owned()
        .await
        .map_err(rest::internal_error)?;
    let fw = match firmware
        .select(Firmware::as_select())
        .filter(id.eq(path_id))
        .first(&mut conn)
        .await
    {
        Ok(fw) => fw,
        Err(diesel::result::Error::NotFound) => {
            return Err(rest::client_error(
                StatusCode::NOT_FOUND,
                format!("firmware {} not found", path_id),
            ));
        }
        Err(e) => {
            return Err(rest::internal_error(e));
        }
    };

    let mut path = api_config.data_storage_location;
    let safe_name = format!("{}.bin", fw.file_id);
    path.push("firmware");
    path.push(&safe_name);

    // Open file
    let file = fs::File::open(&path).await.map_err(rest::internal_error)?;

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
    //headers.insert("Accept-Ranges", HeaderValue::from_static("bytes"));
    headers.insert(
        "Content-Length",
        HeaderValue::from_str(&fw.size.to_string()).unwrap(),
    );

    Ok((headers, body))
}
