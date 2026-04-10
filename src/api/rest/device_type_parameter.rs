use crate::api::rest;
use crate::db::models::{
    DeviceTypeParameter, NewDeviceTypeParameter, ParameterType, UpdateDeviceTypeParameter,
};
use crate::db::schema::device::dsl as device_dsl;
use crate::db::schema::device_type_parameter::dsl as dtp_dsl;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use diesel::ExpressionMethods;
use diesel::QueryDsl;
use diesel::SelectableHelper;
use diesel::result::DatabaseErrorKind;
use diesel_async::{AsyncConnection, RunQueryDsl};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

// ---------------------------------------------------------------------------
// Value encoding helpers
// ---------------------------------------------------------------------------

fn bytes_to_json_value(type_: ParameterType, bytes: Vec<u8>) -> Result<JsonValue, String> {
    match type_ {
        ParameterType::String => String::from_utf8(bytes)
            .map(JsonValue::String)
            .map_err(|e| format!("stored string value is not valid UTF-8: {e}")),
        ParameterType::Integer => {
            let arr: [u8; 8] = bytes
                .try_into()
                .map_err(|_| "stored integer value must be exactly 8 bytes".to_string())?;
            let n = i64::from_be_bytes(arr);
            Ok(JsonValue::Number(n.into()))
        }
        ParameterType::Float => {
            let arr: [u8; 8] = bytes
                .try_into()
                .map_err(|_| "stored float value must be exactly 8 bytes".to_string())?;
            let f = f64::from_be_bytes(arr);
            serde_json::Number::from_f64(f)
                .map(JsonValue::Number)
                .ok_or_else(|| "stored float value is non-finite".to_string())
        }
        ParameterType::Boolean => {
            if bytes.is_empty() {
                return Err("stored boolean value must be exactly 1 byte".to_string());
            }
            Ok(JsonValue::Bool(bytes[0] != 0))
        }
        ParameterType::Binary => Ok(JsonValue::String(STANDARD.encode(&bytes))),
    }
}

fn json_value_to_bytes(type_: ParameterType, value: JsonValue) -> Result<Vec<u8>, String> {
    match type_ {
        ParameterType::String => match value {
            JsonValue::String(s) => Ok(s.into_bytes()),
            _ => Err("expected a string value for a String parameter".to_string()),
        },
        ParameterType::Integer => match value {
            JsonValue::Number(n) => n
                .as_i64()
                .map(|i| i.to_be_bytes().to_vec())
                .ok_or_else(|| "expected an integer value for an Integer parameter".to_string()),
            _ => Err("expected a number value for an Integer parameter".to_string()),
        },
        ParameterType::Float => match value {
            JsonValue::Number(n) => n
                .as_f64()
                .map(|f| f.to_be_bytes().to_vec())
                .ok_or_else(|| "expected a float value for a Float parameter".to_string()),
            _ => Err("expected a number value for a Float parameter".to_string()),
        },
        ParameterType::Boolean => match value {
            JsonValue::Bool(b) => Ok(vec![if b { 1u8 } else { 0u8 }]),
            _ => Err("expected a boolean value for a Boolean parameter".to_string()),
        },
        ParameterType::Binary => match value {
            JsonValue::String(s) => STANDARD
                .decode(s.as_bytes())
                .map_err(|e| format!("invalid base64 for Binary parameter: {e}")),
            _ => Err("expected a base64 string value for a Binary parameter".to_string()),
        },
    }
}

// ---------------------------------------------------------------------------
// Payload types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewDeviceTypeParameterPayload {
    pub key: String,
    #[serde(rename = "type")]
    pub type_: ParameterType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_value: Option<JsonValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceTypeParameterPayload {
    pub id: i32,
    pub device_type: i32,
    pub key: String,
    #[serde(rename = "type")]
    pub type_: ParameterType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_value: Option<JsonValue>,
}

