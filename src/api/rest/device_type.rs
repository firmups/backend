use crate::api::rest;
use crate::db::models::{DeviceType, NewDeviceType, UpdateDeviceType};
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use diesel::ExpressionMethods;
use diesel::SelectableHelper;
use diesel::query_dsl::methods::{FilterDsl, FindDsl, SelectDsl};
use diesel::result::DatabaseErrorKind;
use diesel_async::RunQueryDsl;

#[axum::debug_handler]
pub async fn create_device_type(
    State(api_config): State<rest::RestApiConfig>,
    Json(payload): Json<NewDeviceType>,
) -> Result<(StatusCode, Json<DeviceType>), rest::error::ApiError> {
    use crate::db::schema::device_type::dsl::*;
    // Basic validation
    let name_trimmed = payload.name.trim();
    if name_trimmed.len() > 100 {
        return Err(rest::error::client_error(
            StatusCode::BAD_REQUEST,
            "name too long (max 100)".to_string(),
        ));
    }

    // Insert
    let mut conn = match api_config.shared_pool.get().await {
        Ok(c) => c,
        Err(e) => {
            return Err(rest::error::internal_error(e));
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
        Ok(created) => return Ok((StatusCode::CREATED, Json(created))),
        Err(diesel::result::Error::DatabaseError(kind, info)) => {
            // Handle uniqueness violation nicely (if you have a unique index on name)
            if kind == DatabaseErrorKind::UniqueViolation {
                return Err(rest::error::client_error(
                    StatusCode::CONFLICT,
                    format!("device type '{}' already exists", name_trimmed),
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
pub async fn list_device_types(
    State(api_config): State<rest::RestApiConfig>,
) -> Result<Json<Vec<DeviceType>>, rest::error::ApiError> {
    use crate::db::schema::device_type::dsl::*;

    let mut conn = api_config
        .shared_pool
        .clone()
        .get_owned()
        .await
        .map_err(rest::error::internal_error)?;
    let result = device_type
        .select(DeviceType::as_select())
        .load(&mut conn)
        .await
        .map_err(rest::error::internal_error)?;

    Ok(Json(result))
}

#[axum::debug_handler]
pub async fn get_device_type(
    State(api_config): State<rest::RestApiConfig>,
    Path(path_id): Path<i32>,
) -> Result<Json<DeviceType>, rest::error::ApiError> {
    use crate::db::schema::device_type::dsl::*;

    let mut conn = api_config
        .shared_pool
        .clone()
        .get_owned()
        .await
        .map_err(rest::error::internal_error)?;
    let result = match device_type
        .select(DeviceType::as_select())
        .filter(id.eq(path_id))
        .first(&mut conn)
        .await
    {
        Ok(dt) => dt,
        Err(diesel::result::Error::NotFound) => {
            return Err(rest::error::client_error(
                StatusCode::NOT_FOUND,
                format!("device type {} not found", path_id),
            ));
        }
        Err(e) => {
            return Err(rest::error::internal_error(e));
        }
    };

    Ok(Json(result))
}

#[axum::debug_handler]
pub async fn update_device_type(
    State(api_config): State<rest::RestApiConfig>,
    Path(path_id): Path<i32>,
    Json(payload): Json<UpdateDeviceType>,
) -> Result<(StatusCode, Json<DeviceType>), rest::error::ApiError> {
    use crate::db::schema::device_type::dsl::*;
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

    // Insert
    let mut conn = match api_config.shared_pool.get().await {
        Ok(c) => c,
        Err(e) => {
            return Err(rest::error::internal_error(e));
        }
    };

    let result: Result<DeviceType, diesel::result::Error> =
        diesel::update(device_type.find(path_id))
            .set(&payload)
            .returning(DeviceType::as_returning())
            .get_result(&mut conn)
            .await;
    match result {
        Ok(created) => return Ok((StatusCode::CREATED, Json(created))),
        Err(diesel::result::Error::NotFound) => {
            return Err(rest::error::client_error(
                axum::http::StatusCode::NOT_FOUND,
                format!("device type {} not found", path_id),
            ));
        }
        Err(diesel::result::Error::DatabaseError(kind, info)) => {
            // Handle uniqueness violation nicely (if you have a unique index on name)
            if kind == DatabaseErrorKind::UniqueViolation {
                return Err(rest::error::client_error(
                    StatusCode::CONFLICT,
                    format!("device type with this name already exists"),
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
pub async fn delete_device_type(
    State(api_config): State<rest::RestApiConfig>,
    Path(path_id): Path<i32>,
) -> Result<Json<DeviceType>, rest::error::ApiError> {
    use crate::db::schema::device_type::dsl::*;

    let mut conn = api_config
        .shared_pool
        .clone()
        .get_owned()
        .await
        .map_err(rest::error::internal_error)?;

    let deleted: Result<DeviceType, diesel::result::Error> =
        diesel::delete(device_type.filter(id.eq(path_id)))
            .returning(DeviceType::as_returning())
            .get_result(&mut conn)
            .await;

    match deleted {
        Ok(row) => Ok(Json(row)),
        Err(diesel::result::Error::NotFound) => Err(rest::error::client_error(
            axum::http::StatusCode::NOT_FOUND,
            format!("device type {} not found", path_id),
        )),
        Err(e) => Err(rest::error::internal_error(e)),
    }
}
