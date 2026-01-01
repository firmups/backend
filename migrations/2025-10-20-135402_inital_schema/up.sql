-- Device Types
CREATE TABLE device_type (
    id SERIAL PRIMARY KEY,
    name VARCHAR(100) NOT NULL
);
-- Device Type Parameters
CREATE TYPE parameter_type AS ENUM ('STRING', 'INTEGER', 'BOOLEAN', 'FLOAT', 'BINARY');

CREATE TABLE device_type_parameter (
    id SERIAL PRIMARY KEY,
    device_type INT NOT NULL,
    key VARCHAR(100) NOT NULL,
    type parameter_type NOT NULL,
    default_value BYTEA,
    FOREIGN KEY (device_type) REFERENCES device_type(id) ON DELETE RESTRICT
);

-- Firmware
CREATE TABLE firmware (
    id SERIAL PRIMARY KEY,
    name VARCHAR(100) NOT NULL,
    version VARCHAR(100) NOT NULL,
    file_id VARCHAR(36) NOT NULL,
    size BIGINT NOT NULL,
    sha256 VARCHAR(64) NOT NULL,
    CONSTRAINT device_type_firmware_unique UNIQUE (name, version)
);

-- Device Type Firmware (many-to-many)
CREATE TABLE device_type_firmware (
    id SERIAL PRIMARY KEY,
    device_type INT NOT NULL,
    firmware INT NOT NULL,
    FOREIGN KEY (device_type) REFERENCES device_type(id) ON DELETE CASCADE,
    FOREIGN KEY (firmware) REFERENCES firmware(id) ON DELETE CASCADE,
    CONSTRAINT device_type_firmware_unique_pair UNIQUE (device_type, firmware)
);

-- Devices
CREATE TYPE device_status AS ENUM ('ACTIVE', 'INACTIVE', 'MAINTENANCE');

CREATE TABLE device (
    id SERIAL PRIMARY KEY,
    name VARCHAR(100) NOT NULL,
    type INT NOT NULL,
    firmware INT,
    desired_firmware INT NOT NULL,
    status device_status NOT NULL,
    CONSTRAINT fk_device_type FOREIGN KEY (type) REFERENCES device_type(id) ON DELETE RESTRICT,
    CONSTRAINT fk_firmware FOREIGN KEY (firmware) REFERENCES firmware(id) ON DELETE RESTRICT,
    CONSTRAINT fk_desired_firmware FOREIGN KEY (desired_firmware) REFERENCES firmware(id) ON DELETE RESTRICT,
    CONSTRAINT fk_device_type_current
        FOREIGN KEY (type, firmware)
        REFERENCES device_type_firmware (device_type, firmware)
        ON UPDATE CASCADE ON DELETE RESTRICT,
    CONSTRAINT fk_device_type_desired
        FOREIGN KEY (type, desired_firmware)
        REFERENCES device_type_firmware (device_type, firmware)
        ON UPDATE CASCADE ON DELETE RESTRICT
);

-- -- Device Errors
-- CREATE TABLE device_error (
--     id             SERIAL PRIMARY KEY,
--     device_id      BIGINT NOT NULL REFERENCES device(id) ON DELETE CASCADE,
--     error_code_id  BIGINT NOT NULL REFERENCES error_code(id) ON DELETE RESTRICT,
--     occurred_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
--     cleared_at     TIMESTAMPTZ,
--     details        JSONB,
-- );

-- CREATE TYPE error_severity AS ENUM ('CRITICAL', 'MAJOR', 'MINOR');

-- CREATE TABLE error_code (
--     id                SERIAL PRIMARY KEY,
--     code              VARCHAR(64) NOT NULL,
--     title             VARCHAR(200) NOT NULL,
--     description       TEXT,
--     severity          error_severity NOT NULL,
--     device_type       INT REFERENCES device_type(id) ON DELETE CASCADE, -- NULL for system codes
--     CONSTRAINT uq_error_code_namespace UNIQUE (device_type, code),
-- );

-- Device Parameters
CREATE TABLE device_parameter (
    id SERIAL PRIMARY KEY,
    device INT NOT NULL,
    key VARCHAR(100) NOT NULL,
    type parameter_type NOT NULL,
    value BYTEA,
    FOREIGN KEY (device) REFERENCES device(id) ON DELETE RESTRICT
);

-- Device Keys
CREATE TYPE key_type AS ENUM ('LIGHTWEIGHT', 'TLS');
CREATE TYPE key_status AS ENUM ('ACTIVE', 'NEXT', 'EXPIRED');

CREATE TABLE device_key (
    id SERIAL PRIMARY KEY,
    device INT NOT NULL,
    key_type key_type NOT NULL,
    status key_status NOT NULL,
    FOREIGN KEY (device) REFERENCES device(id) ON DELETE CASCADE
);

-- Lightweight Key Details
CREATE TYPE crypto_algorithm AS ENUM ('AES-GCM128', 'ASCON-AEAD128');

CREATE TABLE lightweight_key_details (
    id SERIAL PRIMARY KEY,
    device_key INT NOT NULL,
    algorithm crypto_algorithm NOT NULL,
    key BYTEA NOT NULL,
    FOREIGN KEY (device_key) REFERENCES device_key(id) ON DELETE CASCADE
);

-- TLS Key Details
CREATE TABLE tls_key_details (
    id SERIAL PRIMARY KEY,
    device_key INT NOT NULL,
    valid_from TIMESTAMP NOT NULL,
    valid_to TIMESTAMP NOT NULL,
    FOREIGN KEY (device_key) REFERENCES device_key(id) ON DELETE CASCADE
);
