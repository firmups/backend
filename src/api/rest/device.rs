use crate::api::rest;
use crate::db::models::{Device, DeviceStatus, NewDevice, UpdateDevice};
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use diesel::ExpressionMethods;
use diesel::QueryDsl;
use diesel::SelectableHelper;
use diesel::result::DatabaseErrorKind;
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};

/// Check if a device type has any parameters without default values.
/// Returns the keys of parameters that have no default.
async fn get_params_without_defaults(
    conn: &mut AsyncPgConnection,
    device_type_id: i32,
) -> Result<Vec<String>, diesel::result::Error> {
    use crate::db::schema::device_type_parameter::dsl::*;

    device_type_parameter
        .filter(device_type.eq(device_type_id))
        .filter(default_value.is_null())
        .select(key)
        .load::<String>(conn)
        .await
}

/// Check if a device has overrides for all parameters without defaults.
/// Returns the keys of parameters that are missing overrides.
async fn get_missing_param_overrides(
    conn: &mut AsyncPgConnection,
    device_id: i32,
    device_type_id: i32,
) -> Result<Vec<String>, diesel::result::Error> {
    use crate::db::schema::device_parameter::dsl as dp_dsl;
    use crate::db::schema::device_type_parameter::dsl as dtp_dsl;

    let params_without_defaults: Vec<(i32, String)> = dtp_dsl::device_type_parameter
        .filter(dtp_dsl::device_type.eq(device_type_id))
        .filter(dtp_dsl::default_value.is_null())
        .select((dtp_dsl::id, dtp_dsl::key))
        .load(conn)
        .await?;

    if params_without_defaults.is_empty() {
        return Ok(vec![]);
    }

    let overridden_param_ids: Vec<i32> = dp_dsl::device_parameter
        .filter(dp_dsl::device.eq(device_id))
        .select(dp_dsl::device_type_parameter)
        .load(conn)
        .await?;

    let missing: Vec<String> = params_without_defaults
        .into_iter()
        .filter(|(param_id, _)| !overridden_param_ids.contains(param_id))
        .map(|(_, param_key)| param_key)
        .collect();

    Ok(missing)
}

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
        gateway_id: payload.gateway_id,
    };

    let device_type_id = payload.type_;

    let tx_result: Result<Device, rest::error::TransactionError> = conn
        .transaction::<_, rest::error::TransactionError, _>(async move |conn| {
            // Lock device_type to prevent race conditions with parameter creation
            // Namespace 2 = device_type locks
            diesel::dsl::sql_query("SELECT pg_advisory_xact_lock(2, $1)")
                .bind::<diesel::sql_types::Integer, _>(device_type_id)
                .execute(conn)
                .await?;

            // If status is Active, check that there are no parameters without defaults
            // (since a new device cannot have any overrides yet)
            if new_row.status == DeviceStatus::Active {
                let params_without_defaults =
                    get_params_without_defaults(conn, device_type_id).await?;
                if !params_without_defaults.is_empty() {
                    return Err(rest::error::client_error(
                        StatusCode::BAD_REQUEST,
                        format!(
                            "cannot set device to active: missing values for parameters without defaults: {}",
                            params_without_defaults.join(", ")
                        ),
                    )
                    .into());
                }
            }

            let device: Device = diesel::insert_into(device_dsl::device)
                .values(&new_row)
                .returning(Device::as_returning())
                .get_result(conn)
                .await?;

            Ok(device)
        })
        .await;

    use diesel::result::Error as DieselError;

    match tx_result {
        Ok(device) => Ok((StatusCode::CREATED, Json(device))),
        Err(rest::error::TransactionError::Db(DieselError::DatabaseError(
            DatabaseErrorKind::ForeignKeyViolation,
            info,
        ))) => match info.constraint_name() {
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
            Some("fk_gateway") => Err(rest::error::client_error(
                StatusCode::BAD_REQUEST,
                "unknown gateway device".to_string(),
            )),
            _ => {
                let error =
                    DieselError::DatabaseError(DatabaseErrorKind::ForeignKeyViolation, info);
                Err(rest::error::internal_error(error))
            }
        },
        Err(rest::error::TransactionError::Db(DieselError::DatabaseError(
            DatabaseErrorKind::UniqueViolation,
            _,
        ))) => Err(rest::error::client_error(
            StatusCode::CONFLICT,
            "Device already exists".to_string(),
        )),
        Err(rest::error::TransactionError::Db(e)) => Err(rest::error::internal_error(e)),
        Err(rest::error::TransactionError::Api(api)) => Err(api),
    }
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

    let tx_result: Result<Device, rest::error::TransactionError> = conn
        .transaction::<_, rest::error::TransactionError, _>(async move |conn| {
            if payload.status == Some(DeviceStatus::Active) {
                let current_device: Device = device_dsl::device
                    .find(path_id)
                    .select(Device::as_select())
                    .for_share()
                    .first(conn)
                    .await?;

                if current_device.status != DeviceStatus::Active {
                    let missing =
                        get_missing_param_overrides(conn, path_id, current_device.type_).await?;

                    if !missing.is_empty() {
                        return Err(rest::error::client_error(
                            StatusCode::BAD_REQUEST,
                            format!(
                                "cannot set device to active: missing values for parameters: {}",
                                missing.join(", ")
                            ),
                        )
                        .into());
                    }
                }
            }

            let device: Device = diesel::update(device_dsl::device.find(path_id))
                .set(&payload)
                .returning(Device::as_returning())
                .get_result(conn)
                .await?;

            Ok(device)
        })
        .await;

    use diesel::result::Error as DieselError;

    match tx_result {
        Ok(device) => Ok((StatusCode::OK, Json(device))),
        Err(rest::error::TransactionError::Db(DieselError::DatabaseError(
            DatabaseErrorKind::ForeignKeyViolation,
            info,
        ))) => match info.constraint_name() {
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
            Some("fk_gateway") => Err(rest::error::client_error(
                StatusCode::BAD_REQUEST,
                "unknown gateway device".to_string(),
            )),
            _ => {
                let error =
                    DieselError::DatabaseError(DatabaseErrorKind::ForeignKeyViolation, info);
                Err(rest::error::internal_error(error))
            }
        },
        Err(rest::error::TransactionError::Db(DieselError::DatabaseError(
            DatabaseErrorKind::UniqueViolation,
            _,
        ))) => Err(rest::error::client_error(
            StatusCode::CONFLICT,
            "Device already exists".to_string(),
        )),
        Err(rest::error::TransactionError::Db(DieselError::NotFound)) => {
            Err(rest::error::client_error(
                StatusCode::NOT_FOUND,
                format!("device {} not found", path_id),
            ))
        }
        Err(rest::error::TransactionError::Db(e)) => Err(rest::error::internal_error(e)),
        Err(rest::error::TransactionError::Api(api_err)) => Err(api_err),
    }
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
