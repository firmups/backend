use crate::api::rest;
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
use log::warn;
use sha2::{Digest, Sha256};
use tokio::fs;
use tokio_util::io::ReaderStream;
use uuid::Uuid;

#[axum::debug_handler]
pub async fn list_firmwares(
    State(api_config): State<rest::RestApiConfig>,
) -> Result<Json<Vec<Firmware>>, rest::error::ApiError> {
    use crate::db::schema::firmware::dsl::*;

    let mut conn = api_config
        .shared_pool
        .clone()
        .get_owned()
        .await
        .map_err(rest::error::internal_error)?;
    let result = firmware
        .select(Firmware::as_select())
        .load(&mut conn)
        .await
        .map_err(rest::error::internal_error)?;

    Ok(Json(result))
}

#[axum::debug_handler]
pub async fn create_firmware(
    State(api_config): State<rest::RestApiConfig>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<Firmware>), rest::error::ApiError> {
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
            return Err(rest::error::client_error(
                StatusCode::BAD_REQUEST,
                "name cannot be empty".to_string(),
            ));
        }
    };

    if in_name.len() > 100 {
        return Err(rest::error::client_error(
            StatusCode::BAD_REQUEST,
            "name too long (max 100)".to_string(),
        ));
    }

    let in_version = match in_version {
        Some(v) if !v.is_empty() => v,
        _ => {
            return Err(rest::error::client_error(
                StatusCode::BAD_REQUEST,
                "version cannot be empty".to_string(),
            ));
        }
    };

    if in_version.len() > 100 {
        return Err(rest::error::client_error(
            StatusCode::BAD_REQUEST,
            "version too long (max 100)".to_string(),
        ));
    }

    let file = match in_file_bytes {
        Some(f) if !f.is_empty() => f,
        _ => {
            return Err(rest::error::client_error(
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
        format!("{:x}", hasher.finalize())
    };

    let new_firmware = NewFirmware {
        name: in_name,
        version: in_version,
        file_id: Uuid::new_v4().to_string(),
        size: in_size,
        sha256: in_sha256,
    };

    let safe_name = format!("{}.bin", new_firmware.file_id);
    let mut path = api_config.data_storage_location;
    path.push("firmware");
    fs::create_dir_all(&path)
        .await
        .map_err(rest::error::internal_error)?;
    path.push(&safe_name);
    fs::write(&path, &file)
        .await
        .map_err(rest::error::internal_error)?;
    let mut conn = match api_config.shared_pool.get().await {
        Ok(c) => c,
        Err(e) => {
            let _ = fs::remove_file(&path).await;
            return Err(rest::error::internal_error(e));
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
                let _ = fs::remove_file(&path).await;
                return Err(rest::error::client_error(
                    StatusCode::CONFLICT,
                    format!(
                        "firmware '{}:{}' already exists",
                        new_firmware.name, new_firmware.version
                    ),
                ));
            } else {
                let _ = fs::remove_file(&path).await;
                let error = diesel::result::Error::DatabaseError(kind, info);
                return Err(rest::error::internal_error(error));
            }
        }
        Err(err) => {
            let _ = fs::remove_file(&path).await;
            return Err(rest::error::internal_error(err));
        }
    }
}

// #[axum::debug_handler]
// pub async fn update_firmware(
//     State(api_config): State<rest::RestApiConfig>,
//     Path(path_id): Path<i32>,
//     mut multipart: Multipart,
// ) -> Result<(StatusCode, Json<Firmware>), rest::error::ApiError> {
//     use crate::db::schema::firmware::dsl::*;

//     let mut in_name: Option<String> = None;
//     let mut in_version: Option<String> = None;
//     let mut in_file_bytes: Option<Vec<u8>> = None;

//     while let Some(field) = multipart.next_field().await.unwrap_or(None) {
//         let field_name = field.name().unwrap_or("").to_string();
//         match field_name.as_str() {
//             "name" => {
//                 in_name = match field.text().await {
//                     Ok(opt) => Some(opt),
//                     Err(_) => None,
//                 };
//             }
//             "version" => {
//                 in_version = match field.text().await {
//                     Ok(opt) => Some(opt),
//                     Err(_) => None,
//                 };
//             }
//             "file" => {
//                 in_file_bytes = match field.bytes().await {
//                     Ok(opt) => Some(opt.to_vec()),
//                     Err(_) => None,
//                 };
//             }
//             _ => {}
//         }
//     }

//     // Basic validation
//     if in_name.is_some() {
//         if in_name.clone().unwrap().len() > 100 {
//             return Err(rest::error::client_error(
//                 StatusCode::BAD_REQUEST,
//                 "name too long (max 100)".to_string(),
//             ));
//         }
//     };

//     if in_version.is_some() {
//         if in_version.clone().unwrap().len() > 100 {
//             return Err(rest::error::client_error(
//                 StatusCode::BAD_REQUEST,
//                 "version too long (max 100)".to_string(),
//             ));
//         }
//     };

//     let mut updated_firmware = UpdateFirmware {
//         name: in_name,
//         version: in_version,
//         file_id: None,
//         size: None,
//         sha256: None,
//     };

//     let mut base_path = api_config.data_storage_location;
//     base_path.push("firmware");
//     let mut new_path: Option<PathBuf> = None;
//     if in_file_bytes.is_some() {
//         let file = in_file_bytes.unwrap();
//         let in_size = file.len() as i64;
//         let in_sha256 = {
//             let mut hasher = Sha256::new();
//             hasher.update(&file);
//             format!("{:x}", hasher.finalize())
//         };

//         updated_firmware.file_id = Some(Uuid::new_v4().to_string());
//         updated_firmware.size = Some(in_size);
//         updated_firmware.sha256 = Some(in_sha256);

//         let safe_name = format!("{}.bin", updated_firmware.file_id.clone().unwrap());
//         fs::create_dir_all(&base_path)
//             .await
//             .map_err(rest::error::internal_error)?;
//         let mut path = base_path.clone();
//         path.push(&safe_name);
//         fs::write(&path, &file)
//             .await
//             .map_err(rest::error::internal_error)?;
//         new_path = Some(path);
//     };

//     let mut conn = match api_config.shared_pool.get().await {
//         Ok(c) => c,
//         Err(e) => {
//             if new_path.is_some() {
//                 let _ = fs::remove_file(&new_path.unwrap()).await;
//             }
//             return Err(internal_error(e));
//         }
//     };

//     let changedset = updated_firmware.clone();
//     let old_base = base_path.clone();
//     let tx_result: Result<(Firmware, Option<PathBuf>), diesel::result::Error> = conn
//         .transaction::<(Firmware, Option<PathBuf>), diesel::result::Error, _>(|mut conn| {
//             Box::pin(async move {
//                 let mut old_path: Option<PathBuf> = None;

//                 if changedset.file_id.is_some() {
//                     let old: Firmware = diesel::QueryDsl::for_update(
//                         firmware
//                             .select(Firmware::as_select())
//                             .filter(id.eq(path_id)),
//                     )
//                     .first(&mut conn)
//                     .await?;

//                     let old_safe_name = format!("{}.bin", old.file_id);
//                     let mut temp_path = old_base;
//                     temp_path.push(&old_safe_name);
//                     old_path = Some(temp_path);
//                 }

//                 let new_firmware: Firmware = diesel::update(firmware.find(path_id))
//                     .set(&changedset)
//                     .returning(Firmware::as_returning())
//                     .get_result(&mut conn)
//                     .await?;

//                 Ok((new_firmware, old_path))
//             })
//         })
//         .await;
//     match tx_result {
//         Ok((new_fw, old_path)) => {
//             if let Some(p) = old_path {
//                 let _ = fs::remove_file(&p).await; // best-effort
//             }
//             return Ok((StatusCode::OK, Json(new_fw)));
//         }
//         Err(diesel::result::Error::NotFound) => {
//             if new_path.is_some() {
//                 let _ = fs::remove_file(&new_path.unwrap()).await;
//             }
//             return Err(rest::error::client_error(
//                 axum::http::StatusCode::NOT_FOUND,
//                 format!("firmware {} not found", path_id),
//             ));
//         }
//         Err(diesel::result::Error::DatabaseError(kind, info)) => {
//             // Handle uniqueness violation nicely (if you have a unique index on name)
//             if kind == DatabaseErrorKind::UniqueViolation {
//                 if new_path.is_some() {
//                     let _ = fs::remove_file(&new_path.unwrap()).await;
//                 }
//                 return Err(client_error(
//                     StatusCode::CONFLICT,
//                     "firmware with this name and version already exists".to_string(),
//                 ));
//             } else {
//                 if new_path.is_some() {
//                     let _ = fs::remove_file(&new_path.unwrap()).await;
//                 }
//                 let error = diesel::result::Error::DatabaseError(kind, info);
//                 return Err(internal_error(error));
//             }
//         }
//         Err(e) => {
//             if new_path.is_some() {
//                 let _ = fs::remove_file(&new_path.unwrap()).await;
//             }
//             return Err(internal_error(e));
//         }
//     };
// }

#[axum::debug_handler]
pub async fn get_firmware(
    State(api_config): State<rest::RestApiConfig>,
    Path(path_id): Path<i32>,
) -> Result<Json<Firmware>, rest::error::ApiError> {
    use crate::db::schema::firmware::dsl::*;

    let mut conn = api_config
        .shared_pool
        .clone()
        .get_owned()
        .await
        .map_err(rest::error::internal_error)?;
    let result = match firmware
        .select(Firmware::as_select())
        .filter(id.eq(path_id))
        .first(&mut conn)
        .await
    {
        Ok(fw) => fw,
        Err(diesel::result::Error::NotFound) => {
            return Err(rest::error::client_error(
                StatusCode::NOT_FOUND,
                format!("firmware {} not found", path_id),
            ));
        }
        Err(e) => {
            return Err(rest::error::internal_error(e));
        }
    };

    Ok(Json(result))
}

#[axum::debug_handler]
pub async fn delete_firmware(
    State(api_config): State<rest::RestApiConfig>,
    Path(path_id): Path<i32>,
) -> Result<Json<Firmware>, rest::error::ApiError> {
    use crate::db::schema::firmware::dsl::*;

    let mut conn = api_config
        .shared_pool
        .clone()
        .get_owned()
        .await
        .map_err(rest::error::internal_error)?;

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
            return Err(rest::error::client_error(
                axum::http::StatusCode::NOT_FOUND,
                format!("firmware {} not found", path_id),
            ));
        }
        Err(e) => Err(rest::error::internal_error(e)),
    }
}

