-- Element-level activation: hide an element without deleting it.
ALTER TABLE canvas_elements ADD COLUMN is_active boolean NOT NULL DEFAULT true;
CREATE INDEX canvas_elements_active_idx ON canvas_elements (pack_id) WHERE is_active;