ALTER TABLE substitutes DROP CONSTRAINT substitutes_name_template_id_key;
ALTER TABLE substitutes DROP CONSTRAINT substitutes_name_check;
ALTER TABLE substitutes ADD COLUMN name_hash TEXT GENERATED ALWAYS AS (md5(name)) STORED;
ALTER TABLE substitutes ADD CONSTRAINT substitutes_name_template_id_key UNIQUE (name_hash, template_id);