#[axum::debug_handler]
pub async fn get_firmware_file_metadata(
    State(api_config): State<rest::RestApiConfig>,
    Path(path_id): Path<i32>,
) -> Result<impl IntoResponse, rest::error::ApiError> {
    use crate::db::schema::firmware::dsl::*;

    let mut conn = api_config
        .shared_pool
        .clone()
        .get_owned()
        .await
        .map_err(rest::error::internal_error)?;
    let fw = match firmware
        .select(Firmware::as_select())
        .filter(id.eq(path_id))
        .first(&mut conn)
        .await
    {
        Ok(fw) => fw,
        Err(diesel::result::Error::NotFound) => {
            return Err(rest::error::client_error(
                StatusCode::NOT_FOUND,
                format!("firmware {} not found", path_id),
            ));
        }
        Err(e) => {
            return Err(rest::error::internal_error(e));
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
        HeaderValue::from_str(&format!("attachment; filename=\"{}\"", filename)).map_err(|_| {
            rest::error::internal_error(rest::error::client_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to set Content-Disposition header".to_string(),
            ))
        })?,
    );
    headers.insert(
        "ETag",
        HeaderValue::from_str(&format!("\"{}\"", fw.sha256)).map_err(|_| {
            rest::error::internal_error(rest::error::client_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to set ETag header".to_string(),
            ))
        })?,
    );
    //headers.insert("Accept-Ranges", HeaderValue::from_static("bytes"));
    headers.insert(
        "Content-Length",
        HeaderValue::from_str(&fw.size.to_string()).map_err(|_| {
            rest::error::internal_error(rest::error::client_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to set Content-Length header".to_string(),
            ))
        })?,
    );

    Ok((headers, Body::empty()))
}

