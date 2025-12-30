use crate::api::rest;
use crate::db::models::{DeviceTypeFirmware, NewDeviceTypeFirmware};
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use diesel::ExpressionMethods;
use diesel::SelectableHelper;
use diesel::query_dsl::methods::{FilterDsl, SelectDsl};
use diesel::result::DatabaseErrorKind;
use diesel_async::RunQueryDsl;
use log::debug;

#[axum::debug_handler]
pub async fn create_device_type_firmware(
    State(api_config): State<rest::RestApiConfig>,
    Json(payload): Json<NewDeviceTypeFirmware>,
) -> Result<(StatusCode, Json<DeviceTypeFirmware>), rest::error::ApiError> {
    use crate::db::schema::device_type_firmware::dsl::*;
    let mut conn = match api_config.shared_pool.get().await {
        Ok(c) => c,
        Err(e) => {
            return Err(rest::error::internal_error(e));
        }
    };

    let result: Result<DeviceTypeFirmware, diesel::result::Error> =
        diesel::insert_into(device_type_firmware)
            .values(&payload)
            .returning(DeviceTypeFirmware::as_returning())
            .get_result(&mut conn)
            .await;
    match result {
        Ok(created) => return Ok((StatusCode::CREATED, Json(created))),
        Err(diesel::result::Error::DatabaseError(kind, info)) => {
            // Handle uniqueness violation nicely (if you have a unique index on name)
            if kind == DatabaseErrorKind::UniqueViolation {
                return Err(rest::error::client_error(
                    StatusCode::CONFLICT,
                    format!("device type firmware already exists"),
                ));
            } else if kind == DatabaseErrorKind::ForeignKeyViolation {
                return Err(rest::error::client_error(
                    StatusCode::BAD_REQUEST,
                    "invalid device_type_id or firmware_id".to_string(),
                ));
            } else {
                let error = diesel::result::Error::DatabaseError(kind, info);
                return Err(rest::error::internal_error(error));
            }
        }
        Err(e) => return Err(rest::error::internal_error(e)),
    }
}

#[axum::debug_handler]
pub async fn list_device_type_firmwares(
    State(api_config): State<rest::RestApiConfig>,
) -> Result<Json<Vec<DeviceTypeFirmware>>, rest::error::ApiError> {
    use crate::db::schema::device_type_firmware::dsl::*;

    let mut conn = api_config
        .shared_pool
        .clone()
        .get_owned()
        .await
        .map_err(rest::error::internal_error)?;
    let result = device_type_firmware
        .select(DeviceTypeFirmware::as_select())
        .load(&mut conn)
        .await
        .map_err(rest::error::internal_error)?;

    Ok(Json(result))
}

#[axum::debug_handler]
pub async fn get_device_type_firmware(
    State(api_config): State<rest::RestApiConfig>,
    Path(path_id): Path<i32>,
) -> Result<Json<DeviceTypeFirmware>, rest::error::ApiError> {
    use crate::db::schema::device_type_firmware::dsl::*;
    debug!("get_device_type_firmware called");

    let mut conn = api_config
        .shared_pool
        .clone()
        .get_owned()
        .await
        .map_err(rest::error::internal_error)?;
    let result = match device_type_firmware
        .select(DeviceTypeFirmware::as_select())
        .filter(id.eq(path_id))
        .first(&mut conn)
        .await
    {
        Ok(dtf) => dtf,
        Err(diesel::result::Error::NotFound) => {
            return Err(rest::error::client_error(
                StatusCode::NOT_FOUND,
                format!("device type firmware {} not found", path_id),
            ));
        }
        Err(e) => {
            return Err(rest::error::internal_error(e));
        }
    };

    Ok(Json(result))
}

#[axum::debug_handler]
pub async fn delete_device_type_firmware(
    State(api_config): State<rest::RestApiConfig>,
    Path(path_id): Path<i32>,
) -> Result<Json<DeviceTypeFirmware>, rest::error::ApiError> {
    use crate::db::schema::device_type_firmware::dsl::*;
    debug!("delete_device_type called: id={}", path_id);

    let mut conn = api_config
        .shared_pool
        .clone()
        .get_owned()
        .await
        .map_err(rest::error::internal_error)?;

    let deleted: Result<DeviceTypeFirmware, diesel::result::Error> =
        diesel::delete(device_type_firmware.filter(id.eq(path_id)))
            .returning(DeviceTypeFirmware::as_returning())
            .get_result(&mut conn)
            .await;

    match deleted {
        Ok(row) => Ok(Json(row)),
        Err(diesel::result::Error::NotFound) => {
            return Err(rest::error::client_error(
                axum::http::StatusCode::NOT_FOUND,
                format!("device_type_firmware {} not found", path_id),
            ));
        }
        Err(e) => return Err(rest::error::internal_error(e)),
    }
}
