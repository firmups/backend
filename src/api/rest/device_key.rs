use crate::api::rest;
use crate::db::models::{
    CryptoAlgorithm, DeviceKey, KeyStatus, KeyType, LightweightKeyDetails,
    NewLightweightKeyDetails, TlsKeyDetails,
};
use crate::db::schema::device_key::dsl as key_dsl;
use crate::db::schema::lightweight_key_details::dsl as lw_dsl;
use crate::db::schema::tls_key_details::dsl as tls_dsl;
use crate::db::schema::{device_key as dk, lightweight_key_details as lw, tls_key_details as tls};
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use diesel::BoolExpressionMethods;
use diesel::ExpressionMethods;
use diesel::JoinOnDsl;
use diesel::NullableExpressionMethods;
use diesel::QueryDsl;
use diesel::SelectableHelper;
use diesel_async::{AsyncConnection, RunQueryDsl};
use log::info;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewDeviceKeyPayload {
    #[serde(flatten)]
    pub kind: NewDeviceKeyKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceKeyPayload {
    pub id: i32,
    pub status: KeyStatus,
    #[serde(flatten)]
    pub kind: DeviceKeyKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "key_type")]
pub enum NewDeviceKeyKind {
    #[serde(rename = "LIGHTWEIGHT")]
    Lightweight {
        details: LightweightKeyDetailsPayload,
    },
    #[serde(rename = "TLS")]
    Tls { details: TlsKeyDetailsPayload },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LightweightKeyDetailsPayload {
    pub algorithm: CryptoAlgorithm,
    #[serde(
        serialize_with = "rest::serde_helpers::as_base64",
        deserialize_with = "rest::serde_helpers::from_base64"
    )]
    pub key: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TlsKeyDetailsPayload {
    pub valid_from: chrono::NaiveDateTime,
    pub valid_to: chrono::NaiveDateTime,
}

impl From<LightweightKeyDetails> for LightweightKeyDetailsPayload {
    fn from(src: LightweightKeyDetails) -> Self {
        let LightweightKeyDetails { algorithm, key, .. } = src;
        Self { algorithm, key }
    }
}

