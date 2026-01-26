use crate::api::rest;
use crate::db::models::{Device, NewDevice, UpdateDevice};
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use diesel::ExpressionMethods;
use diesel::SelectableHelper;
use diesel::query_dsl::methods::{FilterDsl, FindDsl, SelectDsl};
use diesel::result::DatabaseErrorKind;
use diesel_async::RunQueryDsl;

#[axum::debug_handler]
pub async fn list_devices(
    State(api_config): State<rest::RestApiConfig>,
) -> Result<Json<Vec<Device>>, rest::error::ApiError> {
    use crate::db::schema::device::dsl::*;

    let mut conn = api_config
        .shared_pool
        .clone()
        .get_owned()
        .await
        .map_err(rest::error::internal_error)?;
    let result = device
        .select(Device::as_select())
        .load(&mut conn)
        .await
        .map_err(rest::error::internal_error)?;

    Ok(Json(result))
}

#[axum::debug_handler]
pub async fn create_device(
    State(api_config): State<rest::RestApiConfig>,
    Json(payload): Json<NewDevice>,
) -> Result<(StatusCode, Json<Device>), rest::error::ApiError> {
    use crate::db::schema::device::dsl as device_dsl;
    // Basic validation
    let name_trimmed = payload.name;
    if name_trimmed.is_empty() {
        return Err(rest::error::client_error(
            StatusCode::BAD_REQUEST,
            "name cannot be empty".to_string(),
        ));
    }
    if name_trimmed.len() > 100 {
        return Err(rest::error::client_error(
            StatusCode::BAD_REQUEST,
            "name too long (max 100)".to_string(),
        ));
    }

    let mut conn = match api_config.shared_pool.get().await {
        Ok(c) => c,
        Err(e) => {
            return Err(rest::error::internal_error(e));
        }
    };

    let new_row = NewDevice {
        name: name_trimmed.to_string(),
        type_: payload.type_,
        firmware: payload.firmware,
        desired_firmware: payload.desired_firmware,
        status: payload.status,
    };

    // Perform the insert and return the created row
    let result: Result<(StatusCode, Json<Device>), rest::error::ApiError> =
        match diesel::insert_into(device_dsl::device)
            .values(&new_row)
            .returning(Device::as_returning())
            .get_result(&mut conn)
            .await
        {
            Ok(device) => Ok((StatusCode::CREATED, Json(device))),
            Err(diesel::result::Error::DatabaseError(
                DatabaseErrorKind::ForeignKeyViolation,
                info,
            )) => {
                // Optional: check which constraint failed for more specific messages.
                match info.constraint_name() {
                    Some("fk_device_type") => Err(rest::error::client_error(
                        StatusCode::BAD_REQUEST,
                        "unknown device type".to_string(),
                    )),
                    Some("fk_firmware") => Err(rest::error::client_error(
                        StatusCode::BAD_REQUEST,
                        "unknown firmware".to_string(),
                    )),
                    Some("fk_desired_firmware") => Err(rest::error::client_error(
                        StatusCode::BAD_REQUEST,
                        "unknown desired firmware".to_string(),
                    )),
                    Some("fk_device_type_current") => Err(rest::error::client_error(
                        StatusCode::BAD_REQUEST,
                        "device type has no link to firmware".to_string(),
                    )),
                    Some("fk_device_type_desired") => Err(rest::error::client_error(
                        StatusCode::BAD_REQUEST,
                        "device type has no link to desired firmware".to_string(),
                    )),
                    _ => {
                        let error = diesel::result::Error::DatabaseError(
                            DatabaseErrorKind::ForeignKeyViolation,
                            info,
                        );
                        Err(rest::error::internal_error(error))
                    }
                }
            }
            // If you also have uniqueness constraints etc., you can match them too:
            Err(diesel::result::Error::DatabaseError(DatabaseErrorKind::UniqueViolation, info)) => {
                // e.g., duplicate device name
                let _detail = info.message(); // or .details()
                Err(rest::error::client_error(
                    StatusCode::CONFLICT,
                    "Device already exists".to_string(),
                ))
            }
            Err(e) => Err(rest::error::internal_error(e)),
        };
    result
}

#[axum::debug_handler]
pub async fn get_device(
    State(api_config): State<rest::RestApiConfig>,
    Path(path_id): Path<i32>,
) -> Result<Json<Device>, rest::error::ApiError> {
    use crate::db::schema::device::dsl::*;

    let mut conn = api_config
        .shared_pool
        .clone()
        .get_owned()
        .await
        .map_err(rest::error::internal_error)?;
    let result = match device
        .select(Device::as_select())
        .filter(id.eq(path_id))
        .first(&mut conn)
        .await
    {
        Ok(fw) => fw,
        Err(diesel::result::Error::NotFound) => {
            return Err(rest::error::client_error(
                StatusCode::NOT_FOUND,
                format!("device {} not found", path_id),
            ));
        }
        Err(e) => {
            return Err(rest::error::internal_error(e));
        }
    };

    Ok(Json(result))
}

