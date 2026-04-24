-- Device Types
CREATE TABLE device_type (
    id SERIAL PRIMARY KEY,
    name VARCHAR(100) NOT NULL
);
-- Device Type Parameters
CREATE TYPE parameter_type AS ENUM ('string', 'integer', 'boolean', 'float', 'binary');

CREATE TABLE device_type_parameter (
    id SERIAL PRIMARY KEY,
    device_type INT NOT NULL,
    key VARCHAR(100) NOT NULL,
    type parameter_type NOT NULL,
    default_value BYTEA,
    FOREIGN KEY (device_type) REFERENCES device_type(id) ON DELETE RESTRICT,
    CONSTRAINT device_type_parameter_unique_key UNIQUE (device_type, key)
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
CREATE TYPE device_status AS ENUM ('active', 'inactive', 'maintenance');

CREATE TABLE device (
    id SERIAL PRIMARY KEY,
    name VARCHAR(100) NOT NULL,
    type INT NOT NULL,
    firmware INT,
    desired_firmware INT NOT NULL,
    status device_status NOT NULL,
    gateway_id INT,
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
        ON UPDATE CASCADE ON DELETE RESTRICT,
    CONSTRAINT fk_gateway FOREIGN KEY (gateway_id) REFERENCES device(id) ON DELETE SET NULL
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
    device_type_parameter INT NOT NULL,
    value BYTEA,
    FOREIGN KEY (device) REFERENCES device(id) ON DELETE CASCADE,
    FOREIGN KEY (device_type_parameter) REFERENCES device_type_parameter(id) ON DELETE RESTRICT,
    CONSTRAINT device_parameter_unique UNIQUE (device, device_type_parameter)
);

-- Device Keys
CREATE TYPE key_type AS ENUM ('lightweight', 'tls');
CREATE TYPE key_status AS ENUM ('active', 'next', 'expired');

CREATE TABLE device_key (
    id SERIAL PRIMARY KEY,
    device INT NOT NULL,
    key_type key_type NOT NULL,
    status key_status NOT NULL,
    FOREIGN KEY (device) REFERENCES device(id) ON DELETE CASCADE
);

-- Lightweight Key Details
CREATE TYPE crypto_algorithm AS ENUM ('aes_gcm128', 'ascon_aead128');

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
