use crate::api::rest;
use crate::api::rest::serde_helpers::{json_value_to_parameter, parameter_to_json_value};
use crate::db::models::{
    Device, DeviceParameter, DeviceStatus, DeviceTypeParameter, NewRollout, NewRolloutPrerequisite,
    NewRolloutStage, ParameterType, PrerequisiteOperator, Rollout, RolloutPrerequisite,
    RolloutStage, RolloutStageStatus, RolloutStatus,
};
use crate::db::schema::device::dsl as device_dsl;
use crate::db::schema::device_parameter::dsl as dp_dsl;
use crate::db::schema::device_type_parameter::dsl as dtp_dsl;
use crate::db::schema::rollout::dsl as rollout_dsl;
use crate::db::schema::rollout_prerequisite::dsl as rp_dsl;
use crate::db::schema::rollout_stage::dsl as stage_dsl;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use chrono::{NaiveDateTime, Utc};
use diesel::ExpressionMethods;
use diesel::OptionalExtension;
use diesel::QueryDsl;
use diesel::SelectableHelper;
use diesel::result::DatabaseErrorKind;
use diesel::result::Error as DieselError;
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Payload types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct CreateRolloutPayload {
    pub name: String,
    pub device_type: i32,
    pub firmware: i32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateRolloutPayload {
    pub name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreatePrerequisitePayload {
    pub device_type_parameter: i32,
    pub operator: PrerequisiteOperator,
    pub value: JsonValue,
}

#[derive(Debug, Clone, Serialize)]
pub struct PrerequisitePayload {
    pub id: i32,
    pub device_type_parameter: i32,
    pub key: String,
    #[serde(rename = "type")]
    pub type_: ParameterType,
    pub operator: PrerequisiteOperator,
    pub value: JsonValue,
}

fn default_threshold() -> i16 {
    100
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateStagePayload {
    pub stage_order: i32,
    pub target_percent: i16,
    #[serde(default = "default_threshold")]
    pub success_threshold_percent: i16,
}

#[derive(Debug, Clone, Serialize)]
pub struct StageStatusPayload {
    pub id: i32,
    pub stage_order: i32,
    pub target_percent: i16,
    pub success_threshold_percent: i16,
    pub status: RolloutStageStatus,
    pub cohort_size: usize,
    pub targeted: usize,
    pub converged: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct RolloutStatusPayload {
    pub id: i32,
    pub name: String,
    pub status: RolloutStatus,
    pub device_type: i32,
    pub firmware: i32,
    pub eligible: usize,
    pub targeted: usize,
    pub converged: usize,
    pub stages: Vec<StageStatusPayload>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EligibleDevicePayload {
    pub device: i32,
    pub targeted: bool,
    pub converged: bool,
    /// Lowest stage (by order) whose cumulative cohort includes this device.
    pub stage_order: Option<i32>,
}

// ---------------------------------------------------------------------------
// Eligibility / cohort helpers
// ---------------------------------------------------------------------------

/// A device of the rollout's type that satisfies every parameter prerequisite.
#[derive(Debug, Clone)]
struct EligibleDevice {
    id: i32,
    converged: bool,
    targeted: bool,
}

fn to_i64(b: &[u8]) -> Option<i64> {
    <[u8; 8]>::try_from(b).ok().map(i64::from_be_bytes)
}

fn to_f64(b: &[u8]) -> Option<f64> {
    <[u8; 8]>::try_from(b).ok().map(f64::from_be_bytes)
}

/// Whether an operator is meaningful for a given parameter type. Ordering
/// comparisons only apply to numeric parameters; equality applies to all.
fn operator_valid_for(op: PrerequisiteOperator, type_: ParameterType) -> bool {
    use PrerequisiteOperator::*;
    match op {
        Eq | Ne => true,
        Lt | Lte | Gt | Gte => matches!(type_, ParameterType::Integer | ParameterType::Float),
    }
}

/// Evaluate a single prerequisite against a device's effective (decoded) value.
/// Undecodable values fail the check rather than erroring.
fn evaluate(op: PrerequisiteOperator, type_: ParameterType, effective: &[u8], target: &[u8]) -> bool {
    use PrerequisiteOperator::*;
    match op {
        Eq => effective == target,
        Ne => effective != target,
        Lt | Lte | Gt | Gte => {
            let ord = match type_ {
                ParameterType::Integer => match (to_i64(effective), to_i64(target)) {
                    (Some(a), Some(b)) => a.partial_cmp(&b),
                    _ => None,
                },
                ParameterType::Float => match (to_f64(effective), to_f64(target)) {
                    (Some(a), Some(b)) => a.partial_cmp(&b),
                    _ => None,
                },
                _ => None,
            };
            match ord {
                Some(o) => match op {
                    Lt => o.is_lt(),
                    Lte => o.is_le(),
                    Gt => o.is_gt(),
                    Gte => o.is_ge(),
                    _ => false,
                },
                None => false,
            }
        }
    }
}

/// Number of devices in the cumulative cohort for a stage targeting
/// `target_percent`% of an eligible population of size `n` (ceil, capped at n).
fn cohort_size(target_percent: i16, n: usize) -> usize {
    let p = target_percent.max(0) as usize;
    (p * n).div_ceil(100).min(n)
}

fn now() -> NaiveDateTime {
    Utc::now().naive_utc()
}

async fn load_rollout(
    conn: &mut AsyncPgConnection,
    rollout_id: i32,
) -> Result<Rollout, rest::error::ApiError> {
    rollout_dsl::rollout
        .find(rollout_id)
        .select(Rollout::as_select())
        .first(conn)
        .await
        .map_err(|e| match e {
            DieselError::NotFound => rest::error::client_error(
                StatusCode::NOT_FOUND,
                format!("rollout {} not found", rollout_id),
            ),
            e => rest::error::internal_error(e),
        })
}

/// Compute the ordered set of eligible devices for a rollout: active devices of
/// the rollout's type whose effective parameter values satisfy all prerequisites.
/// Ordered by device id for deterministic cohort assignment.
async fn eligible_devices(
    conn: &mut AsyncPgConnection,
    r: &Rollout,
) -> Result<Vec<EligibleDevice>, rest::error::ApiError> {
    let prereqs: Vec<RolloutPrerequisite> = rp_dsl::rollout_prerequisite
        .filter(rp_dsl::rollout.eq(r.id))
        .select(RolloutPrerequisite::as_select())
        .load(conn)
        .await
        .map_err(rest::error::internal_error)?;

    let devices: Vec<Device> = device_dsl::device
        .filter(device_dsl::type_.eq(r.device_type))
        .filter(device_dsl::status.eq(DeviceStatus::Active))
        .order(device_dsl::id.asc())
        .select(Device::as_select())
        .load(conn)
        .await
        .map_err(rest::error::internal_error)?;

    let mk = |d: &Device| EligibleDevice {
        id: d.id,
        converged: d.firmware == Some(r.firmware),
        targeted: d.desired_firmware == r.firmware,
    };

    if prereqs.is_empty() {
        return Ok(devices.iter().map(mk).collect());
    }

    let dtp_ids: Vec<i32> = prereqs.iter().map(|p| p.device_type_parameter).collect();
    let dtps: Vec<DeviceTypeParameter> = dtp_dsl::device_type_parameter
        .filter(dtp_dsl::id.eq_any(&dtp_ids))
        .select(DeviceTypeParameter::as_select())
        .load(conn)
        .await
        .map_err(rest::error::internal_error)?;
    let dtp_map: HashMap<i32, DeviceTypeParameter> =
        dtps.into_iter().map(|d| (d.id, d)).collect();

    let device_ids: Vec<i32> = devices.iter().map(|d| d.id).collect();
    let overrides: Vec<DeviceParameter> = dp_dsl::device_parameter
        .filter(dp_dsl::device.eq_any(&device_ids))
        .filter(dp_dsl::device_type_parameter.eq_any(&dtp_ids))
        .select(DeviceParameter::as_select())
        .load(conn)
        .await
        .map_err(rest::error::internal_error)?;
    let mut override_map: HashMap<(i32, i32), Vec<u8>> = HashMap::new();
    for o in overrides {
        if let Some(v) = o.value {
            override_map.insert((o.device, o.device_type_parameter), v);
        }
    }

    let mut result = Vec::new();
    for d in &devices {
        let mut ok = true;
        for p in &prereqs {
            let Some(dtp) = dtp_map.get(&p.device_type_parameter) else {
                ok = false;
                break;
            };
            // effective value: device override, else device-type default
            let effective: Option<&[u8]> = override_map
                .get(&(d.id, p.device_type_parameter))
                .map(|v| v.as_slice())
                .or(dtp.default_value.as_deref());
            let Some(effective) = effective else {
                ok = false;
                break;
            };
            if !evaluate(p.operator, dtp.type_, effective, &p.value) {
                ok = false;
                break;
            }
        }
        if ok {
            result.push(mk(d));
        }
    }
    Ok(result)
}

async fn build_status(
    conn: &mut AsyncPgConnection,
    rollout_id: i32,
) -> Result<RolloutStatusPayload, rest::error::ApiError> {
    let r = load_rollout(conn, rollout_id).await?;
    let stages: Vec<RolloutStage> = stage_dsl::rollout_stage
        .filter(stage_dsl::rollout.eq(r.id))
        .order(stage_dsl::stage_order.asc())
        .select(RolloutStage::as_select())
        .load(conn)
        .await
        .map_err(rest::error::internal_error)?;
    let eligible = eligible_devices(conn, &r).await?;
    let n = eligible.len();

    let stage_payloads = stages
        .iter()
        .map(|s| {
            let size = cohort_size(s.target_percent, n);
            let targeted = eligible.iter().take(size).filter(|e| e.targeted).count();
            let converged = eligible.iter().take(size).filter(|e| e.converged).count();
            StageStatusPayload {
                id: s.id,
                stage_order: s.stage_order,
                target_percent: s.target_percent,
                success_threshold_percent: s.success_threshold_percent,
                status: s.status,
                cohort_size: size,
                targeted,
                converged,
            }
        })
        .collect();

    let targeted = eligible.iter().filter(|e| e.targeted).count();
    let converged = eligible.iter().filter(|e| e.converged).count();

    Ok(RolloutStatusPayload {
        id: r.id,
        name: r.name,
        status: r.status,
        device_type: r.device_type,
        firmware: r.firmware,
        eligible: n,
        targeted,
        converged,
        stages: stage_payloads,
    })
}

/// Ensure a rollout is in draft state for configuration changes.
fn require_draft(r: &Rollout) -> Result<(), rest::error::ApiError> {
    if r.status != RolloutStatus::Draft {
        return Err(rest::error::client_error(
            StatusCode::CONFLICT,
            "rollout can only be modified while in draft".to_string(),
        ));
    }
    Ok(())
}

fn map_tx_error(e: rest::error::TransactionError) -> rest::error::ApiError {
    match e {
        rest::error::TransactionError::Api(api) => api,
        rest::error::TransactionError::Db(db) => rest::error::internal_error(db),
    }
}

// ---------------------------------------------------------------------------
// Rollout CRUD
// ---------------------------------------------------------------------------

#[axum::debug_handler]
pub async fn create_rollout(
    State(api_config): State<rest::RestApiConfig>,
    Json(payload): Json<CreateRolloutPayload>,
) -> Result<(StatusCode, Json<Rollout>), rest::error::ApiError> {
    let name = payload.name.trim().to_string();
    if name.is_empty() {
        return Err(rest::error::client_error(
            StatusCode::BAD_REQUEST,
            "name cannot be empty".to_string(),
        ));
    }
    if name.len() > 100 {
        return Err(rest::error::client_error(
            StatusCode::BAD_REQUEST,
            "name too long (max 100)".to_string(),
        ));
    }

    let mut conn = api_config
        .shared_pool
        .clone()
        .get_owned()
        .await
        .map_err(rest::error::internal_error)?;

    let new_row = NewRollout {
        name,
        device_type: payload.device_type,
        firmware: payload.firmware,
        status: RolloutStatus::Draft,
    };

    let result: Result<Rollout, DieselError> = diesel::insert_into(rollout_dsl::rollout)
        .values(&new_row)
        .returning(Rollout::as_returning())
        .get_result(&mut conn)
        .await;

    match result {
        Ok(created) => Ok((StatusCode::CREATED, Json(created))),
        Err(DieselError::DatabaseError(DatabaseErrorKind::ForeignKeyViolation, info)) => {
            match info.constraint_name() {
                Some("fk_rollout_device_type") => Err(rest::error::client_error(
                    StatusCode::BAD_REQUEST,
                    "unknown device type".to_string(),
                )),
                Some("fk_rollout_firmware") => Err(rest::error::client_error(
                    StatusCode::BAD_REQUEST,
                    "unknown firmware".to_string(),
                )),
                Some("fk_rollout_type_firmware") => Err(rest::error::client_error(
                    StatusCode::BAD_REQUEST,
                    "firmware is not compatible with the device type".to_string(),
                )),
                _ => {
                    let e =
                        DieselError::DatabaseError(DatabaseErrorKind::ForeignKeyViolation, info);
                    Err(rest::error::internal_error(e))
                }
            }
        }
        Err(e) => Err(rest::error::internal_error(e)),
    }
}

#[axum::debug_handler]
pub async fn list_rollouts(
    State(api_config): State<rest::RestApiConfig>,
) -> Result<Json<Vec<Rollout>>, rest::error::ApiError> {
    let mut conn = api_config
        .shared_pool
        .clone()
        .get_owned()
        .await
        .map_err(rest::error::internal_error)?;
    let result = rollout_dsl::rollout
        .order(rollout_dsl::id.asc())
        .select(Rollout::as_select())
        .load(&mut conn)
        .await
        .map_err(rest::error::internal_error)?;
    Ok(Json(result))
}

#[axum::debug_handler]
pub async fn get_rollout(
    State(api_config): State<rest::RestApiConfig>,
    Path(rollout_id): Path<i32>,
) -> Result<Json<Rollout>, rest::error::ApiError> {
    let mut conn = api_config
        .shared_pool
        .clone()
        .get_owned()
        .await
        .map_err(rest::error::internal_error)?;
    let r = load_rollout(&mut conn, rollout_id).await?;
    Ok(Json(r))
}

#[axum::debug_handler]
pub async fn update_rollout(
    State(api_config): State<rest::RestApiConfig>,
    Path(rollout_id): Path<i32>,
    Json(payload): Json<UpdateRolloutPayload>,
) -> Result<Json<Rollout>, rest::error::ApiError> {
    let name = match payload.name {
        Some(n) => {
            let trimmed = n.trim().to_string();
            if trimmed.is_empty() {
                return Err(rest::error::client_error(
                    StatusCode::BAD_REQUEST,
                    "name cannot be empty".to_string(),
                ));
            }
            if trimmed.len() > 100 {
                return Err(rest::error::client_error(
                    StatusCode::BAD_REQUEST,
                    "name too long (max 100)".to_string(),
                ));
            }
            trimmed
        }
        None => {
            return Err(rest::error::client_error(
                StatusCode::BAD_REQUEST,
                "nothing to update".to_string(),
            ));
        }
    };

    let mut conn = api_config
        .shared_pool
        .clone()
        .get_owned()
        .await
        .map_err(rest::error::internal_error)?;

    let updated: Result<Rollout, DieselError> = diesel::update(rollout_dsl::rollout.find(rollout_id))
        .set((
            rollout_dsl::name.eq(name),
            rollout_dsl::updated_at.eq(now()),
        ))
        .returning(Rollout::as_returning())
        .get_result(&mut conn)
        .await;

    match updated {
        Ok(r) => Ok(Json(r)),
        Err(DieselError::NotFound) => Err(rest::error::client_error(
            StatusCode::NOT_FOUND,
            format!("rollout {} not found", rollout_id),
        )),
        Err(e) => Err(rest::error::internal_error(e)),
    }
}

#[axum::debug_handler]
pub async fn delete_rollout(
    State(api_config): State<rest::RestApiConfig>,
    Path(rollout_id): Path<i32>,
) -> Result<Json<Rollout>, rest::error::ApiError> {
    let mut conn = api_config
        .shared_pool
        .clone()
        .get_owned()
        .await
        .map_err(rest::error::internal_error)?;

    let result = conn
        .transaction::<_, rest::error::TransactionError, _>(async move |conn| {
            let r = load_rollout(conn, rollout_id).await?;
            if matches!(r.status, RolloutStatus::Active | RolloutStatus::Paused) {
                return Err(rest::error::client_error(
                    StatusCode::CONFLICT,
                    "cannot delete an active or paused rollout; cancel it first".to_string(),
                )
                .into());
            }
            // prerequisites and stages cascade on delete
            let deleted: Rollout = diesel::delete(rollout_dsl::rollout.find(rollout_id))
                .returning(Rollout::as_returning())
                .get_result(conn)
                .await?;
            Ok(deleted)
        })
        .await
        .map_err(map_tx_error)?;

    Ok(Json(result))
}

// ---------------------------------------------------------------------------
// Prerequisites
// ---------------------------------------------------------------------------

#[axum::debug_handler]
pub async fn list_prerequisites(
    State(api_config): State<rest::RestApiConfig>,
    Path(rollout_id): Path<i32>,
) -> Result<Json<Vec<PrerequisitePayload>>, rest::error::ApiError> {
    let mut conn = api_config
        .shared_pool
        .clone()
        .get_owned()
        .await
        .map_err(rest::error::internal_error)?;

    let result = conn
        .transaction::<_, rest::error::TransactionError, _>(async move |conn| {
            load_rollout(conn, rollout_id).await?;

            let rows: Vec<(RolloutPrerequisite, DeviceTypeParameter)> =
                rp_dsl::rollout_prerequisite
                    .inner_join(dtp_dsl::device_type_parameter)
                    .filter(rp_dsl::rollout.eq(rollout_id))
                    .order(rp_dsl::id.asc())
                    .select((
                        RolloutPrerequisite::as_select(),
                        DeviceTypeParameter::as_select(),
                    ))
                    .load(conn)
                    .await?;

            let payloads = rows
                .into_iter()
                .map(|(p, dtp)| PrerequisitePayload {
                    id: p.id,
                    device_type_parameter: p.device_type_parameter,
                    key: dtp.key.clone(),
                    type_: dtp.type_,
                    operator: p.operator,
                    value: parameter_to_json_value(dtp.type_, p.value).unwrap_or(JsonValue::Null),
                })
                .collect();
            Ok(payloads)
        })
        .await
        .map_err(map_tx_error)?;

    Ok(Json(result))
}

#[axum::debug_handler]
pub async fn create_prerequisite(
    State(api_config): State<rest::RestApiConfig>,
    Path(rollout_id): Path<i32>,
    Json(payload): Json<CreatePrerequisitePayload>,
) -> Result<(StatusCode, Json<PrerequisitePayload>), rest::error::ApiError> {
    let mut conn = api_config
        .shared_pool
        .clone()
        .get_owned()
        .await
        .map_err(rest::error::internal_error)?;

    let result = conn
        .transaction::<_, rest::error::TransactionError, _>(async move |conn| {
            let r = load_rollout(conn, rollout_id).await?;
            require_draft(&r)?;

            // Parameter must belong to the rollout's device type.
            let dtp: DeviceTypeParameter = dtp_dsl::device_type_parameter
                .find(payload.device_type_parameter)
                .select(DeviceTypeParameter::as_select())
                .first(conn)
                .await
                .optional()?
                .ok_or_else(|| {
                    rest::error::client_error(
                        StatusCode::BAD_REQUEST,
                        format!("unknown parameter {}", payload.device_type_parameter),
                    )
                })?;

            if dtp.device_type != r.device_type {
                return Err(rest::error::client_error(
                    StatusCode::BAD_REQUEST,
                    "parameter does not belong to the rollout's device type".to_string(),
                )
                .into());
            }

            if !operator_valid_for(payload.operator, dtp.type_) {
                return Err(rest::error::client_error(
                    StatusCode::BAD_REQUEST,
                    "ordering operators are only valid for integer or float parameters".to_string(),
                )
                .into());
            }

            let value_bytes = json_value_to_parameter(dtp.type_, payload.value.clone())
                .map_err(|msg| rest::error::client_error(StatusCode::BAD_REQUEST, msg))?;

            let new_row = NewRolloutPrerequisite {
                rollout: rollout_id,
                device_type_parameter: payload.device_type_parameter,
                operator: payload.operator,
                value: value_bytes,
            };

            let created: RolloutPrerequisite = diesel::insert_into(rp_dsl::rollout_prerequisite)
                .values(&new_row)
                .returning(RolloutPrerequisite::as_returning())
                .get_result(conn)
                .await
                .map_err(|e| match e {
                    DieselError::DatabaseError(DatabaseErrorKind::UniqueViolation, _) => {
                        rest::error::client_error(
                            StatusCode::CONFLICT,
                            "a prerequisite with this parameter and operator already exists"
                                .to_string(),
                        )
                        .into()
                    }
                    e => rest::error::TransactionError::Db(e),
                })?;

            Ok(PrerequisitePayload {
                id: created.id,
                device_type_parameter: created.device_type_parameter,
                key: dtp.key.clone(),
                type_: dtp.type_,
                operator: created.operator,
                value: parameter_to_json_value(dtp.type_, created.value)
                    .unwrap_or(JsonValue::Null),
            })
        })
        .await
        .map_err(map_tx_error)?;

    Ok((StatusCode::CREATED, Json(result)))
}

#[axum::debug_handler]
pub async fn delete_prerequisite(
    State(api_config): State<rest::RestApiConfig>,
    Path((rollout_id, prereq_id)): Path<(i32, i32)>,
) -> Result<StatusCode, rest::error::ApiError> {
    let mut conn = api_config
        .shared_pool
        .clone()
        .get_owned()
        .await
        .map_err(rest::error::internal_error)?;

    conn.transaction::<_, rest::error::TransactionError, _>(async move |conn| {
        let r = load_rollout(conn, rollout_id).await?;
        require_draft(&r)?;

        let deleted = diesel::delete(
            rp_dsl::rollout_prerequisite
                .filter(rp_dsl::id.eq(prereq_id))
                .filter(rp_dsl::rollout.eq(rollout_id)),
        )
        .execute(conn)
        .await?;

        if deleted == 0 {
            return Err(rest::error::client_error(
                StatusCode::NOT_FOUND,
                format!("prerequisite {} not found", prereq_id),
            )
            .into());
        }
        Ok(())
    })
    .await
    .map_err(map_tx_error)?;

    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Stages
// ---------------------------------------------------------------------------

#[axum::debug_handler]
pub async fn list_stages(
    State(api_config): State<rest::RestApiConfig>,
    Path(rollout_id): Path<i32>,
) -> Result<Json<Vec<RolloutStage>>, rest::error::ApiError> {
    let mut conn = api_config
        .shared_pool
        .clone()
        .get_owned()
        .await
        .map_err(rest::error::internal_error)?;

    let result = conn
        .transaction::<_, rest::error::TransactionError, _>(async move |conn| {
            load_rollout(conn, rollout_id).await?;
            let stages: Vec<RolloutStage> = stage_dsl::rollout_stage
                .filter(stage_dsl::rollout.eq(rollout_id))
                .order(stage_dsl::stage_order.asc())
                .select(RolloutStage::as_select())
                .load(conn)
                .await?;
            Ok(stages)
        })
        .await
        .map_err(map_tx_error)?;

    Ok(Json(result))
}

#[axum::debug_handler]
pub async fn create_stage(
    State(api_config): State<rest::RestApiConfig>,
    Path(rollout_id): Path<i32>,
    Json(payload): Json<CreateStagePayload>,
) -> Result<(StatusCode, Json<RolloutStage>), rest::error::ApiError> {
    if payload.stage_order < 1 {
        return Err(rest::error::client_error(
            StatusCode::BAD_REQUEST,
            "stage_order must be >= 1".to_string(),
        ));
    }
    if !(1..=100).contains(&payload.target_percent) {
        return Err(rest::error::client_error(
            StatusCode::BAD_REQUEST,
            "target_percent must be between 1 and 100".to_string(),
        ));
    }
    if !(0..=100).contains(&payload.success_threshold_percent) {
        return Err(rest::error::client_error(
            StatusCode::BAD_REQUEST,
            "success_threshold_percent must be between 0 and 100".to_string(),
        ));
    }

    let mut conn = api_config
        .shared_pool
        .clone()
        .get_owned()
        .await
        .map_err(rest::error::internal_error)?;

    let result = conn
        .transaction::<_, rest::error::TransactionError, _>(async move |conn| {
            let r = load_rollout(conn, rollout_id).await?;
            require_draft(&r)?;

            let existing: Vec<RolloutStage> = stage_dsl::rollout_stage
                .filter(stage_dsl::rollout.eq(rollout_id))
                .select(RolloutStage::as_select())
                .load(conn)
                .await?;

            // target_percent must strictly increase with stage_order
            for s in &existing {
                if s.stage_order < payload.stage_order && s.target_percent >= payload.target_percent
                {
                    return Err(rest::error::client_error(
                        StatusCode::BAD_REQUEST,
                        "target_percent must strictly increase with stage_order".to_string(),
                    )
                    .into());
                }
                if s.stage_order > payload.stage_order && s.target_percent <= payload.target_percent
                {
                    return Err(rest::error::client_error(
                        StatusCode::BAD_REQUEST,
                        "target_percent must strictly increase with stage_order".to_string(),
                    )
                    .into());
                }
            }

            let new_row = NewRolloutStage {
                rollout: rollout_id,
                stage_order: payload.stage_order,
                target_percent: payload.target_percent,
                success_threshold_percent: payload.success_threshold_percent,
            };

            let created: RolloutStage = diesel::insert_into(stage_dsl::rollout_stage)
                .values(&new_row)
                .returning(RolloutStage::as_returning())
                .get_result(conn)
                .await
                .map_err(|e| match e {
                    DieselError::DatabaseError(DatabaseErrorKind::UniqueViolation, _) => {
                        rest::error::client_error(
                            StatusCode::CONFLICT,
                            format!("stage_order {} already exists", payload.stage_order),
                        )
                        .into()
                    }
                    e => rest::error::TransactionError::Db(e),
                })?;

            Ok(created)
        })
        .await
        .map_err(map_tx_error)?;

    Ok((StatusCode::CREATED, Json(result)))
}

#[axum::debug_handler]
pub async fn delete_stage(
    State(api_config): State<rest::RestApiConfig>,
    Path((rollout_id, stage_id)): Path<(i32, i32)>,
) -> Result<StatusCode, rest::error::ApiError> {
    let mut conn = api_config
        .shared_pool
        .clone()
        .get_owned()
        .await
        .map_err(rest::error::internal_error)?;

    conn.transaction::<_, rest::error::TransactionError, _>(async move |conn| {
        let r = load_rollout(conn, rollout_id).await?;
        require_draft(&r)?;

        let deleted = diesel::delete(
            stage_dsl::rollout_stage
                .filter(stage_dsl::id.eq(stage_id))
                .filter(stage_dsl::rollout.eq(rollout_id)),
        )
        .execute(conn)
        .await?;

        if deleted == 0 {
            return Err(rest::error::client_error(
                StatusCode::NOT_FOUND,
                format!("stage {} not found", stage_id),
            )
            .into());
        }
        Ok(())
    })
    .await
    .map_err(map_tx_error)?;

    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Lifecycle
// ---------------------------------------------------------------------------

#[axum::debug_handler]
pub async fn start_rollout(
    State(api_config): State<rest::RestApiConfig>,
    Path(rollout_id): Path<i32>,
) -> Result<Json<RolloutStatusPayload>, rest::error::ApiError> {
    let mut conn = api_config
        .shared_pool
        .clone()
        .get_owned()
        .await
        .map_err(rest::error::internal_error)?;

    conn.transaction::<_, rest::error::TransactionError, _>(async move |conn| {
        let r = load_rollout(conn, rollout_id).await?;
        if r.status != RolloutStatus::Draft {
            return Err(rest::error::client_error(
                StatusCode::CONFLICT,
                "only a draft rollout can be started".to_string(),
            )
            .into());
        }

        let stages: Vec<RolloutStage> = stage_dsl::rollout_stage
            .filter(stage_dsl::rollout.eq(rollout_id))
            .select(RolloutStage::as_select())
            .load(conn)
            .await?;

        if stages.is_empty() {
            return Err(rest::error::client_error(
                StatusCode::BAD_REQUEST,
                "rollout has no stages".to_string(),
            )
            .into());
        }
        if !stages.iter().any(|s| s.target_percent == 100) {
            return Err(rest::error::client_error(
                StatusCode::BAD_REQUEST,
                "rollout must have a final stage reaching 100%".to_string(),
            )
            .into());
        }

        diesel::update(rollout_dsl::rollout.find(rollout_id))
            .set((
                rollout_dsl::status.eq(RolloutStatus::Active),
                rollout_dsl::updated_at.eq(now()),
            ))
            .execute(conn)
            .await?;
        Ok(())
    })
    .await
    .map_err(map_tx_error)?;

    let status = build_status(&mut conn, rollout_id).await?;
    Ok(Json(status))
}

#[axum::debug_handler]
pub async fn advance_rollout(
    State(api_config): State<rest::RestApiConfig>,
    Path(rollout_id): Path<i32>,
) -> Result<Json<RolloutStatusPayload>, rest::error::ApiError> {
    let mut conn = api_config
        .shared_pool
        .clone()
        .get_owned()
        .await
        .map_err(rest::error::internal_error)?;

    conn.transaction::<_, rest::error::TransactionError, _>(async move |conn| {
        // Serialize concurrent stage execution per rollout (namespace 3).
        diesel::sql_query("SELECT pg_advisory_xact_lock(3, $1)")
            .bind::<diesel::sql_types::Integer, _>(rollout_id)
            .execute(conn)
            .await?;

        let r = load_rollout(conn, rollout_id).await?;
        if r.status != RolloutStatus::Active {
            return Err(rest::error::client_error(
                StatusCode::CONFLICT,
                "rollout is not active".to_string(),
            )
            .into());
        }

        let stages: Vec<RolloutStage> = stage_dsl::rollout_stage
            .filter(stage_dsl::rollout.eq(rollout_id))
            .order(stage_dsl::stage_order.asc())
            .select(RolloutStage::as_select())
            .load(conn)
            .await?;

        let eligible = eligible_devices(conn, &r).await?;
        let n = eligible.len();
        let ts = now();

        // 1. Health gate: the current in-progress stage must have converged.
        if let Some(cur) = stages
            .iter()
            .find(|s| s.status == RolloutStageStatus::InProgress)
        {
            let size = cohort_size(cur.target_percent, n);
            let converged = eligible.iter().take(size).filter(|e| e.converged).count();
            let required = (cur.success_threshold_percent.max(0) as usize * size).div_ceil(100);
            if converged < required {
                return Err(rest::error::client_error(
                    StatusCode::CONFLICT,
                    format!(
                        "stage {} has not converged: {}/{} devices on target firmware, need {}",
                        cur.stage_order, converged, size, required
                    ),
                )
                .into());
            }
            diesel::update(stage_dsl::rollout_stage.find(cur.id))
                .set((
                    stage_dsl::status.eq(RolloutStageStatus::Completed),
                    stage_dsl::completed_at.eq(ts),
                ))
                .execute(conn)
                .await?;
        }

        // 2. Start the next pending stage, or complete the rollout.
        let next = stages
            .iter()
            .filter(|s| s.status == RolloutStageStatus::Pending)
            .min_by_key(|s| s.stage_order)
            .cloned();

        match next {
            Some(stage) => {
                let size = cohort_size(stage.target_percent, n);
                let cohort_ids: Vec<i32> = eligible.iter().take(size).map(|e| e.id).collect();
                if !cohort_ids.is_empty() {
                    diesel::update(
                        device_dsl::device
                            .filter(device_dsl::id.eq_any(&cohort_ids))
                            .filter(device_dsl::desired_firmware.ne(r.firmware)),
                    )
                    .set(device_dsl::desired_firmware.eq(r.firmware))
                    .execute(conn)
                    .await?;
                }
                diesel::update(stage_dsl::rollout_stage.find(stage.id))
                    .set((
                        stage_dsl::status.eq(RolloutStageStatus::InProgress),
                        stage_dsl::started_at.eq(ts),
                    ))
                    .execute(conn)
                    .await?;
                diesel::update(rollout_dsl::rollout.find(rollout_id))
                    .set(rollout_dsl::updated_at.eq(ts))
                    .execute(conn)
                    .await?;
            }
            None => {
                diesel::update(rollout_dsl::rollout.find(rollout_id))
                    .set((
                        rollout_dsl::status.eq(RolloutStatus::Completed),
                        rollout_dsl::updated_at.eq(ts),
                    ))
                    .execute(conn)
                    .await?;
            }
        }
        Ok(())
    })
    .await
    .map_err(map_tx_error)?;

    let status = build_status(&mut conn, rollout_id).await?;
    Ok(Json(status))
}

/// Shared state transition for pause/resume/cancel.
async fn transition(
    api_config: &rest::RestApiConfig,
    rollout_id: i32,
    allowed_from: &[RolloutStatus],
    to: RolloutStatus,
    err_msg: &str,
) -> Result<Json<Rollout>, rest::error::ApiError> {
    let mut conn = api_config
        .shared_pool
        .clone()
        .get_owned()
        .await
        .map_err(rest::error::internal_error)?;

    let allowed: Vec<RolloutStatus> = allowed_from.to_vec();
    let msg = err_msg.to_string();

    let result = conn
        .transaction::<_, rest::error::TransactionError, _>(async move |conn| {
            let r = load_rollout(conn, rollout_id).await?;
            if !allowed.contains(&r.status) {
                return Err(rest::error::client_error(StatusCode::CONFLICT, msg).into());
            }
            let updated: Rollout = diesel::update(rollout_dsl::rollout.find(rollout_id))
                .set((rollout_dsl::status.eq(to), rollout_dsl::updated_at.eq(now())))
                .returning(Rollout::as_returning())
                .get_result(conn)
                .await?;
            Ok(updated)
        })
        .await
        .map_err(map_tx_error)?;

    Ok(Json(result))
}

#[axum::debug_handler]
pub async fn pause_rollout(
    State(api_config): State<rest::RestApiConfig>,
    Path(rollout_id): Path<i32>,
) -> Result<Json<Rollout>, rest::error::ApiError> {
    transition(
        &api_config,
        rollout_id,
        &[RolloutStatus::Active],
        RolloutStatus::Paused,
        "only an active rollout can be paused",
    )
    .await
}

#[axum::debug_handler]
pub async fn resume_rollout(
    State(api_config): State<rest::RestApiConfig>,
    Path(rollout_id): Path<i32>,
) -> Result<Json<Rollout>, rest::error::ApiError> {
    transition(
        &api_config,
        rollout_id,
        &[RolloutStatus::Paused],
        RolloutStatus::Active,
        "only a paused rollout can be resumed",
    )
    .await
}

#[axum::debug_handler]
pub async fn cancel_rollout(
    State(api_config): State<rest::RestApiConfig>,
    Path(rollout_id): Path<i32>,
) -> Result<Json<Rollout>, rest::error::ApiError> {
    transition(
        &api_config,
        rollout_id,
        &[
            RolloutStatus::Draft,
            RolloutStatus::Active,
            RolloutStatus::Paused,
        ],
        RolloutStatus::Cancelled,
        "rollout is already completed or cancelled",
    )
    .await
}

// ---------------------------------------------------------------------------
// Status / devices
// ---------------------------------------------------------------------------

#[axum::debug_handler]
pub async fn rollout_status(
    State(api_config): State<rest::RestApiConfig>,
    Path(rollout_id): Path<i32>,
) -> Result<Json<RolloutStatusPayload>, rest::error::ApiError> {
    let mut conn = api_config
        .shared_pool
        .clone()
        .get_owned()
        .await
        .map_err(rest::error::internal_error)?;

    let status = conn
        .transaction::<_, rest::error::TransactionError, _>(async move |conn| {
            Ok(build_status(conn, rollout_id).await?)
        })
        .await
        .map_err(map_tx_error)?;

    Ok(Json(status))
}

#[axum::debug_handler]
pub async fn list_rollout_devices(
    State(api_config): State<rest::RestApiConfig>,
    Path(rollout_id): Path<i32>,
) -> Result<Json<Vec<EligibleDevicePayload>>, rest::error::ApiError> {
    let mut conn = api_config
        .shared_pool
        .clone()
        .get_owned()
        .await
        .map_err(rest::error::internal_error)?;

    let result = conn
        .transaction::<_, rest::error::TransactionError, _>(async move |conn| {
            let r = load_rollout(conn, rollout_id).await?;
            let stages: Vec<RolloutStage> = stage_dsl::rollout_stage
                .filter(stage_dsl::rollout.eq(rollout_id))
                .order(stage_dsl::stage_order.asc())
                .select(RolloutStage::as_select())
                .load(conn)
                .await?;
            let eligible = eligible_devices(conn, &r).await?;
            let n = eligible.len();

            // Precompute each stage's cumulative cohort size.
            let stage_cohorts: Vec<(i32, usize)> = stages
                .iter()
                .map(|s| (s.stage_order, cohort_size(s.target_percent, n)))
                .collect();

            let payloads = eligible
                .iter()
                .enumerate()
                .map(|(idx, e)| {
                    let stage_order = stage_cohorts
                        .iter()
                        .find(|(_, size)| idx < *size)
                        .map(|(order, _)| *order);
                    EligibleDevicePayload {
                        device: e.id,
                        targeted: e.targeted,
                        converged: e.converged,
                        stage_order,
                    }
                })
                .collect();
            Ok(payloads)
        })
        .await
        .map_err(map_tx_error)?;

    Ok(Json(result))
}
