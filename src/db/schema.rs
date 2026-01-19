// @generated automatically by Diesel CLI.

pub mod sql_types {
    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "crypto_algorithm"))]
    pub struct CryptoAlgorithm;

    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "device_status"))]
    pub struct DeviceStatus;

    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "key_status"))]
    pub struct KeyStatus;

    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "key_type"))]
    pub struct KeyType;

    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "parameter_type"))]
    pub struct ParameterType;
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::DeviceStatus;

    device (id) {
        id -> Int4,
        #[max_length = 100]
        name -> Varchar,
        #[sql_name = "type"]
        type_ -> Int4,
        firmware -> Nullable<Int4>,
        desired_firmware -> Int4,
        status -> DeviceStatus,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::KeyType;
    use super::sql_types::KeyStatus;

    device_key (id) {
        id -> Int4,
        device -> Int4,
        key_type -> KeyType,
        status -> KeyStatus,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::ParameterType;

    device_parameter (id) {
        id -> Int4,
        device -> Int4,
        #[max_length = 100]
        key -> Varchar,
        #[sql_name = "type"]
        type_ -> ParameterType,
        value -> Nullable<Bytea>,
    }
}

diesel::table! {
    device_type (id) {
        id -> Int4,
        #[max_length = 100]
        name -> Varchar,
    }
}

diesel::table! {
    device_type_firmware (id) {
        id -> Int4,
        device_type -> Int4,
        firmware -> Int4,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::ParameterType;

    device_type_parameter (id) {
        id -> Int4,
        device_type -> Int4,
        #[max_length = 100]
        key -> Varchar,
        #[sql_name = "type"]
        type_ -> ParameterType,
        default_value -> Nullable<Bytea>,
    }
}

diesel::table! {
    firmware (id) {
        id -> Int4,
        #[max_length = 100]
        name -> Varchar,
        #[max_length = 100]
        version -> Varchar,
        #[max_length = 36]
        file_id -> Varchar,
        size -> Int8,
        #[max_length = 64]
        sha256 -> Varchar,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::CryptoAlgorithm;

    lightweight_key_details (id) {
        id -> Int4,
        device_key -> Int4,
        algorithm -> CryptoAlgorithm,
        key -> Bytea,
    }
}

diesel::table! {
    tls_key_details (id) {
        id -> Int4,
        device_key -> Int4,
        valid_from -> Timestamp,
        valid_to -> Timestamp,
    }
}

diesel::joinable!(device -> device_type (type_));
diesel::joinable!(device_key -> device (device));
diesel::joinable!(device_parameter -> device (device));
diesel::joinable!(device_type_firmware -> device_type (device_type));
diesel::joinable!(device_type_firmware -> firmware (firmware));
diesel::joinable!(device_type_parameter -> device_type (device_type));
diesel::joinable!(lightweight_key_details -> device_key (device_key));
diesel::joinable!(tls_key_details -> device_key (device_key));

diesel::allow_tables_to_appear_in_same_query!(
    device,
    device_key,
    device_parameter,
    device_type,
    device_type_firmware,
    device_type_parameter,
    firmware,
    lightweight_key_details,
    tls_key_details,
);
