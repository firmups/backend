use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use log::error;
use serde::Serialize;
use std::fmt;
use thiserror::Error;
use uuid::Uuid;

#[derive(Serialize, Debug)]
pub struct InternalErrorBody {
    pub error_id: String,
}

#[derive(Serialize, Debug)]
pub struct ErrorBody {
    pub error: String,
}

#[derive(Error, Debug)]
pub enum TransactionError {
    #[error(transparent)]
    Db(#[from] diesel::result::Error),

    #[error(transparent)]
    Api(#[from] ApiError),
}

#[derive(Error, Debug)]
pub enum ApiError {
    Client {
        status: StatusCode,
        body: ErrorBody,
    },
    Internal {
        status: StatusCode,
        body: InternalErrorBody,
    },
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ApiError::Client { body, .. } => write!(f, "{body}"),
            ApiError::Internal { body, .. } => write!(f, "{body}"),
        }
    }
}

impl fmt::Display for ErrorBody {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "error: {}", self.error)
    }
}
impl fmt::Display for InternalErrorBody {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "error_id: {}", self.error_id)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        match self {
            ApiError::Client { status, body } => (status, Json(body)).into_response(),
            ApiError::Internal { status, body } => (status, Json(body)).into_response(),
        }
    }
}

pub fn client_error(status_code: StatusCode, err: String) -> ApiError {
    error!("Client Error: {err}");
    ApiError::Client {
        status: status_code,
        body: ErrorBody { error: err },
    }
}

pub fn internal_error<E>(err: E) -> ApiError
where
    E: std::error::Error,
{
    let error_id = Uuid::new_v4();

    // Log the UUID and the error message for correlation
    let error_id_string = error_id.to_string();
    let error_string = err.to_string();
    error!("[{error_id_string}] Internal error: {error_string}");

    ApiError::Internal {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        body: InternalErrorBody {
            error_id: error_id_string,
        },
    }
}

#[derive(Error, Debug)]
pub struct FirmupsRestInternalError {
    pub message: String,
}

impl fmt::Display for FirmupsRestInternalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}
