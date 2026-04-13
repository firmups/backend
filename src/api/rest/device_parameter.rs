use crate::api::rest;
use crate::api::rest::serde_helpers::{json_value_to_parameter, parameter_to_json_value};
use crate::db::models::{DeviceParameter, DeviceTypeParameter, NewDeviceParameter, ParameterType};
use crate::db::schema::device::dsl as device_dsl;
use crate::db::schema::device_parameter::dsl as dp_dsl;
use crate::db::schema::device_type_parameter::dsl as dtp_dsl;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use diesel::ExpressionMethods;
use diesel::QueryDsl;
use diesel::SelectableHelper;
use diesel_async::{
    AsyncConnection, AsyncPgConnection, RunQueryDsl, scoped_futures::ScopedFutureExt,
};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

// ---------------------------------------------------------------------------
// Payload types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct DeviceParameterPayload {
    pub device_type_parameter: i32,
    pub key: String,
    #[serde(rename = "type")]
    pub type_: ParameterType,
    pub value: JsonValue,
    pub is_override: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SetDeviceParameterPayload {
    pub value: JsonValue,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn effective_payload(
    dtp: &DeviceTypeParameter,
    override_value: Option<Vec<u8>>,
) -> DeviceParameterPayload {
    let (value, is_override) = match override_value {
        Some(bytes) => (
            parameter_to_json_value(dtp.type_, bytes).unwrap_or(JsonValue::Null),
            true,
        ),
        None => (
            dtp.default_value
                .as_ref()
                .and_then(|b| parameter_to_json_value(dtp.type_, b.clone()).ok())
                .unwrap_or(JsonValue::Null),
            false,
        ),
    };
    DeviceParameterPayload {
        device_type_parameter: dtp.id,
        key: dtp.key.clone(),
        type_: dtp.type_,
        value,
        is_override,
    }
}

async fn get_device_type(
    conn: &mut AsyncPgConnection,
    device_id: i32,
) -> Result<i32, rest::error::ApiError> {
    device_dsl::device
        .filter(device_dsl::id.eq(device_id))
        .select(device_dsl::type_)
        .first::<i32>(conn)
        .await
        .map_err(|e| match e {
            diesel::result::Error::NotFound => rest::error::client_error(
                StatusCode::NOT_FOUND,
                format!("device {} not found", device_id),
            ),
            e => rest::error::internal_error(e),
        })
}

async fn get_device_type_locked(
    conn: &mut AsyncPgConnection,
    device_id: i32,
) -> Result<i32, rest::error::ApiError> {
    device_dsl::device
        .filter(device_dsl::id.eq(device_id))
        .select(device_dsl::type_)
        .for_share()
        .first::<i32>(conn)
        .await
        .map_err(|e| match e {
            diesel::result::Error::NotFound => rest::error::client_error(
                StatusCode::NOT_FOUND,
                format!("device {} not found", device_id),
            ),
            e => rest::error::internal_error(e),
        })
}

async fn get_device_type_parameter(
    conn: &mut AsyncPgConnection,
    device_type_id: i32,
    dtp_id: i32,
) -> Result<DeviceTypeParameter, rest::error::ApiError> {
    dtp_dsl::device_type_parameter
        .filter(dtp_dsl::id.eq(dtp_id))
        .filter(dtp_dsl::device_type.eq(device_type_id))
        .select(DeviceTypeParameter::as_select())
        .first(conn)
        .await
        .map_err(|e| match e {
            diesel::result::Error::NotFound => rest::error::client_error(
                StatusCode::NOT_FOUND,
                format!(
                    "parameter {} not found for device type {}",
                    dtp_id, device_type_id
                ),
            ),
            e => rest::error::internal_error(e),
        })
}

async fn get_device_type_parameter_locked(
    conn: &mut AsyncPgConnection,
    device_type_id: i32,
    dtp_id: i32,
) -> Result<DeviceTypeParameter, rest::error::ApiError> {
    dtp_dsl::device_type_parameter
        .filter(dtp_dsl::id.eq(dtp_id))
        .filter(dtp_dsl::device_type.eq(device_type_id))
        .select(DeviceTypeParameter::as_select())
        .for_share()
        .first(conn)
        .await
        .map_err(|e| match e {
            diesel::result::Error::NotFound => rest::error::client_error(
                StatusCode::NOT_FOUND,
                format!(
                    "parameter {} not found for device type {}",
                    dtp_id, device_type_id
                ),
            ),
            e => rest::error::internal_error(e),
        })
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

#[axum::debug_handler]
pub async fn list_device_parameters(
    State(api_config): State<rest::RestApiConfig>,
    Path(device_id): Path<i32>,
) -> Result<Json<Vec<DeviceParameterPayload>>, rest::error::ApiError> {
    let mut conn = api_config
        .shared_pool
        .clone()
        .get_owned()
        .await
        .map_err(rest::error::internal_error)?;

    let result = conn
        .transaction::<_, rest::error::TransactionError, _>(|conn| {
            async move {
                let device_type_id = get_device_type(conn, device_id).await?;

                let dtps: Vec<DeviceTypeParameter> = dtp_dsl::device_type_parameter
                    .filter(dtp_dsl::device_type.eq(device_type_id))
                    .select(DeviceTypeParameter::as_select())
                    .load(conn)
                    .await?;

                let overrides: Vec<DeviceParameter> = dp_dsl::device_parameter
                    .filter(dp_dsl::device.eq(device_id))
                    .select(DeviceParameter::as_select())
                    .load(conn)
                    .await?;

                let result = dtps
                    .iter()
                    .map(|dtp| {
                        let override_value = overrides
                            .iter()
                            .find(|dp| dp.device_type_parameter == dtp.id)
                            .and_then(|dp| dp.value.clone());
                        effective_payload(dtp, override_value)
                    })
                    .collect();

                Ok(result)
            }
            .scope_boxed()
        })
        .await
        .map_err(|e| match e {
            rest::error::TransactionError::Api(api_err) => api_err,
            rest::error::TransactionError::Db(db_err) => rest::error::internal_error(db_err),
        })?;

    Ok(Json(result))
}

#[axum::debug_handler]
pub async fn get_device_parameter(
    State(api_config): State<rest::RestApiConfig>,
    Path((device_id, dtp_id)): Path<(i32, i32)>,
) -> Result<Json<DeviceParameterPayload>, rest::error::ApiError> {
    let mut conn = api_config
        .shared_pool
        .clone()
        .get_owned()
        .await
        .map_err(rest::error::internal_error)?;

    let result = conn
        .transaction::<_, rest::error::TransactionError, _>(|conn| {
            async move {
                let device_type_id = get_device_type(conn, device_id).await?;
                let dtp = get_device_type_parameter(conn, device_type_id, dtp_id).await?;

                let override_value: Option<Vec<u8>> = dp_dsl::device_parameter
                    .filter(dp_dsl::device.eq(device_id))
                    .filter(dp_dsl::device_type_parameter.eq(dtp_id))
                    .select(dp_dsl::value)
                    .first::<Option<Vec<u8>>>(conn)
                    .await
                    .unwrap_or(None);

                Ok(effective_payload(&dtp, override_value))
            }
            .scope_boxed()
        })
        .await
        .map_err(|e| match e {
            rest::error::TransactionError::Api(api_err) => api_err,
            rest::error::TransactionError::Db(db_err) => rest::error::internal_error(db_err),
        })?;

    Ok(Json(result))
}

#[axum::debug_handler]
pub async fn set_device_parameter(
    State(api_config): State<rest::RestApiConfig>,
    Path((device_id, dtp_id)): Path<(i32, i32)>,
    Json(payload): Json<SetDeviceParameterPayload>,
) -> Result<Json<DeviceParameterPayload>, rest::error::ApiError> {
    let mut conn = api_config
        .shared_pool
        .clone()
        .get_owned()
        .await
        .map_err(rest::error::internal_error)?;

    let result = conn
        .transaction::<_, rest::error::TransactionError, _>(|conn| {
            async move {
                let device_type_id = get_device_type_locked(conn, device_id).await?;
                let dtp = get_device_type_parameter_locked(conn, device_type_id, dtp_id).await?;

                let new_bytes = json_value_to_parameter(dtp.type_, payload.value)
                    .map_err(|msg| rest::error::client_error(StatusCode::BAD_REQUEST, msg))?;

                // If the new value equals the default, remove the override (revert to default)
                if dtp.default_value.as_deref() == Some(new_bytes.as_slice()) {
                    diesel::delete(
                        dp_dsl::device_parameter
                            .filter(dp_dsl::device.eq(device_id))
                            .filter(dp_dsl::device_type_parameter.eq(dtp_id)),
                    )
                    .execute(conn)
                    .await?;

                    return Ok(effective_payload(&dtp, None));
                }

                // Upsert the override
                diesel::insert_into(dp_dsl::device_parameter)
                    .values(&NewDeviceParameter {
                        device: device_id,
                        device_type_parameter: dtp_id,
                        value: Some(new_bytes.clone()),
                    })
                    .on_conflict((dp_dsl::device, dp_dsl::device_type_parameter))
                    .do_update()
                    .set(dp_dsl::value.eq(Some(new_bytes.clone())))
                    .execute(conn)
                    .await?;

                Ok(effective_payload(&dtp, Some(new_bytes)))
            }
            .scope_boxed()
        })
        .await
        .map_err(|e| match e {
            rest::error::TransactionError::Api(api_err) => api_err,
            rest::error::TransactionError::Db(db_err) => rest::error::internal_error(db_err),
        })?;

    Ok(Json(result))
}

#[axum::debug_handler]
pub async fn reset_device_parameter(
    State(api_config): State<rest::RestApiConfig>,
    Path((device_id, dtp_id)): Path<(i32, i32)>,
) -> Result<Json<DeviceParameterPayload>, rest::error::ApiError> {
    let mut conn = api_config
        .shared_pool
        .clone()
        .get_owned()
        .await
        .map_err(rest::error::internal_error)?;

    let result = conn
        .transaction::<_, rest::error::TransactionError, _>(|conn| {
            async move {
                let device_type_id = get_device_type_locked(conn, device_id).await?;
                let dtp = get_device_type_parameter_locked(conn, device_type_id, dtp_id).await?;

                if dtp.default_value.is_none() {
                    return Err(rest::error::client_error(
                        StatusCode::CONFLICT,
                        format!(
                            "parameter {} has no default value and cannot be reset",
                            dtp_id
                        ),
                    )
                    .into());
                }

                diesel::delete(
                    dp_dsl::device_parameter
                        .filter(dp_dsl::device.eq(device_id))
                        .filter(dp_dsl::device_type_parameter.eq(dtp_id)),
                )
                .execute(conn)
                .await?;

                Ok(effective_payload(&dtp, None))
            }
            .scope_boxed()
        })
        .await
        .map_err(|e| match e {
            rest::error::TransactionError::Api(api_err) => api_err,
            rest::error::TransactionError::Db(db_err) => rest::error::internal_error(db_err),
        })?;

    Ok(Json(result))
}
