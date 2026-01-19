use chrono::NaiveDateTime;
use diesel::prelude::*;

use diesel_derive_enum::DbEnum;

#[derive(Debug, Clone, Copy, PartialEq, Eq, DbEnum, serde::Serialize, serde::Deserialize)]
#[ExistingTypePath = "crate::db::schema::sql_types::CryptoAlgorithm"]
pub enum CryptoAlgorithm {
    /// Maps to the Postgres enum label 'AES-GCM'
    #[db_rename = "AES-GCM128"]
    #[serde(rename = "AES_GCM128")]
    AesGcm128,

    /// Maps to the Postgres enum label 'ASCON-AEAD128'
    #[db_rename = "ASCON-AEAD128"]
    #[serde(rename = "ASCON_AEAD128")]
    AsconAead128,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, DbEnum, serde::Serialize, serde::Deserialize)]
#[ExistingTypePath = "crate::db::schema::sql_types::DeviceStatus"]
#[DbValueStyle = "snake_case"]
pub enum DeviceStatus {
    #[db_rename = "ACTIVE"]
    ACTIVE = 0,
    #[db_rename = "INACTIVE"]
    INACTIVE = 1,
    #[db_rename = "MAINTENANCE"]
    MAINTENANCE = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, DbEnum, serde::Serialize, serde::Deserialize)]
#[ExistingTypePath = "crate::db::schema::sql_types::KeyStatus"]
#[DbValueStyle = "snake_case"]
pub enum KeyStatus {
    #[db_rename = "ACTIVE"]
    ACTIVE,
    #[db_rename = "NEXT"]
    NEXT,
    #[db_rename = "EXPIRED"]
    EXPIRED,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, DbEnum)]
#[ExistingTypePath = "crate::db::schema::sql_types::KeyType"]
#[DbValueStyle = "snake_case"]
pub enum KeyType {
    #[db_rename = "LIGHTWEIGHT"]
    LIGHTWEIGHT,
    #[db_rename = "TLS"]
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
#[diesel(table_name = crate::db::schema::device)]
pub struct Device {
    pub id: i32,
    pub name: String,
    pub type_: i32,
    pub firmware: Option<i32>,
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

#[derive(Debug, Clone, AsChangeset, serde::Serialize, serde::Deserialize)]
#[diesel(table_name = crate::db::schema::device)]
pub struct UpdateDevice {
    pub name: Option<String>,
    pub type_: Option<i32>,
    pub firmware: Option<i32>,
    pub desired_firmware: Option<i32>,
    pub status: Option<DeviceStatus>,
}

// device_key
#[derive(Debug, Clone, Identifiable, Queryable, Selectable, Associations, AsChangeset)]
#[diesel(table_name = crate::db::schema::device_key)]
#[diesel(belongs_to(Device, foreign_key = device))]
pub struct DeviceKey {
    pub id: i32,
    pub device: i32,
    pub key_type: KeyType,
    pub status: KeyStatus,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = crate::db::schema::device_key)]
#[diesel(belongs_to(Device, foreign_key = device))]
pub struct NewDeviceKey {
    pub device: i32,
    pub key_type: KeyType,
    pub status: KeyStatus,
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

#[derive(Debug, Clone, AsChangeset, serde::Serialize, serde::Deserialize)]
#[diesel(table_name = crate::db::schema::device_type)]
pub struct UpdateDeviceType {
    pub name: Option<String>,
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
    pub device_type: i32,
    pub firmware: i32,
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
#[diesel(table_name = crate::db::schema::lightweight_key_details)]
#[diesel(belongs_to(DeviceKey, foreign_key = device_key))]
pub struct LightweightKeyDetails {
    pub id: i32,
    pub device_key: i32, // FK -> device_key.id
    pub algorithm: CryptoAlgorithm,
    pub key: Vec<u8>,
}

#[derive(Debug, Clone, Insertable, serde::Serialize, serde::Deserialize)]
#[diesel(table_name = crate::db::schema::lightweight_key_details)]
#[diesel(belongs_to(DeviceKey, foreign_key = device_key))]
pub struct NewLightweightKeyDetails {
    pub device_key: i32,
    pub algorithm: CryptoAlgorithm,
    pub key: Vec<u8>,
}

// tls_key_details
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
#[diesel(table_name = crate::db::schema::tls_key_details)]
#[diesel(belongs_to(DeviceKey, foreign_key = device_key))]
pub struct TlsKeyDetails {
    pub id: i32,
    pub device_key: i32, // FK -> device_key.id
    pub valid_from: NaiveDateTime,
    pub valid_to: NaiveDateTime,
}

#[derive(Debug, Clone, Insertable, serde::Serialize, serde::Deserialize)]
#[diesel(table_name = crate::db::schema::tls_key_details)]
#[diesel(belongs_to(DeviceKey, foreign_key = device_key))]
pub struct NewTlsKeyDetails {
    pub device_key: i32,
    pub valid_from: NaiveDateTime,
    pub valid_to: NaiveDateTime,
}
