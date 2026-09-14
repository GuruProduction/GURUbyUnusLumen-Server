//! Typed repositories for the GURU open server content catalog and review queue.
//!
//! Read side serves the public API. The review side implements the only
//! authenticated surface: reviewer tokens gate submission review and Unus
//! Lumen master-content publishing. Rejected submissions are hard-purged —
//! payload nulled and staged media wiped from disk — by design.

use crate::models::{
    Agent, CanvasElement, CanvasPack, ConfigDefault, PromptSection, PromptVersion, ReviewToken,
    Skill, Submission, Tool,
};
use serde_json::Value;
use sqlx::{PgPool, Result as SqlxResult};
use tracing::warn;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Content reads (public API backing store)
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct ContentRepository {
    pool: PgPool,
}

impl ContentRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    // -- prompts -------------------------------------------------------------

    pub async fn list_active_prompts(&self) -> SqlxResult<Vec<PromptSection>> {
        sqlx::query_as::<_, PromptSection>(
            r#"
            SELECT id, slug, category, name, content, delivery_mode, trigger_keywords,
                   match_threshold, global_threshold, is_active,
                   version, created_at, updated_at
            FROM prompt_sections
            WHERE is_active
            ORDER BY category, name
            "#,
        )
        .fetch_all(&self.pool)
        .await
    }

    pub async fn get_prompt_by_slug(&self, slug: &str) -> SqlxResult<Option<PromptSection>> {
        sqlx::query_as::<_, PromptSection>(
            r#"
            SELECT id, slug, category, name, content, delivery_mode, trigger_keywords,
                   match_threshold, global_threshold, is_active,
                   version, created_at, updated_at
            FROM prompt_sections
            WHERE slug = $1
            "#,
        )
        .bind(slug)
        .fetch_optional(&self.pool)
        .await
    }

    pub async fn list_prompt_versions(&self, prompt_id: Uuid) -> SqlxResult<Vec<PromptVersion>> {
        sqlx::query_as::<_, PromptVersion>(
            r#"
            SELECT id, prompt_id, version, name, content, created_at
            FROM prompt_versions
            WHERE prompt_id = $1
            ORDER BY version DESC
            "#,
        )
        .bind(prompt_id)
        .fetch_all(&self.pool)
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn upsert_prompt(
        &self,
        id: Uuid,
        slug: &str,
        category: &str,
        name: &str,
        content: &str,
        delivery_mode: &str,
        trigger_keywords: &[String],
        match_threshold: Option<f64>,
    ) -> SqlxResult<PromptSection> {
        let mut tx = self.pool.begin().await?;
        let existing: Option<(Uuid, i32)> = sqlx::query_as(
            "SELECT id, version FROM prompt_sections WHERE slug = $1",
        )
        .bind(slug)
        .fetch_optional(&mut *tx)
        .await?;

        let row: PromptSection = match existing {
            Some((existing_id, _current_version)) => {
                sqlx::query_as::<_, PromptSection>(
                    r#"
                    UPDATE prompt_sections
                    SET name = $2, category = $3, content = $4, delivery_mode = $5,
                        trigger_keywords = $6, match_threshold = $7,
                        version = version + 1, updated_at = now()
                    WHERE id = $1
                    RETURNING id, slug, category, name, content, delivery_mode,
                              trigger_keywords, match_threshold, global_threshold,
                              is_active, version, created_at, updated_at
                    "#,
                )
                .bind(existing_id)
                .bind(name)
                .bind(category)
                .bind(content)
                .bind(delivery_mode)
                .bind(trigger_keywords)
                .bind(match_threshold)
                .fetch_one(&mut *tx)
                .await?
            }
            None => {
                sqlx::query_as::<_, PromptSection>(
                    r#"
                    INSERT INTO prompt_sections (id, slug, category, name, content, delivery_mode,
                                                  trigger_keywords, match_threshold)
                    VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
                    RETURNING id, slug, category, name, content, delivery_mode,
                              trigger_keywords, match_threshold, global_threshold,
                              is_active, version, created_at, updated_at
                    "#,
                )
                .bind(id)
                .bind(slug)
                .bind(category)
                .bind(name)
                .bind(content)
                .bind(delivery_mode)
                .bind(trigger_keywords)
                .bind(match_threshold)
                .fetch_one(&mut *tx)
                .await?
            }
        };

        sqlx::query(
            "INSERT INTO prompt_versions (id, prompt_id, version, name, content) VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(Uuid::new_v4())
        .bind(row.id)
        .bind(row.version)
        .bind(name)
        .bind(content)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(row)
    }

    // -- skills ---------------------------------------------------------------

    pub async fn list_active_skills(&self) -> SqlxResult<Vec<Skill>> {
        sqlx::query_as::<_, Skill>(
            r#"
            SELECT id, slug, name, description, when_to_use, allowed_tools, content,
                   version, source, author_label, price_cents, is_active, created_at, updated_at
            FROM skills
            WHERE is_active
            ORDER BY name
            "#,
        )
        .fetch_all(&self.pool)
        .await
    }

    pub async fn get_skill_by_slug(&self, slug: &str) -> SqlxResult<Option<Skill>> {
        sqlx::query_as::<_, Skill>(
            r#"
            SELECT id, slug, name, description, when_to_use, allowed_tools, content,
                   version, source, author_label, price_cents, is_active, created_at, updated_at
            FROM skills
            WHERE slug = $1 AND is_active
            "#,
        )
        .bind(slug)
        .fetch_optional(&self.pool)
        .await
    }

    // -- tools ----------------------------------------------------------------

    pub async fn list_active_tools(&self) -> SqlxResult<Vec<Tool>> {
        sqlx::query_as::<_, Tool>(
            r#"
            SELECT id, slug, name, description, category, parameters, permissions,
                   execution_location, code, checksum, version, source, author_label,
                   price_cents, is_active, created_at, updated_at
            FROM tools
            WHERE is_active
            ORDER BY category, name
            "#,
        )
        .fetch_all(&self.pool)
        .await
    }

    pub async fn get_tool_by_slug(&self, slug: &str) -> SqlxResult<Option<Tool>> {
        sqlx::query_as::<_, Tool>(
            r#"
            SELECT id, slug, name, description, category, parameters, permissions,
                   execution_location, code, checksum, version, source, author_label,
                   price_cents, is_active, created_at, updated_at
            FROM tools
            WHERE slug = $1 AND is_active
            "#,
        )
        .bind(slug)
        .fetch_optional(&self.pool)
        .await
    }

    // -- agents (mask templates) ----------------------------------------------

    pub async fn list_active_agents(&self) -> SqlxResult<Vec<Agent>> {
        sqlx::query_as::<_, Agent>(
            r#"
            SELECT id, slug, name, role_description, system_prompt, tool_permissions,
                   wave, is_builtin, source, author_label, price_cents, is_active,
                   created_at, updated_at
            FROM agents
            WHERE is_active
            ORDER BY wave, name
            "#,
        )
        .fetch_all(&self.pool)
        .await
    }

    pub async fn get_agent_by_slug(&self, slug: &str) -> SqlxResult<Option<Agent>> {
        sqlx::query_as::<_, Agent>(
            r#"
            SELECT id, slug, name, role_description, system_prompt, tool_permissions,
                   wave, is_builtin, source, author_label, price_cents, is_active,
                   created_at, updated_at
            FROM agents
            WHERE slug = $1 AND is_active
            "#,
        )
        .bind(slug)
        .fetch_optional(&self.pool)
        .await
    }

    // -- canvas packs (the loading space) --------------------------------------

    pub async fn list_active_packs(&self) -> SqlxResult<Vec<CanvasPack>> {
        sqlx::query_as::<_, CanvasPack>(
            r#"
            SELECT id, slug, name, description, box_height_dp, max_elements,
                   background_colour, version, source, author_label, price_cents,
                   is_active, created_at, updated_at
            FROM canvas_packs
            WHERE is_active
            ORDER BY name
            "#,
        )
        .fetch_all(&self.pool)
        .await
    }

    pub async fn get_pack_by_slug(&self, slug: &str) -> SqlxResult<Option<CanvasPack>> {
        sqlx::query_as::<_, CanvasPack>(
            r#"
            SELECT id, slug, name, description, box_height_dp, max_elements,
                   background_colour, version, source, author_label, price_cents,
                   is_active, created_at, updated_at
            FROM canvas_packs
            WHERE slug = $1 AND is_active
            "#,
        )
        .bind(slug)
        .fetch_optional(&self.pool)
        .await
    }

    pub async fn list_pack_elements(&self, pack_id: Uuid) -> SqlxResult<Vec<CanvasElement>> {
        sqlx::query_as::<_, CanvasElement>(
            r#"
            SELECT id, pack_id, sort_order, element_type, name, text_content, script,
                   media_path, media_mime, media_bytes, checksum, is_active, created_at
            FROM canvas_elements
            WHERE pack_id = $1 AND is_active
            ORDER BY sort_order
            "#,
        )
        .bind(pack_id)
        .fetch_all(&self.pool)
        .await
    }

    pub async fn get_element_by_id(&self, element_id: Uuid) -> SqlxResult<Option<CanvasElement>> {
        sqlx::query_as::<_, CanvasElement>(
            r#"
            SELECT id, pack_id, sort_order, element_type, name, text_content, script,
                   media_path, media_mime, media_bytes, checksum, is_active, created_at
            FROM canvas_elements
            WHERE id = $1
            "#,
        )
        .bind(element_id)
        .fetch_optional(&self.pool)
        .await
    }

    // -- config ---------------------------------------------------------------

    pub async fn list_config(&self) -> SqlxResult<Vec<ConfigDefault>> {
        sqlx::query_as::<_, ConfigDefault>(
            "SELECT key, value, updated_at FROM config_defaults ORDER BY key",
        )
        .fetch_all(&self.pool)
        .await
    }

    // -- master edits -----------------------------------------------------------

    pub async fn update_skill(
        &self,
        id: Uuid,
        name: &str,
        description: &str,
        when_to_use: &str,
        allowed_tools: &Value,
        content: &str,
    ) -> SqlxResult<Skill> {
        sqlx::query_as::<_, Skill>(
            r#"
            UPDATE skills
            SET name = $2, description = $3, when_to_use = $4,
                allowed_tools = $5, content = $6, version = version + 1, updated_at = now()
            WHERE id = $1
            RETURNING id, slug, name, description, when_to_use, allowed_tools, content,
                      version, source, author_label, price_cents, is_active, created_at, updated_at
            "#,
        )
        .bind(id)
        .bind(name)
        .bind(description)
        .bind(when_to_use)
        .bind(allowed_tools)
        .bind(content)
        .fetch_one(&self.pool)
        .await
    }

    pub async fn set_skill_active(&self, slug: &str, active: bool) -> SqlxResult<bool> {
        let r = sqlx::query("UPDATE skills SET is_active = $2, updated_at = now() WHERE slug = $1")
            .bind(slug)
            .bind(active)
            .execute(&self.pool)
            .await?;
        Ok(r.rows_affected() > 0)
    }

    pub async fn update_tool(
        &self,
        id: Uuid,
        name: &str,
        description: &str,
        category: &str,
        parameters: &Value,
        permissions: &Value,
        execution_location: &str,
        code: &str,
    ) -> SqlxResult<Tool> {
        let checksum = {
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(code.as_bytes());
            hex::encode(hasher.finalize())
        };
        sqlx::query_as::<_, Tool>(
            r#"
            UPDATE tools
            SET name = $2, description = $3, category = $4, parameters = $5,
                permissions = $6, execution_location = $7, code = $8, checksum = $9,
                version = version + 1, updated_at = now()
            WHERE id = $1
            RETURNING id, slug, name, description, category, parameters, permissions,
                      execution_location, code, checksum, version, source, author_label,
                      price_cents, is_active, created_at, updated_at
            "#,
        )
        .bind(id)
        .bind(name)
        .bind(description)
        .bind(category)
        .bind(parameters)
        .bind(permissions)
        .bind(execution_location)
        .bind(code)
        .bind(&checksum)
        .fetch_one(&self.pool)
        .await
    }

    pub async fn set_tool_active(&self, slug: &str, active: bool) -> SqlxResult<bool> {
        let r = sqlx::query("UPDATE tools SET is_active = $2, updated_at = now() WHERE slug = $1")
            .bind(slug)
            .bind(active)
            .execute(&self.pool)
            .await?;
        Ok(r.rows_affected() > 0)
    }

    pub async fn update_agent(
        &self,
        id: Uuid,
        name: &str,
        role_description: &str,
        system_prompt: &str,
        tool_permissions: &Value,
    ) -> SqlxResult<Agent> {
        sqlx::query_as::<_, Agent>(
            r#"
            UPDATE agents
            SET name = $2, role_description = $3, system_prompt = $4,
                tool_permissions = $5, updated_at = now()
            WHERE id = $1
            RETURNING id, slug, name, role_description, system_prompt, tool_permissions,
                      wave, is_builtin, source, author_label, price_cents, is_active,
                      created_at, updated_at
            "#,
        )
        .bind(id)
        .bind(name)
        .bind(role_description)
        .bind(system_prompt)
        .bind(tool_permissions)
        .fetch_one(&self.pool)
        .await
    }

    pub async fn set_agent_active(&self, slug: &str, active: bool) -> SqlxResult<bool> {
        let r = sqlx::query("UPDATE agents SET is_active = $2, updated_at = now() WHERE slug = $1")
            .bind(slug)
            .bind(active)
            .execute(&self.pool)
            .await?;
        Ok(r.rows_affected() > 0)
    }

    pub async fn set_prompt_active(&self, slug: &str, active: bool) -> SqlxResult<bool> {
        let r = sqlx::query(
            "UPDATE prompt_sections SET is_active = $2, updated_at = now() WHERE slug = $1",
        )
        .bind(slug)
        .bind(active)
        .execute(&self.pool)
        .await?;
        Ok(r.rows_affected() > 0)
    }

    pub async fn update_pack(
        &self,
        id: Uuid,
        name: &str,
        description: &str,
        box_height_dp: i32,
        max_elements: i32,
        background_colour: Option<&str>,
    ) -> SqlxResult<CanvasPack> {
        sqlx::query_as::<_, CanvasPack>(
            r#"
            UPDATE canvas_packs
            SET name = $2, description = $3, box_height_dp = $4, max_elements = $5,
                background_colour = $6, version = version + 1, updated_at = now()
            WHERE id = $1
            RETURNING id, slug, name, description, box_height_dp, max_elements,
                      background_colour, version, source, author_label, price_cents,
                      is_active, created_at, updated_at
            "#,
        )
        .bind(id)
        .bind(name)
        .bind(description)
        .bind(box_height_dp)
        .bind(max_elements)
        .bind(background_colour)
        .fetch_one(&self.pool)
        .await
    }

    pub async fn set_pack_active(&self, slug: &str, active: bool) -> SqlxResult<bool> {
        let r = sqlx::query(
            "UPDATE canvas_packs SET is_active = $2, updated_at = now() WHERE slug = $1",
        )
        .bind(slug)
        .bind(active)
        .execute(&self.pool)
        .await?;
        Ok(r.rows_affected() > 0)
    }

    /// Replace the element list of a pack in one transaction. Elements carry
    /// (sort_order, element_type, name, text_content, media_path). Existing
    /// rows for the pack are deleted first — element identity is per-version.
    /// is_active lives on the element row so hides are reversible.
    pub async fn replace_pack_elements(
        &self,
        pack_id: Uuid,
        elements: Vec<(i32, String, String, Option<String>, Option<String>)>,
    ) -> SqlxResult<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "UPDATE canvas_elements SET is_active = false WHERE pack_id = $1",
        )
        .bind(pack_id)
        .execute(&mut *tx)
        .await?;
        for (sort_order, element_type, name, text_content, media_path) in elements {
            sqlx::query(
                r#"
                INSERT INTO canvas_elements (id, pack_id, sort_order, element_type, name,
                                              text_content, media_path, is_active)
                VALUES ($1, $2, $3, $4, $5, $6, $7, true)
                "#,
            )
            .bind(Uuid::new_v4())
            .bind(pack_id)
            .bind(sort_order)
            .bind(&element_type)
            .bind(&name)
            .bind(text_content)
            .bind(media_path)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn update_pack_element(
        &self,
        id: Uuid,
        sort_order: Option<i32>,
        name: Option<&str>,
        text_content: Option<&str>,
        media_path: Option<&str>,
    ) -> SqlxResult<bool> {
        let r = sqlx::query(
            r#"
            UPDATE canvas_elements
            SET sort_order = coalesce($2, sort_order),
                name = coalesce($3, name),
                text_content = coalesce($4, text_content),
                media_path = coalesce($5, media_path)
            WHERE id = $1
            "#,
        )
        .bind(id)
        .bind(sort_order)
        .bind(name)
        .bind(text_content)
        .bind(media_path)
        .execute(&self.pool)
        .await?;
        Ok(r.rows_affected() > 0)
    }

    pub async fn set_pack_element_active(&self, id: Uuid, active: bool) -> SqlxResult<bool> {
        let r = sqlx::query("UPDATE canvas_elements SET is_active = $2 WHERE id = $1")
            .bind(id)
            .bind(active)
            .execute(&self.pool)
            .await?;
        Ok(r.rows_affected() > 0)
    }

    pub async fn set_config(&self, key: &str, value: &Value) -> SqlxResult<()> {
        sqlx::query(
            r#"
            INSERT INTO config_defaults (key, value, updated_at)
            VALUES ($1, $2, now())
            ON CONFLICT (key) DO UPDATE
            SET value = EXCLUDED.value, updated_at = now()
            "#,
        )
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    // -- hard deletes: rows leave the database, version history included -------

    /// Delete a prompt and its entire version history in one transaction.
    pub async fn delete_prompt(&self, slug: &str) -> SqlxResult<bool> {
        let mut tx = self.pool.begin().await?;
        let _r = sqlx::query("DELETE FROM prompt_versions WHERE prompt_id IN (SELECT id FROM prompt_sections WHERE slug = $1)")
            .bind(slug)
            .execute(&mut *tx)
            .await?;
        let rows = sqlx::query("DELETE FROM prompt_sections WHERE slug = $1")
            .bind(slug)
            .execute(&mut *tx)
            .await?;
        let deleted = rows.rows_affected() > 0;
        tx.commit().await?;
        Ok(deleted)
    }

    pub async fn delete_skill(&self, slug: &str) -> SqlxResult<bool> {
        let r = sqlx::query("DELETE FROM skills WHERE slug = $1")
            .bind(slug)
            .execute(&self.pool)
            .await?;
        Ok(r.rows_affected() > 0)
    }

    pub async fn delete_tool(&self, slug: &str) -> SqlxResult<bool> {
        let r = sqlx::query("DELETE FROM tools WHERE slug = $1")
            .bind(slug)
            .execute(&self.pool)
            .await?;
        Ok(r.rows_affected() > 0)
    }

    pub async fn delete_agent(&self, slug: &str) -> SqlxResult<bool> {
        let r = sqlx::query("DELETE FROM agents WHERE slug = $1")
            .bind(slug)
            .execute(&self.pool)
            .await?;
        Ok(r.rows_affected() > 0)
    }

    /// Delete a pack and all its elements in one transaction.
    pub async fn delete_pack(&self, slug: &str) -> SqlxResult<bool> {
        let mut tx = self.pool.begin().await?;
        let _ = sqlx::query(
            "DELETE FROM canvas_elements WHERE pack_id IN (SELECT id FROM canvas_packs WHERE slug = $1)",
        )
        .bind(slug)
        .execute(&mut *tx)
        .await?;
        let rows = sqlx::query("DELETE FROM canvas_packs WHERE slug = $1")
            .bind(slug)
            .execute(&mut *tx)
            .await?;
        let deleted = rows.rows_affected() > 0;
        tx.commit().await?;
        Ok(deleted)
    }

    pub async fn delete_pack_element(&self, id: Uuid) -> SqlxResult<bool> {
        let r = sqlx::query("DELETE FROM canvas_elements WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(r.rows_affected() > 0)
    }

    pub async fn delete_config(&self, key: &str) -> SqlxResult<bool> {
        let r = sqlx::query("DELETE FROM config_defaults WHERE key = $1")
            .bind(key)
            .execute(&self.pool)
            .await?;
        Ok(r.rows_affected() > 0)
    }

    // -- community catalog inserts (promotion from approved submissions) -------

    pub async fn insert_skill(
        &self,
        id: Uuid,
        slug: &str,
        name: &str,
        description: &str,
        when_to_use: &str,
        allowed_tools: &Value,
        content: &str,
        author_label: Option<&str>,
    ) -> SqlxResult<Skill> {
        sqlx::query_as::<_, Skill>(
            r#"
            INSERT INTO skills (id, slug, name, description, when_to_use, allowed_tools,
                                content, source, author_label)
            VALUES ($1, $2, $3, $4, $5, $6, $7, 'community', $8)
            ON CONFLICT (slug) DO UPDATE
            SET name = EXCLUDED.name, description = EXCLUDED.description,
                when_to_use = EXCLUDED.when_to_use, allowed_tools = EXCLUDED.allowed_tools,
                content = EXCLUDED.content, author_label = EXCLUDED.author_label,
                source = 'community', version = skills.version + 1, updated_at = now()
            RETURNING id, slug, name, description, when_to_use, allowed_tools, content,
                      version, source, author_label, price_cents, is_active, created_at, updated_at
            "#,
        )
        .bind(id)
        .bind(slug)
        .bind(name)
        .bind(description)
        .bind(when_to_use)
        .bind(allowed_tools)
        .bind(content)
        .bind(author_label)
        .fetch_one(&self.pool)
        .await
    }

    pub async fn insert_tool(
        &self,
        id: Uuid,
        slug: &str,
        name: &str,
        description: &str,
        category: &str,
        parameters: &Value,
        permissions: &Value,
        execution_location: &str,
        code: &str,
        author_label: Option<&str>,
    ) -> SqlxResult<Tool> {
        let checksum = {
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(code.as_bytes());
            hex::encode(hasher.finalize())
        };
        sqlx::query_as::<_, Tool>(
            r#"
            INSERT INTO tools (id, slug, name, description, category, parameters,
                               permissions, execution_location, code, checksum,
                               source, author_label)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, 'community', $11)
            ON CONFLICT (slug) DO UPDATE
            SET name = EXCLUDED.name, description = EXCLUDED.description,
                category = EXCLUDED.category, parameters = EXCLUDED.parameters,
                permissions = EXCLUDED.permissions,
                execution_location = EXCLUDED.execution_location,
                code = EXCLUDED.code, checksum = EXCLUDED.checksum,
                author_label = EXCLUDED.author_label, source = 'community',
                version = tools.version + 1, updated_at = now()
            RETURNING id, slug, name, description, category, parameters, permissions,
                      execution_location, code, checksum, version, source, author_label,
                      price_cents, is_active, created_at, updated_at
            "#,
        )
        .bind(id)
        .bind(slug)
        .bind(name)
        .bind(description)
        .bind(category)
        .bind(parameters)
        .bind(permissions)
        .bind(execution_location)
        .bind(code)
        .bind(&checksum)
        .bind(author_label)
        .fetch_one(&self.pool)
        .await
    }

    pub async fn insert_agent(
        &self,
        id: Uuid,
        slug: &str,
        name: &str,
        role_description: &str,
        system_prompt: &str,
        tool_permissions: &Value,
        author_label: Option<&str>,
    ) -> SqlxResult<Agent> {
        sqlx::query_as::<_, Agent>(
            r#"
            INSERT INTO agents (id, slug, name, role_description, system_prompt,
                                 tool_permissions, is_builtin, source, author_label)
            VALUES ($1, $2, $3, $4, $5, $6, false, 'community', $7)
            ON CONFLICT (slug) DO UPDATE
            SET name = EXCLUDED.name, role_description = EXCLUDED.role_description,
                system_prompt = EXCLUDED.system_prompt,
                tool_permissions = EXCLUDED.tool_permissions,
                author_label = EXCLUDED.author_label, source = 'community',
                updated_at = now()
            RETURNING id, slug, name, role_description, system_prompt, tool_permissions,
                      wave, is_builtin, source, author_label, price_cents, is_active,
                      created_at, updated_at
            "#,
        )
        .bind(id)
        .bind(slug)
        .bind(name)
        .bind(role_description)
        .bind(system_prompt)
        .bind(tool_permissions)
        .bind(author_label)
        .fetch_one(&self.pool)
        .await
    }

    pub async fn insert_canvas_pack(
        &self,
        id: Uuid,
        slug: &str,
        name: &str,
        description: &str,
        box_height_dp: i32,
        max_elements: i32,
        background_colour: Option<&str>,
        elements: &[(i32, String, String, Option<String>, Option<String>)],
        author_label: Option<&str>,
    ) -> SqlxResult<CanvasPack> {
        let mut tx = self.pool.begin().await?;
        let pack: CanvasPack = sqlx::query_as::<_, CanvasPack>(
            r#"
            INSERT INTO canvas_packs (id, slug, name, description, box_height_dp,
                                      max_elements, background_colour, source, author_label)
            VALUES ($1, $2, $3, $4, $5, $6, $7, 'community', $8)
            ON CONFLICT (slug) DO UPDATE
            SET name = EXCLUDED.name, description = EXCLUDED.description,
                box_height_dp = EXCLUDED.box_height_dp,
                max_elements = EXCLUDED.max_elements,
                background_colour = EXCLUDED.background_colour,
                author_label = EXCLUDED.author_label, source = 'community',
                version = canvas_packs.version + 1, updated_at = now()
            RETURNING id, slug, name, description, box_height_dp, max_elements,
                      background_colour, version, source, author_label, price_cents,
                      is_active, created_at, updated_at
            "#,
        )
        .bind(id)
        .bind(slug)
        .bind(name)
        .bind(description)
        .bind(box_height_dp)
        .bind(max_elements)
        .bind(background_colour)
        .bind(author_label)
        .fetch_one(&mut *tx)
        .await?;

        // Replace any prior elements belonging to this slug's pack.
        sqlx::query("DELETE FROM canvas_elements WHERE pack_id = $1")
            .bind(pack.id)
            .execute(&mut *tx)
            .await?;
        for (sort_order, element_type, elem_name, text_content, media_path) in elements {
            sqlx::query(
                r#"
                INSERT INTO canvas_elements (id, pack_id, sort_order, element_type, name,
                                              text_content, media_path)
                VALUES ($1, $2, $3, $4, $5, $6, $7)
                "#,
            )
            .bind(Uuid::new_v4())
            .bind(pack.id)
            .bind(sort_order)
            .bind(element_type)
            .bind(elem_name)
            .bind(text_content)
            .bind(media_path)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(pack)
    }

    /// Slug+version pairs for one content table, active rows only. Backs the
    /// top-level manifest. Table names are compile-time constants from the
    /// HTTP layer, never user input.
    pub async fn slug_versions(
        &self,
        table: &'static str,
    ) -> SqlxResult<Vec<(String, i32)>> {
        let sql = match table {
            "prompt_sections" => "SELECT slug, version FROM prompt_sections WHERE is_active ORDER BY slug",
            "skills" => "SELECT slug, version FROM skills WHERE is_active ORDER BY slug",
            "tools" => "SELECT slug, version FROM tools WHERE is_active ORDER BY slug",
            "agents" => "SELECT slug, 1 AS version FROM agents WHERE is_active ORDER BY slug",
            "canvas_packs" => "SELECT slug, version FROM canvas_packs WHERE is_active ORDER BY slug",
            other => unreachable!("no such content table: {other}"),
        };
        sqlx::query_as::<_, (String, i32)>(sql)
            .fetch_all(&self.pool)
            .await
    }

    /// A content hash over every active row of every content type, used as the
    /// top-level manifest ETag. Touch any content, the hash changes.
    pub async fn content_state_hash(&self) -> SqlxResult<String> {
        let row: (String,) = sqlx::query_as(
            r#"
            SELECT coalesce(
                md5(string_agg(
                    row_data, ''
                    ORDER BY row_data
                )),
                'empty'
            ) AS state_hash
            FROM (
                SELECT slug || ':' || version || ':' || updated_at::text AS row_data FROM prompt_sections WHERE is_active
                UNION ALL
                SELECT slug || ':' || version || ':' || updated_at::text FROM skills WHERE is_active
                UNION ALL
                SELECT slug || ':' || version || ':' || updated_at::text FROM tools WHERE is_active
                UNION ALL
                SELECT slug || ':' || version || ':' || updated_at::text FROM agents WHERE is_active
                UNION ALL
                SELECT slug || ':' || version || ':' || updated_at::text FROM canvas_packs WHERE is_active
                UNION ALL
                SELECT key || ':' || updated_at::text FROM config_defaults
            ) all_rows
            "#,
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0)
    }
}

// ---------------------------------------------------------------------------
// Review queue
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct ReviewRepository {
    pool: PgPool,
}

impl ReviewRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    // -- tokens ---------------------------------------------------------------

    pub async fn create_review_token(
        &self,
        label: &str,
        token_hash: &str,
    ) -> SqlxResult<ReviewToken> {
        sqlx::query_as::<_, ReviewToken>(
            r#"
            INSERT INTO review_tokens (id, label, token_hash)
            VALUES ($1, $2, $3)
            RETURNING id, label, token_hash, is_active, created_at, last_used_at
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(label)
        .bind(token_hash)
        .fetch_one(&self.pool)
        .await
    }

    /// Verify a bearer token, returning its row when active.
    pub async fn verify_review_token(&self, token_hash: &str) -> SqlxResult<Option<ReviewToken>> {
        sqlx::query_as::<_, ReviewToken>(
            r#"
            SELECT id, label, token_hash, is_active, created_at, last_used_at
            FROM review_tokens
            WHERE token_hash = $1 AND is_active
            "#,
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await
    }

    pub async fn touch_token(&self, token_id: Uuid) -> SqlxResult<()> {
        sqlx::query("UPDATE review_tokens SET last_used_at = now() WHERE id = $1")
            .bind(token_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // -- submissions ----------------------------------------------------------

    pub async fn create_submission(
        &self,
        content_type: &str,
        slug: &str,
        name: &str,
        payload: &Value,
        author_label: Option<&str>,
    ) -> SqlxResult<Submission> {
        sqlx::query_as::<_, Submission>(
            r#"
            INSERT INTO submissions (id, content_type, slug, name, payload, author_label)
            VALUES ($1, $2, $3, $4, $5, $6)
            RETURNING id, content_type, slug, name, payload, author_label, status,
                      review_note, catalog_id, submitted_at, reviewed_at
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(content_type)
        .bind(slug)
        .bind(name)
        .bind(payload)
        .bind(author_label)
        .fetch_one(&self.pool)
        .await
    }

    pub async fn list_pending_submissions(&self) -> SqlxResult<Vec<Submission>> {
        sqlx::query_as::<_, Submission>(
            r#"
            SELECT id, content_type, slug, name, payload, author_label, status,
                   review_note, catalog_id, submitted_at, reviewed_at
            FROM submissions
            WHERE status = 'pending'
            ORDER BY submitted_at
            "#,
        )
        .fetch_all(&self.pool)
        .await
    }

    pub async fn get_submission(&self, id: Uuid) -> SqlxResult<Option<Submission>> {
        sqlx::query_as::<_, Submission>(
            r#"
            SELECT id, content_type, slug, name, payload, author_label, status,
                   review_note, catalog_id, submitted_at, reviewed_at
            FROM submissions
            WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
    }

    /// Approve a pending submission into the catalog. The caller promotes any
    /// staged media to content storage before calling this; catalog_id links
    /// the catalog row back to its originating submission.
    pub async fn approve_submission(
        &self,
        id: Uuid,
        catalog_id: Uuid,
        note: Option<&str>,
    ) -> SqlxResult<()> {
        sqlx::query(
            r#"
            UPDATE submissions
            SET status = 'approved', catalog_id = $2, review_note = $3,
                payload = 'null'::jsonb, reviewed_at = now()
            WHERE id = $1 AND status = 'pending'
            "#,
        )
        .bind(id)
        .bind(catalog_id)
        .bind(note)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Reject a pending submission. The privacy guarantee: the payload is
    /// nulled in the same statement that marks rejection, and the caller
    /// purges any staged media from disk immediately after this returns.
    pub async fn reject_submission(&self, id: Uuid, note: Option<&str>) -> SqlxResult<()> {
        let result = sqlx::query(
            r#"
            UPDATE submissions
            SET status = 'rejected', review_note = $2,
                payload = 'null'::jsonb, catalog_id = NULL, reviewed_at = now()
            WHERE id = $1 AND status = 'pending'
            "#,
        )
        .bind(id)
        .bind(note)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 0 {
            warn!(submission = %id, "reject called on non-pending submission");
        }
        Ok(())
    }
}