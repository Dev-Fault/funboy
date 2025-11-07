CREATE TABLE templates (
	id BIGSERIAL PRIMARY KEY,
	name TEXT NOT NULL UNIQUE CHECK (name ~ '^[a-z0-9_]+$' AND length(name) <= 255)
);

CREATE TABLE IF NOT EXISTS substitutes (
	id BIGSERIAL PRIMARY KEY,
	name TEXT NOT NULL CHECK (length(name) <= 16000),
	template_id BIGINT NOT NULL REFERENCES templates(id) ON DELETE CASCADE,
	UNIQUE(name, template_id)
);
