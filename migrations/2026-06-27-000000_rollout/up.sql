-- Staged firmware rollout
CREATE TYPE rollout_status AS ENUM ('draft', 'active', 'paused', 'completed', 'cancelled');
CREATE TYPE rollout_stage_status AS ENUM ('pending', 'in_progress', 'completed');
CREATE TYPE prerequisite_operator AS ENUM ('eq', 'ne', 'lt', 'lte', 'gt', 'gte');

CREATE TABLE rollout (
    id SERIAL PRIMARY KEY,
    name VARCHAR(100) NOT NULL,
    device_type INT NOT NULL,
    firmware INT NOT NULL,
    status rollout_status NOT NULL DEFAULT 'draft',
    created_at TIMESTAMP NOT NULL DEFAULT now(),
    updated_at TIMESTAMP NOT NULL DEFAULT now(),
    CONSTRAINT fk_rollout_device_type
        FOREIGN KEY (device_type) REFERENCES device_type(id) ON DELETE RESTRICT,
    CONSTRAINT fk_rollout_firmware
        FOREIGN KEY (firmware) REFERENCES firmware(id) ON DELETE RESTRICT,
    CONSTRAINT fk_rollout_type_firmware
        FOREIGN KEY (device_type, firmware)
        REFERENCES device_type_firmware (device_type, firmware)
        ON UPDATE CASCADE ON DELETE RESTRICT
);

-- Parameter-based prerequisites: a device is eligible only if its effective
-- value (override, else device-type default) satisfies every prerequisite.
CREATE TABLE rollout_prerequisite (
    id SERIAL PRIMARY KEY,
    rollout INT NOT NULL,
    device_type_parameter INT NOT NULL,
    operator prerequisite_operator NOT NULL,
    value BYTEA NOT NULL,
    FOREIGN KEY (rollout) REFERENCES rollout(id) ON DELETE CASCADE,
    FOREIGN KEY (device_type_parameter) REFERENCES device_type_parameter(id) ON DELETE RESTRICT,
    CONSTRAINT rollout_prerequisite_unique UNIQUE (rollout, device_type_parameter, operator)
);

-- Cumulative percentage stages, executed in order.
CREATE TABLE rollout_stage (
    id SERIAL PRIMARY KEY,
    rollout INT NOT NULL,
    stage_order INT NOT NULL,
    target_percent SMALLINT NOT NULL,
    success_threshold_percent SMALLINT NOT NULL DEFAULT 100,
    status rollout_stage_status NOT NULL DEFAULT 'pending',
    started_at TIMESTAMP,
    completed_at TIMESTAMP,
    FOREIGN KEY (rollout) REFERENCES rollout(id) ON DELETE CASCADE,
    CONSTRAINT rollout_stage_unique_order UNIQUE (rollout, stage_order),
    CONSTRAINT rollout_stage_target_pct CHECK (target_percent BETWEEN 1 AND 100),
    CONSTRAINT rollout_stage_threshold_pct CHECK (success_threshold_percent BETWEEN 0 AND 100)
);
