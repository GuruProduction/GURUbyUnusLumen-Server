-- GURU open server schema, v1.
--
-- STATELESS BY DESIGN. There are no user accounts, no devices, no conversations,
-- no session tables, no IP addresses, no email addresses in this schema.
-- Every table must pass the leak test before existing: if this database leaked
-- in full tomorrow, no private human data would be exposed.
-- All rows are publishable content, publishable metadata, or transient review
-- queue entries that are hard-purged once decided.

CREATE TABLE prompt_sections (
    id            uuid PRIMARY KEY,
    slug          text        NOT NULL UNIQUE,
    category      text        NOT NULL,
    name          text        NOT NULL,
    content       text        NOT NULL,
    delivery_mode text        NOT NULL DEFAULT 'always',
    is_active     boolean     NOT NULL DEFAULT true,
    version       integer     NOT NULL DEFAULT 1,
    created_at    timestamptz NOT NULL DEFAULT now(),
    updated_at    timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE prompt_versions (
    id         uuid PRIMARY KEY,
    prompt_id  uuid        NOT NULL REFERENCES prompt_sections (id),
    version    integer     NOT NULL,
    name       text        NOT NULL,
    content    text        NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (prompt_id, version)
);

CREATE TABLE skills (
    id            uuid PRIMARY KEY,
    slug          text        NOT NULL UNIQUE,
    name          text        NOT NULL,
    description   text        NOT NULL,
    when_to_use   text        NOT NULL DEFAULT '',
    allowed_tools jsonb       NOT NULL DEFAULT '[]',
    content       text        NOT NULL,
    version       integer     NOT NULL DEFAULT 1,
    source        text        NOT NULL DEFAULT 'builtin',   -- builtin | community
    author_label  text,
    price_cents   bigint      NOT NULL DEFAULT 0,            -- marketplace rails, 0 = free
    is_active     boolean     NOT NULL DEFAULT true,
    created_at    timestamptz NOT NULL DEFAULT now(),
    updated_at    timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE tools (
    id                uuid PRIMARY KEY,
    slug              text        NOT NULL UNIQUE,
    name              text        NOT NULL,
    description       text        NOT NULL,
    category          text        NOT NULL,
    parameters        jsonb       NOT NULL DEFAULT '[]',
    permissions       jsonb       NOT NULL DEFAULT '[]',
    execution_location text       NOT NULL DEFAULT 'on-device',
    code              text        NOT NULL DEFAULT '',
    checksum          text        NOT NULL DEFAULT '',        -- sha256 of code
    version           integer     NOT NULL DEFAULT 1,
    source            text        NOT NULL DEFAULT 'builtin',
    author_label      text,
    price_cents       bigint      NOT NULL DEFAULT 0,
    is_active         boolean     NOT NULL DEFAULT true,
    created_at        timestamptz NOT NULL DEFAULT now(),
    updated_at        timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE agents (
    id               uuid PRIMARY KEY,
    slug             text        NOT NULL UNIQUE,
    name             text        NOT NULL,
    role_description text        NOT NULL,
    system_prompt    text        NOT NULL,
    tool_permissions jsonb       NOT NULL DEFAULT '[]',
    wave             integer     NOT NULL DEFAULT 1,
    is_builtin       boolean     NOT NULL DEFAULT false,
    source           text        NOT NULL DEFAULT 'builtin',
    author_label     text,
    price_cents      bigint      NOT NULL DEFAULT 0,
    is_active        boolean     NOT NULL DEFAULT true,
    created_at       timestamptz NOT NULL DEFAULT now(),
    updated_at       timestamptz NOT NULL DEFAULT now()
);

-- The loading space (formerly AdSpace): interactive canvas packs beamed into
-- the app while the model thinks. Identity, not advertising. No advertiser or
-- campaign tables exist and never will in this schema.
CREATE TABLE canvas_packs (
    id               uuid PRIMARY KEY,
    slug             text        NOT NULL UNIQUE,
    name             text        NOT NULL,
    description      text        NOT NULL DEFAULT '',
    box_height_dp    integer     NOT NULL DEFAULT 220,
    max_elements     integer     NOT NULL DEFAULT 8,
    background_colour text,
    version          integer     NOT NULL DEFAULT 1,
    source           text        NOT NULL DEFAULT 'builtin',
    author_label     text,
    price_cents      bigint      NOT NULL DEFAULT 0,
    is_active        boolean     NOT NULL DEFAULT true,
    created_at       timestamptz NOT NULL DEFAULT now(),
    updated_at       timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE canvas_elements (
    id           uuid PRIMARY KEY,
    pack_id      uuid        NOT NULL REFERENCES canvas_packs (id),
    sort_order   integer     NOT NULL DEFAULT 0,
    element_type text        NOT NULL,          -- lottie | image | video | text | minigame
    name         text        NOT NULL DEFAULT '',
    text_content text,
    script       text,                          -- small js payload for interactive elements
    media_path   text,                          -- relative path under CONTENT_DIR, null for text
    media_mime   text,
    media_bytes  bigint,
    checksum     text        NOT NULL DEFAULT '', -- sha256 of media file, empty for text
    created_at   timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX canvas_elements_pack_idx ON canvas_elements (pack_id);

CREATE TABLE config_defaults (
    key        text PRIMARY KEY,
    value      jsonb       NOT NULL,
    updated_at timestamptz NOT NULL DEFAULT now()
);

-- Reviewer tokens: the ONLY authed concept on this server. Opt-in, hashed,
-- scoped to reviewing community submissions and pushing Unus Lumen master
-- content (prompts, config). A token row stores zero personal data.
CREATE TABLE review_tokens (
    id          uuid PRIMARY KEY,
    label       text        NOT NULL,
    token_hash  text        NOT NULL UNIQUE,    -- sha256 of bearer token
    is_active   boolean     NOT NULL DEFAULT true,
    created_at  timestamptz NOT NULL DEFAULT now(),
    last_used_at timestamptz
);

-- Community submissions. Payloads for rejected submissions are hard-deleted
-- from this table and their staged media files purged from disk on decision.
-- No IP, no email, no account — an opt-in pseudonym at most.
CREATE TABLE submissions (
    id           uuid PRIMARY KEY,
    content_type text        NOT NULL,           -- skill | tool | agent | canvas_pack
    slug         text        NOT NULL,
    name         text        NOT NULL,
    payload      jsonb       NOT NULL,
    author_label text,
    status       text        NOT NULL DEFAULT 'pending',  -- pending | approved | rejected
    review_note  text,
    catalog_id   uuid,
    submitted_at timestamptz NOT NULL DEFAULT now(),
    reviewed_at  timestamptz
);
CREATE INDEX submissions_status_idx ON submissions (status, submitted_at);