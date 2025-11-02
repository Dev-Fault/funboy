CREATE TABLE templates (
	id SERIAL PRIMARY KEY,
	name TEXT NOT NULL UNIQUE CHECK (name ~ '^[a-z0-9_]+$')
);

CREATE TABLE IF NOT EXISTS substitutes (
	id SERIAL PRIMARY KEY,
	name TEXT NOT NULL,
	template_id INTEGER NOT NULL REFERENCES templates(id) ON DELETE CASCADE,
	UNIQUE(name, template_id)
);