impl From<TlsKeyDetails> for TlsKeyDetailsPayload {
    fn from(src: TlsKeyDetails) -> Self {
        let TlsKeyDetails {
            valid_from,
            valid_to,
            ..
        } = src;
        Self {
            valid_from,
            valid_to,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "key_type")]
pub enum DeviceKeyKind {
    #[serde(rename = "LIGHTWEIGHT")]
    Lightweight {
        details: LightweightKeyDetailsPayload,
    },
    #[serde(rename = "TLS")]
    Tls { details: TlsKeyDetailsPayload },
}

#[axum::debug_handler]
pub async fn create_device_key(
    State(api_config): State<rest::RestApiConfig>,
    Path(device_id): Path<i32>,
    Json(payload): Json<NewDeviceKeyPayload>,
) -> Result<(StatusCode, Json<DeviceKeyPayload>), rest::error::ApiError> {
    // Insert
    let mut conn = match api_config.shared_pool.get().await {
        Ok(c) => c,
        Err(e) => {
            return Err(rest::error::internal_error(e));
        }
    };

    let tx_result: Result<DeviceKeyPayload, rest::error::TransactionError> = conn
        .transaction::<_, rest::error::TransactionError, _>(|mut conn| {
            Box::pin(async move {
                let kind: DeviceKeyKind;
                let key_type: KeyType;
                let mut key_status: KeyStatus = KeyStatus::Next;

                // Lock device to prevent multiple keys being created simultaneously
                diesel::dsl::sql_query("SELECT pg_advisory_xact_lock($1)")
                            .bind::<diesel::sql_types::BigInt, _>(device_id as i64)
                            .execute(&mut conn)
                            .await?;

                match payload.kind.clone() {
                    NewDeviceKeyKind::Lightweight { details: det } => {
                        key_type = KeyType::Lightweight;
                        match det.algorithm {
                            CryptoAlgorithm::AsconAead128  => {
                                if det.key.len() != 16 /* ToDo: Replace magic number */ {
                                    return Err(rest::error::TransactionError::from(
                                        rest::error::client_error(
                                            StatusCode::BAD_REQUEST,
                                            format!(
                                                "Invalid key length {} for ascon aead128 should be 16",
                                                det.key.len()
                                            ),
                                        ),
                                    ));
                                };
                            }
                            CryptoAlgorithm::AesGcm128 => {
                                if det.key.len() != 12 /* ToDo: Replace magic number */ {
                                    return Err(rest::error::TransactionError::from(
                                        rest::error::client_error(
                                            StatusCode::BAD_REQUEST,
                                            format!(
                                                "Invalid key length {} for aes gcm128 should be 12",
                                                det.key.len()
                                            ),
                                        ),
                                    ));
                                };
                            }
                        }
                    }
                    NewDeviceKeyKind::Tls { details: _ } => {
                        key_type = KeyType::Tls;
                    }
                }

                let next_filter = key_dsl::device_key
                    .filter(key_dsl::device.eq(device_id))
                    .filter(key_dsl::status.eq(KeyStatus::Next));
                let next_exists: bool = diesel::select(diesel::dsl::exists(next_filter))
                    .get_result(conn)
                    .await?;
                if next_exists {
                    return Err(rest::error::TransactionError::from(
                        rest::error::client_error(
                            StatusCode::CONFLICT,
                            format!(
                                "Already a key with NEXT state present on device {}",
                                device_id
                            ),
                        ),
                    ));
                }
                let active_filter = key_dsl::device_key
                    .filter(key_dsl::device.eq(device_id))
                    .filter(key_dsl::status.eq(KeyStatus::Active));
                let active_exists: bool = diesel::select(diesel::dsl::exists(active_filter))
                    .get_result(conn)
                    .await?;
                if !active_exists {
                    info!(
                        "No ACTIVE key on device {}, setting new key to ACTIVE (initial provisioning)",
                        device_id
                    );
                    key_status = KeyStatus::Active;
                }

                let new_device_key = crate::db::models::NewDeviceKey {
                    device: device_id,
                    key_type,
                    status: key_status,
                };
                let device_key: DeviceKey = diesel::insert_into(key_dsl::device_key)
                    .values(&new_device_key)
                    .returning(DeviceKey::as_returning())
                    .get_result(&mut conn)
                    .await?;

                match payload.kind.clone() {
                    NewDeviceKeyKind::Lightweight { details } => {
                        let to_insert = NewLightweightKeyDetails {
                            device_key: device_key.id,
                            algorithm: details.algorithm,
                            key: details.key,
                        };
                        let insert = diesel::insert_into(lw_dsl::lightweight_key_details)
                            .values(&to_insert)
                            .returning(LightweightKeyDetails::as_returning())
                            .get_result(&mut conn)
                            .await?;
                        kind = DeviceKeyKind::Lightweight {
                            details: LightweightKeyDetailsPayload {
                                algorithm: insert.algorithm,
                                key: insert.key,
                            },
                        };
                    }
                    NewDeviceKeyKind::Tls { details: _ } => {
                        return Err(rest::error::TransactionError::from(
                            rest::error::client_error(
                                StatusCode::CONFLICT,
                                "TLS key functionality not yet implemented".to_string(),
                            ),
                        ));
                    }
                }

                Ok(DeviceKeyPayload {
                    id: device_key.id,
                    status: device_key.status,
                    kind,
                })
            })
        })
        .await;

    use diesel::result::{DatabaseErrorKind, Error as DieselError};

    match tx_result {
        Ok(device_key_payload) => Ok((StatusCode::CREATED, Json(device_key_payload))),

        Err(rest::error::TransactionError::Db(DieselError::DatabaseError(
            DatabaseErrorKind::ForeignKeyViolation,
            info,
        ))) => {
            match info.constraint_name() {
                Some("device_key_device_fkey") => Err(rest::error::client_error(
                    StatusCode::NOT_FOUND,
                    format!("device {} not found", device_id),
                )),
                _ => {
                    // If you want to return an internal error built from the Diesel error:
                    let err =
                        DieselError::DatabaseError(DatabaseErrorKind::ForeignKeyViolation, info);
                    Err(rest::error::internal_error(err))
                }
            }
        }
        Err(rest::error::TransactionError::Db(e)) => Err(rest::error::internal_error(e)),
        Err(rest::error::TransactionError::Api(api)) => Err(api),
    }
}

#[axum::debug_handler]
pub async fn list_device_keys(
    State(api_config): State<rest::RestApiConfig>,
    Path(device_id): Path<i32>,
) -> Result<Json<Vec<DeviceKeyPayload>>, rest::error::ApiError> {
    let mut conn = api_config
        .shared_pool
        .clone()
        .get_owned()
        .await
        .map_err(rest::error::internal_error)?;

    let rows: Vec<(
        DeviceKey,
        Option<LightweightKeyDetails>,
        Option<TlsKeyDetails>,
    )> = dk::table
        .filter(key_dsl::device.eq(device_id))
        .left_outer_join(
            lw::table.on(lw_dsl::device_key
                .eq(key_dsl::id)
                .and(key_dsl::key_type.eq(KeyType::Lightweight))),
        )
        .left_outer_join(
            tls::table.on(tls_dsl::device_key
                .eq(key_dsl::id)
                .and(key_dsl::key_type.eq(KeyType::Tls))),
        )
        .select((
            dk::all_columns,
            lw::all_columns.nullable(),
            tls::all_columns.nullable(),
        ))
        .load(&mut conn)
        .await
        .map_err(rest::error::internal_error)?;

    if rows.is_empty() {
        return Err(rest::error::client_error(
            StatusCode::NOT_FOUND,
            format!("device {} not found", device_id),
        ));
    }

    let mut res = Vec::<DeviceKeyPayload>::new();
    for (key, lw_opt, tls_opt) in rows {
        let kind: DeviceKeyKind;
        if let Some(lw_details) = lw_opt {
            kind = DeviceKeyKind::Lightweight {
                details: lw_details.into(),
            };
        } else if let Some(tls_details) = tls_opt {
            kind = DeviceKeyKind::Tls {
                details: tls_details.into(),
            };
        } else {
            return Err(rest::error::internal_error(
                rest::error::FirmupsRestInternalError {
                    message: format!("No details found for device key {}", key.id),
                },
            ));
        }
        res.push(DeviceKeyPayload {
            id: key.id,
            status: key.status,
            kind,
        });
    }

    Ok(Json(res))
}

#[axum::debug_handler]
pub async fn get_device_key(
    State(api_config): State<rest::RestApiConfig>,
    Path((device_id, path_id)): Path<(i32, i32)>,
) -> Result<Json<DeviceKeyPayload>, rest::error::ApiError> {
    let mut conn = api_config
        .shared_pool
        .clone()
        .get_owned()
        .await
        .map_err(rest::error::internal_error)?;
    let result: Result<
        (
            DeviceKey,
            Option<LightweightKeyDetails>,
            Option<TlsKeyDetails>,
        ),
        diesel::result::Error,
    > = key_dsl::device_key
        .filter(key_dsl::id.eq(path_id))
        .filter(key_dsl::device.eq(device_id))
        .left_outer_join(
            lw::table.on(lw_dsl::device_key
                .eq(key_dsl::id)
                .and(key_dsl::key_type.eq(KeyType::Lightweight))),
        )
        .left_outer_join(
            tls::table.on(tls_dsl::device_key
                .eq(key_dsl::id)
                .and(key_dsl::key_type.eq(KeyType::Tls))),
        )
        .select((
            dk::all_columns,
            lw::all_columns.nullable(),
            tls::all_columns.nullable(),
        ))
        .first(&mut conn)
        .await;
    match result {
        Ok((key, lw_opt, tls_opt)) => {
            let kind: DeviceKeyKind;
            if let Some(lw_details) = lw_opt {
                kind = DeviceKeyKind::Lightweight {
                    details: lw_details.into(),
                };
            } else if let Some(tls_details) = tls_opt {
                kind = DeviceKeyKind::Tls {
                    details: tls_details.into(),
                };
            } else {
                return Err(rest::error::internal_error(
                    rest::error::FirmupsRestInternalError {
                        message: format!("No details found for device key {}", path_id),
                    },
                ));
            }
            Ok(Json(DeviceKeyPayload {
                id: key.id,
                status: key.status,
                kind,
            }))
        }
        Err(diesel::result::Error::NotFound) => Err(rest::error::client_error(
            StatusCode::NOT_FOUND,
            format!("device {} or device key {} not found", device_id, path_id),
        )),
        Err(e) => Err(rest::error::internal_error(e)),
    }
}

#[axum::debug_handler]
pub async fn delete_device_key(
    State(api_config): State<rest::RestApiConfig>,
    Path((device_id, path_id)): Path<(i32, i32)>,
) -> Result<Json<DeviceKeyPayload>, rest::error::ApiError> {
    let mut conn = api_config
        .shared_pool
        .clone()
        .get_owned()
        .await
        .map_err(rest::error::internal_error)?;

    let tx_result: Result<DeviceKeyPayload, rest::error::TransactionError> = conn
        .transaction::<_, rest::error::TransactionError, _>(|mut conn| {
            Box::pin(async move {
                let active_filter = key_dsl::device_key
                    .filter(key_dsl::id.eq(path_id))
                    .filter(key_dsl::device.eq(device_id))
                    .filter(key_dsl::status.eq(KeyStatus::Active));
                let is_active: bool = diesel::select(diesel::dsl::exists(active_filter))
                    .get_result(conn)
                    .await?;
                if is_active {
                    return Err(rest::error::TransactionError::from(
                        rest::error::client_error(
                            StatusCode::CONFLICT,
                            "Active key on device cannot be deleted".to_string(),
                        ),
                    ));
                }

                let (key, lw_opt, tls_opt): (
                    DeviceKey,
                    Option<LightweightKeyDetails>,
                    Option<TlsKeyDetails>,
                ) = key_dsl::device_key
                    .filter(key_dsl::id.eq(path_id))
                    .filter(key_dsl::device.eq(device_id))
                    .left_outer_join(
                        lw::table.on(lw_dsl::device_key
                            .eq(key_dsl::id)
                            .and(key_dsl::key_type.eq(KeyType::Lightweight))),
                    )
                    .left_outer_join(
                        tls::table.on(tls_dsl::device_key
                            .eq(key_dsl::id)
                            .and(key_dsl::key_type.eq(KeyType::Tls))),
                    )
                    .select((
                        dk::all_columns,
                        lw::all_columns.nullable(),
                        tls::all_columns.nullable(),
                    ))
                    .first(&mut conn)
                    .await?;
                let kind: DeviceKeyKind;
                if let Some(lw_details) = lw_opt {
                    kind = DeviceKeyKind::Lightweight {
                        details: lw_details.into(),
                    };
                } else if let Some(tls_details) = tls_opt {
                    kind = DeviceKeyKind::Tls {
                        details: tls_details.into(),
                    };
                } else {
                    return Err(rest::error::TransactionError::from(
                        rest::error::internal_error(rest::error::FirmupsRestInternalError {
                            message: format!("No details found for device key {}", path_id),
                        }),
                    ));
                }

                let _: DeviceKey =
                    diesel::delete(key_dsl::device_key.filter(key_dsl::id.eq(path_id)))
                        .returning(DeviceKey::as_returning())
                        .get_result(&mut conn)
                        .await?;

                Ok(DeviceKeyPayload {
                    id: key.id,
                    status: key.status,
                    kind,
                })
            })
        })
        .await;
    match tx_result {
        Ok(device_key_payload) => Ok(Json(device_key_payload)),
        Err(rest::error::TransactionError::Db(diesel::result::Error::NotFound)) => {
            Err(rest::error::client_error(
                StatusCode::NOT_FOUND,
                format!("device {} or device key {} not found", device_id, path_id),
            ))
        }
        Err(rest::error::TransactionError::Db(e)) => Err(rest::error::internal_error(e)),
        Err(rest::error::TransactionError::Api(api)) => Err(api),
    }
}
