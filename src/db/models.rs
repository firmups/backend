use chrono::NaiveDateTime;
use diesel::prelude::*;

use diesel_derive_enum::DbEnum;

#[derive(Debug, Clone, Copy, PartialEq, Eq, DbEnum)]
#[ExistingTypePath = "crate::db::schema::sql_types::CryptoAlgorithm"]
pub enum CryptoAlgorithm {
    /// Maps to the Postgres enum label 'AES-GCM'
    #[db_rename = "AES-GCM"]
    AesGcm,

    /// Maps to the Postgres enum label 'ASCON-AEAD128'
    #[db_rename = "ASCON-AEAD128"]
    AsconAead128,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, DbEnum, serde::Serialize, serde::Deserialize)]
#[ExistingTypePath = "crate::db::schema::sql_types::DeviceStatus"]
#[DbValueStyle = "snake_case"]
pub enum DeviceStatus {
    ACTIVE,
    INACTIVE,
    MAINTENANCE,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, DbEnum)]
#[ExistingTypePath = "crate::db::schema::sql_types::KeyStatus"]
#[DbValueStyle = "snake_case"]
pub enum KeyStatus {
    ACTIVE,
    NEXT,
    EXPIRED,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, DbEnum)]
#[ExistingTypePath = "crate::db::schema::sql_types::KeyType"]
#[DbValueStyle = "snake_case"]
pub enum KeyType {
    LIGHTWEIGHT,
    TLS,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, DbEnum)]
#[ExistingTypePath = "crate::db::schema::sql_types::ParameterType"]
#[DbValueStyle = "snake_case"]
pub enum ParameterType {
    STRING,
    INTEGER,
    BOOLEAN,
    FLOAT,
    BINARY,
}

// -----------------------------
// Models
// -----------------------------

// device
#[derive(Debug, Clone, Identifiable, Queryable, Selectable, AsChangeset, serde::Serialize)]
#[diesel(table_name = crate::db::schema::device)]
pub struct Device {
    pub id: i32,
    pub type_: i32,            // FK -> device_type.id
    pub firmware: Option<i32>, // FK -> firmware.id (nullable)
    pub desired_firmware: i32,
    pub status: DeviceStatus,
}

#[derive(Debug, Clone, Insertable, serde::Serialize, serde::Deserialize)]
#[diesel(table_name = crate::db::schema::device)]
pub struct NewDevice {
    pub name: String,
    pub type_: i32,
    pub firmware: Option<i32>,
    pub desired_firmware: i32,
    pub status: DeviceStatus,
}

// device_key
#[derive(Debug, Clone, Identifiable, Queryable, Selectable, Associations, AsChangeset)]
#[diesel(table_name = crate::db::schema::device_key)]
#[diesel(belongs_to(Device, foreign_key = device))]
pub struct DeviceKey {
    pub id: i32,
    pub device: i32, // FK -> device.id
    pub key_type: KeyType,
    pub status: KeyStatus,
    pub key_details_id: i32, // points to either lightweight/tls detail row (polymorphic usage)
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = crate::db::schema::device_key)]
pub struct NewDeviceKey {
    pub device: i32,
    pub key_type: KeyType,
    pub status: KeyStatus,
    pub key_details_id: i32,
}

// device_parameter
#[derive(Debug, Clone, Identifiable, Queryable, Selectable, Associations, AsChangeset)]
#[diesel(table_name = crate::db::schema::device_parameter)]
#[diesel(belongs_to(Device, foreign_key = device))]
pub struct DeviceParameter {
    pub id: i32,
    pub device: i32, // FK -> device.id
    pub key: String,
    pub type_: ParameterType,
    pub value: Option<Vec<u8>>, // Bytea
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = crate::db::schema::device_parameter)]
pub struct NewDeviceParameter {
    pub device: i32,
    pub key: String,
    pub type_: ParameterType,
    pub value: Option<Vec<u8>>,
}