impl From<DeviceTypeParameter> for DeviceTypeParameterPayload {
    fn from(src: DeviceTypeParameter) -> Self {
        let default_value = src
            .default_value
            .and_then(|bytes| bytes_to_json_value(src.type_, bytes).ok());
        Self {
            id: src.id,
            device_type: src.device_type,
            key: src.key,
            type_: src.type_,
            default_value,
        }
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

#[axum::debug_handler]
pub async fn create_device_type_parameter(
    State(api_config): State<rest::RestApiConfig>,
    Path(device_type_id): Path<i32>,
    Json(payload): Json<NewDeviceTypeParameterPayload>,
) -> Result<(StatusCode, Json<DeviceTypeParameterPayload>), rest::error::ApiError> {
    let mut conn = match api_config.shared_pool.get().await {
        Ok(c) => c,
        Err(e) => return Err(rest::error::internal_error(e)),
    };

    let key_trimmed = payload.key.trim().to_string();
    let key_for_error = key_trimmed.clone();

    let default_bytes: Option<Vec<u8>> = match payload.default_value {
        Some(v) => match json_value_to_bytes(payload.type_, v) {
            Ok(bytes) => Some(bytes),
            Err(msg) => return Err(rest::error::client_error(StatusCode::BAD_REQUEST, msg)),
        },
        None => None,
    };

    let tx_result: Result<DeviceTypeParameterPayload, rest::error::TransactionError> = conn
        .transaction::<_, rest::error::TransactionError, _>(|mut conn| {
            let key_trimmed = key_trimmed;
            let default_bytes = default_bytes;
            Box::pin(async move {
                // Lock device_type to prevent race conditions with device creation
                // Namespace 2 = device_type locks
                diesel::dsl::sql_query("SELECT pg_advisory_xact_lock(2, $1)")
                    .bind::<diesel::sql_types::Integer, _>(device_type_id)
                    .execute(&mut conn)
                    .await?;

                // If no default_value is provided, check that no devices exist with this device_type
                if default_bytes.is_none() {
                    let devices_exist: bool = diesel::select(diesel::dsl::exists(
                        device_dsl::device.filter(device_dsl::type_.eq(device_type_id)),
                    ))
                    .get_result(&mut conn)
                    .await?;

                    if devices_exist {
                        return Err(rest::error::TransactionError::from(
                            rest::error::client_error(
                                StatusCode::CONFLICT,
                                format!(
                                    "cannot add parameter without default value to device type {}: devices already exist",
                                    device_type_id
                                ),
                            ),
                        ));
                    }
                }

                let new_param = NewDeviceTypeParameter {
                    device_type: device_type_id,
                    key: key_trimmed,
                    type_: payload.type_,
                    default_value: default_bytes,
                };

                let created: DeviceTypeParameter =
                    diesel::insert_into(dtp_dsl::device_type_parameter)
                        .values(&new_param)
                        .returning(DeviceTypeParameter::as_returning())
                        .get_result(&mut conn)
                        .await?;

                Ok(created.into())
            })
        })
        .await;

    use diesel::result::Error as DieselError;

    match tx_result {
        Ok(param_payload) => Ok((StatusCode::CREATED, Json(param_payload))),
        Err(rest::error::TransactionError::Db(DieselError::DatabaseError(
            DatabaseErrorKind::UniqueViolation,
            _,
        ))) => Err(rest::error::client_error(
            StatusCode::CONFLICT,
            format!(
                "parameter '{}' already exists for device type {}",
                key_for_error, device_type_id
            ),
        )),
        Err(rest::error::TransactionError::Db(DieselError::DatabaseError(
            DatabaseErrorKind::ForeignKeyViolation,
            _,
        ))) => Err(rest::error::client_error(
            StatusCode::NOT_FOUND,
            format!("device type {} not found", device_type_id),
        )),
        Err(rest::error::TransactionError::Db(e)) => Err(rest::error::internal_error(e)),
        Err(rest::error::TransactionError::Api(api)) => Err(api),
    }
}

#[axum::debug_handler]
pub async fn list_device_type_parameters(
    State(api_config): State<rest::RestApiConfig>,
    Path(device_type_id): Path<i32>,
) -> Result<Json<Vec<DeviceTypeParameterPayload>>, rest::error::ApiError> {
    let mut conn = api_config
        .shared_pool
        .clone()
        .get_owned()
        .await
        .map_err(rest::error::internal_error)?;

    let result: Vec<DeviceTypeParameter> = dtp_dsl::device_type_parameter
        .filter(dtp_dsl::device_type.eq(device_type_id))
        .select(DeviceTypeParameter::as_select())
        .load(&mut conn)
        .await
        .map_err(rest::error::internal_error)?;

    Ok(Json(result.into_iter().map(|p| p.into()).collect()))
}

#[axum::debug_handler]
pub async fn get_device_type_parameter(
    State(api_config): State<rest::RestApiConfig>,
    Path((device_type_id, param_id)): Path<(i32, i32)>,
) -> Result<Json<DeviceTypeParameterPayload>, rest::error::ApiError> {
    let mut conn = api_config
        .shared_pool
        .clone()
        .get_owned()
        .await
        .map_err(rest::error::internal_error)?;

    let result: Result<DeviceTypeParameter, diesel::result::Error> = dtp_dsl::device_type_parameter
        .filter(dtp_dsl::id.eq(param_id))
        .filter(dtp_dsl::device_type.eq(device_type_id))
        .select(DeviceTypeParameter::as_select())
        .first(&mut conn)
        .await;

    match result {
        Ok(param) => Ok(Json(param.into())),
        Err(diesel::result::Error::NotFound) => Err(rest::error::client_error(
            StatusCode::NOT_FOUND,
            format!(
                "parameter {} not found for device type {}",
                param_id, device_type_id
            ),
        )),
        Err(e) => Err(rest::error::internal_error(e)),
    }
}

#[axum::debug_handler]
pub async fn update_device_type_parameter(
    State(api_config): State<rest::RestApiConfig>,
    Path((device_type_id, param_id)): Path<(i32, i32)>,
    Json(payload): Json<UpdateDeviceTypeParameter>,
) -> Result<Json<DeviceTypeParameterPayload>, rest::error::ApiError> {
    let mut conn = api_config
        .shared_pool
        .clone()
        .get_owned()
        .await
        .map_err(rest::error::internal_error)?;

    let update = UpdateDeviceTypeParameter {
        key: payload.key.trim().to_string(),
    };

    let result: Result<DeviceTypeParameter, diesel::result::Error> = diesel::update(
        dtp_dsl::device_type_parameter
            .filter(dtp_dsl::id.eq(param_id))
            .filter(dtp_dsl::device_type.eq(device_type_id)),
    )
    .set(&update)
    .returning(DeviceTypeParameter::as_returning())
    .get_result(&mut conn)
    .await;

    match result {
        Ok(param) => Ok(Json(param.into())),
        Err(diesel::result::Error::NotFound) => Err(rest::error::client_error(
            StatusCode::NOT_FOUND,
            format!(
                "parameter {} not found for device type {}",
                param_id, device_type_id
            ),
        )),
        Err(diesel::result::Error::DatabaseError(DatabaseErrorKind::UniqueViolation, _)) => {
            Err(rest::error::client_error(
                StatusCode::CONFLICT,
                format!(
                    "parameter '{}' already exists for device type {}",
                    update.key, device_type_id
                ),
            ))
        }
        Err(e) => Err(rest::error::internal_error(e)),
    }
}

#[axum::debug_handler]
pub async fn delete_device_type_parameter(
    State(api_config): State<rest::RestApiConfig>,
    Path((device_type_id, param_id)): Path<(i32, i32)>,
) -> Result<Json<DeviceTypeParameterPayload>, rest::error::ApiError> {
    let mut conn = api_config
        .shared_pool
        .clone()
        .get_owned()
        .await
        .map_err(rest::error::internal_error)?;

    let tx_result: Result<DeviceTypeParameterPayload, rest::error::TransactionError> = conn
        .transaction::<_, rest::error::TransactionError, _>(|mut conn| {
            Box::pin(async move {
                // Lock device_type to prevent race conditions with device creation
                // Namespace 2 = device_type locks
                diesel::dsl::sql_query("SELECT pg_advisory_xact_lock(2, $1)")
                    .bind::<diesel::sql_types::Integer, _>(device_type_id)
                    .execute(&mut conn)
                    .await?;

                // Check if any devices exist with this device type
                let devices_exist: bool = diesel::select(diesel::dsl::exists(
                    device_dsl::device.filter(device_dsl::type_.eq(device_type_id)),
                ))
                .get_result(&mut conn)
                .await?;

                if devices_exist {
                    return Err(rest::error::TransactionError::from(
                        rest::error::client_error(
                            StatusCode::CONFLICT,
                            format!(
                                "cannot delete parameter from device type {}: devices already exist",
                                device_type_id
                            ),
                        ),
                    ));
                }

                let deleted: DeviceTypeParameter = diesel::delete(
                    dtp_dsl::device_type_parameter
                        .filter(dtp_dsl::id.eq(param_id))
                        .filter(dtp_dsl::device_type.eq(device_type_id)),
                )
                .returning(DeviceTypeParameter::as_returning())
                .get_result(&mut conn)
                .await?;

                Ok(deleted.into())
            })
        })
        .await;

    match tx_result {
        Ok(param_payload) => Ok(Json(param_payload)),
        Err(rest::error::TransactionError::Db(diesel::result::Error::NotFound)) => {
            Err(rest::error::client_error(
                StatusCode::NOT_FOUND,
                format!(
                    "parameter {} not found for device type {}",
                    param_id, device_type_id
                ),
            ))
        }
        Err(rest::error::TransactionError::Db(e)) => Err(rest::error::internal_error(e)),
        Err(rest::error::TransactionError::Api(api)) => Err(api),
    }
}
