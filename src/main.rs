use axum::{
    extract::{FromRef, FromRequestParts, State},
    http::{request::Parts, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router
};
use serde::{Deserialize, Serialize};
use log::info;
use diesel::{associations::HasTable, prelude::*};
use diesel_async::{
    pooled_connection::{bb8, AsyncDieselConnectionManager},
    AsyncMigrationHarness, AsyncPgConnection, RunQueryDsl,
};
use dotenvy::dotenv;
use std::env;

// pub fn establish_connection() -> PgConnection {
//     dotenv().ok();

//     let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
//     PgConnection::establish(&database_url)
//         .unwrap_or_else(|_| panic!("Error connecting to {}", database_url))
// }

pub mod models;
pub mod schema;

use self::models::*;
type DbPool = bb8::Pool<AsyncPgConnection>;

#[tokio::main]
async fn main() {
    // load .env variables
    dotenv().ok();
    // initialize logging
    env_logger::init();

    // set up connection pool
    let db_url = std::env::var("DATABASE_URL").unwrap();
    let config = AsyncDieselConnectionManager::<diesel_async::AsyncPgConnection>::new(db_url);
    let pool = DbPool::builder().build(config).await.expect("Failed to create pool");

    // build our application with a route
    let app = Router::new()
        // `GET /` goes to `root`
        .route("/", get(get_devices))
        .with_state(pool);


    // run our app with hyper
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .unwrap();
    info!("listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}

// struct DatabaseConnection(bb8::PooledConnection<'static, AsyncPgConnection>);

// impl<S> FromRequestParts<S> for DatabaseConnection
// where
//     S: Send + Sync,
//     Pool: FromRef<S>,
// {
//     type Rejection = (StatusCode, String);

//     async fn from_request_parts(_parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
//         let pool = Pool::from_ref(state);

//         let conn = pool.get_owned().await.map_err(internal_error)?;

//         Ok(Self(conn))
//     }
// }

#[axum::debug_handler]
async fn get_devices(
    State(pool): State<DbPool>,
) -> Result<Json<Vec<Device>>, (axum::http::StatusCode, String)> {
    use schema::devices::dsl::*;

    let mut conn = pool.get_owned().await.map_err(internal_error)?;
    let result = devices
        .select(Device::as_select())
        .load(&mut conn)
        .await
        .map_err(internal_error)?;

    Ok(Json(result))
}


async fn create_user(
    // this argument tells axum to parse the request body
    // as JSON into a `CreateUser` type
    Json(payload): Json<CreateUser>,
) -> impl IntoResponse {
    // insert your application logic here
    let user = User {
        id: 1337,
        username: payload.username,
    };

    // this will be converted into a JSON response
    // with a status code of `201 Created`
    (StatusCode::CREATED, Json(user))
}

// the input to our `create_user` handler
#[derive(Deserialize)]
struct CreateUser {
    username: String,
}

// the output to our `create_user` handler
#[derive(Serialize)]
struct User {
    id: u64,
    username: String,
}


fn internal_error<E>(err: E) -> (StatusCode, String)
where
    E: std::error::Error,
{
    (StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
}