#[axum::debug_handler]
pub async fn update_device(
    State(api_config): State<rest::RestApiConfig>,
    Path(path_id): Path<i32>,
    Json(payload): Json<UpdateDevice>,
) -> Result<(StatusCode, Json<Device>), rest::error::ApiError> {
    use crate::db::schema::device::dsl as device_dsl;
    // Basic validation
    if payload.name.is_some() {
        let name_str = payload.name.clone().expect("checked is_some above");
        let name_trimmed = name_str.trim();
        if name_trimmed.is_empty() {
            return Err(rest::error::client_error(
                StatusCode::BAD_REQUEST,
                "name cannot be empty".to_string(),
            ));
        }
        if name_trimmed.len() > 100 {
            return Err(rest::error::client_error(
                StatusCode::BAD_REQUEST,
                "name too long (max 100)".to_string(),
            ));
        }
    }

    let mut conn = match api_config.shared_pool.get().await {
        Ok(c) => c,
        Err(e) => {
            return Err(rest::error::internal_error(e));
        }
    };

    // Perform the insert and return the created row
    let result: Result<(StatusCode, Json<Device>), rest::error::ApiError> =
        match diesel::update(device_dsl::device.find(path_id))
            .set(&payload)
            .returning(Device::as_returning())
            .get_result(&mut conn)
            .await
        {
            Ok(device) => Ok((StatusCode::CREATED, Json(device))),
            Err(diesel::result::Error::DatabaseError(
                DatabaseErrorKind::ForeignKeyViolation,
                info,
            )) => {
                // Optional: check which constraint failed for more specific messages.
                match info.constraint_name() {
                    Some("fk_device_type") => Err(rest::error::client_error(
                        StatusCode::BAD_REQUEST,
                        "unknown device type".to_string(),
                    )),
                    Some("fk_firmware") => Err(rest::error::client_error(
                        StatusCode::BAD_REQUEST,
                        "unknown firmware".to_string(),
                    )),
                    Some("fk_desired_firmware") => Err(rest::error::client_error(
                        StatusCode::BAD_REQUEST,
                        "unknown desired firmware".to_string(),
                    )),
                    Some("fk_device_type_current") => Err(rest::error::client_error(
                        StatusCode::BAD_REQUEST,
                        "device type has no link to firmware".to_string(),
                    )),
                    Some("fk_device_type_desired") => Err(rest::error::client_error(
                        StatusCode::BAD_REQUEST,
                        "device type has no link to desired firmware".to_string(),
                    )),
                    _ => {
                        let error = diesel::result::Error::DatabaseError(
                            DatabaseErrorKind::ForeignKeyViolation,
                            info,
                        );
                        Err(rest::error::internal_error(error))
                    }
                }
            }
            Err(diesel::result::Error::DatabaseError(DatabaseErrorKind::UniqueViolation, info)) => {
                // e.g., duplicate device name
                let _detail = info.message(); // or .details()
                Err(rest::error::client_error(
                    StatusCode::CONFLICT,
                    "Device already exists".to_string(),
                ))
            }
            Err(diesel::result::Error::NotFound) => {
                return Err(rest::error::client_error(
                    axum::http::StatusCode::NOT_FOUND,
                    format!("device {} not found", path_id),
                ));
            }
            Err(e) => Err(rest::error::internal_error(e)),
        };
    result
}

#[axum::debug_handler]
pub async fn delete_device(
    State(api_config): State<rest::RestApiConfig>,
    Path(path_id): Path<i32>,
) -> Result<Json<Device>, rest::error::ApiError> {
    use crate::db::schema::device::dsl::*;

    let mut conn = api_config
        .shared_pool
        .clone()
        .get_owned()
        .await
        .map_err(rest::error::internal_error)?;

    let deleted: Result<Device, diesel::result::Error> =
        diesel::delete(device.filter(id.eq(path_id)))
            .returning(Device::as_returning())
            .get_result(&mut conn)
            .await;

    match deleted {
        Ok(row) => Ok(Json(row)),
        Err(diesel::result::Error::NotFound) => Err(rest::error::client_error(
            axum::http::StatusCode::NOT_FOUND,
            format!("device {} not found", path_id),
        )),
        Err(e) => Err(rest::error::internal_error(e)),
    }
}
