-- Device Types
CREATE TABLE device_type (
    id SERIAL PRIMARY KEY,
    name VARCHAR(100) NOT NULL
);
-- Device Type Parameters
CREATE TYPE parameter_type AS ENUM ('STRING', 'INTEGER', 'BOOLEAN', 'FLOAT', 'DATE', 'BINARY');

CREATE TABLE device_type_parameter (
    id SERIAL PRIMARY KEY,
    device_type INT NOT NULL,
    key VARCHAR(100) NOT NULL,
    type parameter_type NOT NULL,
    FOREIGN KEY (device_type) REFERENCES device_type(id) ON DELETE RESTRICT
);

-- Firmware
CREATE TABLE firmware (
    id SERIAL PRIMARY KEY,
    version VARCHAR(100) NOT NULL,
    path VARCHAR(255) NOT NULL
);

-- Device Type Firmware (many-to-many)
CREATE TABLE device_type_firmware (
    id SERIAL PRIMARY KEY,
    device_type INT NOT NULL,
    firmware INT NOT NULL,
    FOREIGN KEY (device_type) REFERENCES device_type(id) ON DELETE RESTRICT,
    FOREIGN KEY (firmware) REFERENCES firmware(id) ON DELETE RESTRICT
);

-- Devices
CREATE TYPE device_status AS ENUM ('ACTIVE', 'INACTIVE', 'MAINTENANCE');

CREATE TABLE device (
    id SERIAL PRIMARY KEY,
    type INT NOT NULL,
    firmware INT,
    desired_firmware INT NOT NULL,
    status device_status NOT NULL,
    FOREIGN KEY (type) REFERENCES device_type(id) ON DELETE RESTRICT,
    FOREIGN KEY (firmware) REFERENCES firmware(id) ON DELETE RESTRICT,
    FOREIGN KEY (desired_firmware) REFERENCES firmware(id) ON DELETE RESTRICT
);

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
    key_details_id INT NOT NULL,
    FOREIGN KEY (device) REFERENCES device(id)
);

-- Lightweight Key Details
CREATE TYPE crypto_algorithm AS ENUM ('AES', 'CHACHA20');

CREATE TABLE lightweight_key_details (
    id SERIAL PRIMARY KEY,
    device_key INT NOT NULL,
    algorithm crypto_algorithm NOT NULL,
    key BYTEA NOT NULL,
    FOREIGN KEY (device_key) REFERENCES device_key(id)
);

-- TLS Key Details
CREATE TABLE tls_key_details (
    id SERIAL PRIMARY KEY,
    device_key INT NOT NULL,
    valid_from TIMESTAMP NOT NULL,
    valid_to TIMESTAMP NOT NULL,
    FOREIGN KEY (device_key) REFERENCES device_key(id)
);
