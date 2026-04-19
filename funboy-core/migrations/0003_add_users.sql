CREATE TABLE users (
    id BIGSERIAL PRIMARY KEY,
    platform TEXT NOT NULL,
    platform_user_id TEXT NOT NULL,
    UNIQUE(platform, platform_user_id)
);

CREATE TABLE user_permissions (
    user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    permission TEXT NOT NULL,
    PRIMARY KEY (user_id, permission)
);

CREATE TABLE user_ollama_settings (
    user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE PRIMARY KEY,
    system_prompt TEXT,
    template TEXT,
    output_limit SMALLINT,
    temperature REAL,
    repeat_penalty REAL,
    top_k INTEGER,
    top_p REAL
);