#[axum::debug_handler]
pub async fn get_firmware_file(
    State(api_config): State<rest::RestApiConfig>,
    Path(path_id): Path<i32>,
) -> Result<impl IntoResponse, rest::error::ApiError> {
    use crate::db::schema::firmware::dsl::*;

    let mut conn = api_config
        .shared_pool
        .clone()
        .get_owned()
        .await
        .map_err(rest::error::internal_error)?;
    let fw = match firmware
        .select(Firmware::as_select())
        .filter(id.eq(path_id))
        .first(&mut conn)
        .await
    {
        Ok(fw) => fw,
        Err(diesel::result::Error::NotFound) => {
            return Err(rest::error::client_error(
                StatusCode::NOT_FOUND,
                format!("firmware {} not found", path_id),
            ));
        }
        Err(e) => {
            return Err(rest::error::internal_error(e));
        }
    };

    let mut path = api_config.data_storage_location;
    let safe_name = format!("{}.bin", fw.file_id);
    path.push("firmware");
    path.push(&safe_name);

    // Open file
    let file = fs::File::open(&path)
        .await
        .map_err(rest::error::internal_error)?;

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
        HeaderValue::from_str(&format!("attachment; filename=\"{}\"", filename)).map_err(|_| {
            rest::error::internal_error(rest::error::client_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to set Content-Disposition header".to_string(),
            ))
        })?,
    );
    headers.insert(
        "ETag",
        HeaderValue::from_str(&format!("\"{}\"", fw.sha256)).map_err(|_| {
            rest::error::internal_error(rest::error::client_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to set ETag header".to_string(),
            ))
        })?,
    );
    //headers.insert("Accept-Ranges", HeaderValue::from_static("bytes"));
    headers.insert(
        "Content-Length",
        HeaderValue::from_str(&fw.size.to_string()).map_err(|_| {
            rest::error::internal_error(rest::error::client_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to set Content-Length header".to_string(),
            ))
        })?,
    );

    Ok((headers, body))
}