// device_type
#[derive(Debug, Clone, Identifiable, Queryable, Selectable, AsChangeset, serde::Serialize)]
#[diesel(table_name = crate::db::schema::device_type)]
pub struct DeviceType {
    pub id: i32,
    pub name: String,
}

#[derive(Debug, Clone, Insertable, serde::Serialize, serde::Deserialize)]
#[diesel(table_name = crate::db::schema::device_type)]
pub struct NewDeviceType {
    pub name: String,
}

// device_type_firmware
#[derive(
    Debug,
    Clone,
    Identifiable,
    Queryable,
    Selectable,
    Associations,
    AsChangeset,
    serde::Serialize,
    serde::Deserialize,
)]
#[diesel(table_name = crate::db::schema::device_type_firmware)]
#[diesel(belongs_to(DeviceType, foreign_key = device_type))]
#[diesel(belongs_to(Firmware, foreign_key = firmware))]
pub struct DeviceTypeFirmware {
    pub id: i32,
    pub device_type: i32, // FK -> device_type.id
    pub firmware: i32,    // FK -> firmware.id
}

#[derive(Debug, Clone, Insertable, serde::Serialize, serde::Deserialize)]
#[diesel(table_name = crate::db::schema::device_type_firmware)]
pub struct NewDeviceTypeFirmware {
    pub device_type: i32,
    pub firmware: i32,
}

// device_type_parameter
#[derive(Debug, Clone, Identifiable, Queryable, Selectable, Associations, AsChangeset)]
#[diesel(table_name = crate::db::schema::device_type_parameter)]
#[diesel(belongs_to(DeviceType, foreign_key = device_type))]
pub struct DeviceTypeParameter {
    pub id: i32,
    pub device_type: i32, // FK -> device_type.id
    pub key: String,
    pub type_: ParameterType,
    pub default_value: Option<Vec<u8>>, // Bytea
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = crate::db::schema::device_type_parameter)]
pub struct NewDeviceTypeParameter {
    pub device_type: i32,
    pub key: String,
    pub type_: ParameterType,
}

// firmware
#[derive(
    Debug,
    Clone,
    Identifiable,
    Queryable,
    Selectable,
    AsChangeset,
    serde::Serialize,
    serde::Deserialize,
)]
#[diesel(table_name = crate::db::schema::firmware)]
pub struct Firmware {
    pub id: i32,
    pub name: String,
    pub version: String,
    pub file_id: String,
    pub size: i64,
    pub sha256: String,
}

#[derive(Debug, Clone, Insertable, serde::Serialize, serde::Deserialize)]
#[diesel(table_name = crate::db::schema::firmware)]
pub struct NewFirmware {
    pub name: String,
    pub version: String,
    pub file_id: String,
    pub size: i64,
    pub sha256: String,
}

// lightweight_key_details
#[derive(Debug, Clone, Identifiable, Queryable, Selectable, Associations, AsChangeset)]
#[diesel(table_name = crate::db::schema::lightweight_key_details)]
#[diesel(belongs_to(DeviceKey, foreign_key = device_key))]
pub struct LightweightKeyDetails {
    pub id: i32,
    pub device_key: i32, // FK -> device_key.id
    pub algorithm: CryptoAlgorithm,
    pub key: Vec<u8>,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = crate::db::schema::lightweight_key_details)]
pub struct NewLightweightKeyDetails {
    pub device_key: i32,
    pub algorithm: CryptoAlgorithm,
    pub key: Vec<u8>,
}

// tls_key_details
#[derive(Debug, Clone, Identifiable, Queryable, Selectable, Associations, AsChangeset)]
#[diesel(table_name = crate::db::schema::tls_key_details)]
#[diesel(belongs_to(DeviceKey, foreign_key = device_key))]
pub struct TlsKeyDetails {
    pub id: i32,
    pub device_key: i32, // FK -> device_key.id
    pub valid_from: NaiveDateTime,
    pub valid_to: NaiveDateTime,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = crate::db::schema::tls_key_details)]
pub struct NewTlsKeyDetails {
    pub device_key: i32,
    pub valid_from: NaiveDateTime,
    pub valid_to: NaiveDateTime,
}
