-- Keyword injection from the legacy server: prompts flagged keyword_injection
-- carry trigger keywords and an optional per-prompt match threshold. The app
-- pulls these lazily so context stays lean.
ALTER TABLE prompt_sections ADD COLUMN trigger_keywords text[] NOT NULL DEFAULT '{}';
ALTER TABLE prompt_sections ADD COLUMN match_threshold double precision;
ALTER TABLE prompt_sections ADD COLUMN global_threshold double precision NOT NULL DEFAULT 0.75;