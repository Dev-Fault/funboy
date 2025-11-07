use std::{collections::HashSet, sync::Arc};

use sqlx::{Error, FromRow, PgPool, Pool, Postgres, Transaction};
use strum::IntoEnumIterator;

use crate::template_substitutor::{TemplateDelimiter, TemplateSubstitutor};
pub const DEBUG_DB_URL: &str = "postgres://funboy:funboy@localhost/funboy_db";

pub type KeySize = i64;

#[derive(Debug, FromRow, Clone)]
pub struct Template {
    pub id: KeySize,
    pub name: String,
}

#[derive(Debug, FromRow, Clone)]
pub struct Substitute {
    pub id: KeySize,
    pub name: String,
    pub template_id: KeySize,
}

#[derive(Debug, Copy, Clone)]
pub enum SortOrder {
    Ascending,
    Descending,
}

impl SortOrder {
    pub fn as_sql(&self) -> &str {
        match self {
            SortOrder::Ascending => "ASC",
            SortOrder::Descending => "DESC",
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub enum Limit {
    Count(KeySize),
    None,
}

impl Limit {
    pub fn as_sql(&self) -> String {
        match self {
            Limit::Count(n) => format!("{}", n),
            Limit::None => "ALL".to_string(),
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub enum OrderBy {
    Id(SortOrder),
    Name(SortOrder),
    NameIgnoreCase(SortOrder),
    Random,
    Default,
}

impl OrderBy {
    pub fn as_sql(&self, alias: Option<&str>) -> String {
        match alias {
            Some(alias) => match self {
                OrderBy::Id(sort_order) => format!("{}.id {}", alias, sort_order.as_sql()),
                OrderBy::Name(sort_order) => format!("{}.name {}", alias, sort_order.as_sql()),
                OrderBy::NameIgnoreCase(sort_order) => {
                    format!("LOWER({}.name) {}", alias, sort_order.as_sql())
                }
                OrderBy::Random => format!("RANDOM()"),
                OrderBy::Default => format!("{}.id ASC", alias),
            },
            None => match self {
                OrderBy::Id(sort_order) => format!("id {}", sort_order.as_sql()),
                OrderBy::Name(sort_order) => format!("name {}", sort_order.as_sql()),
                OrderBy::NameIgnoreCase(sort_order) => {
                    format!("LOWER(name) {}", sort_order.as_sql())
                }
                OrderBy::Random => format!("RANDOM()"),
                OrderBy::Default => format!("id ASC"),
            },
        }
    }
}

pub struct SubstituteReceipt {
    pub updated: Vec<Substitute>,
    pub ignored: Vec<String>,
}

impl SubstituteReceipt {
    pub fn new() -> Self {
        Self {
            updated: Vec::new(),
            ignored: Vec::new(),
        }
    }

    pub fn updated_to_string(&self) -> String {
        self.updated
            .iter()
            .map(|sub| {
                if sub.name.contains(char::is_whitespace) {
                    format!("{}{}{}", '\"', sub.name, '\"')
                } else {
                    sub.name.clone()
                }
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    pub fn ignored_to_string(&self) -> String {
        self.ignored
            .iter()
            .map(|sub| {
                let sub = sub.to_string();
                if sub.contains(char::is_whitespace) {
                    format!("{}{}{}", '\"', sub, '\"')
                } else {
                    sub
                }
            })
            .collect::<Vec<_>>()
            .join(", ")
    }
}

pub struct TemplateReceipt {
    pub updated: Vec<Template>,
    pub ignored: Vec<String>,
}

impl TemplateReceipt {
    pub fn new() -> Self {
        Self {
            updated: Vec::new(),
            ignored: Vec::new(),
        }
    }

    pub fn updated_to_string(&self) -> String {
        self.updated
            .iter()
            .map(|template| template.name.clone())
            .collect::<Vec<_>>()
            .join(", ")
    }

    pub fn ignored_to_string(&self) -> String {
        self.ignored
            .iter()
            .map(|template| template.clone())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

#[derive(Debug)]
pub struct TemplateDatabase {
    pool: Arc<Pool<Postgres>>,
}

impl TemplateDatabase {
    /// Creates a wrapper around pool to handle Template and Substitute queries
    pub fn new(pool: Arc<PgPool>) -> Self {
        TemplateDatabase { pool }
    }

    pub async fn migrate(pool: &PgPool) -> Result<(), sqlx::Error> {
        sqlx::migrate!("./migrations").run(pool).await?;
        Ok(())
    }

    pub async fn create_template(&self, name: &str) -> Result<Option<Template>, Error> {
        let template = sqlx::query_as::<_, Template>(
            "
                    INSERT INTO templates (name) VALUES ($1)
                    ON CONFLICT (name) DO NOTHING
                    RETURNING *
                ",
        )
        .bind(name)
        .fetch_optional(self.pool.as_ref())
        .await?;

        Ok(template)
    }

    async fn update_template_references_in_substitutes(
        &self,
        mut tx: Transaction<'static, Postgres>,
        old_name: &str,
        new_name: &str,
    ) -> Result<Transaction<'static, Postgres>, Error> {
        for delimiter in TemplateDelimiter::iter() {
            // Fetch substitutes that might contain old template
            let substitutes =
                sqlx::query_as::<_, Substitute>("SELECT * FROM substitutes WHERE name LIKE $1")
                    .bind(format!("%{}{}%", delimiter.to_char(), old_name))
                    .fetch_all(&mut *tx)
                    .await?;

            let substitutor = TemplateSubstitutor::new(delimiter);

            // Replace references to old template with new template
            for sub in substitutes {
                let new_sub_name = substitutor
                    .rename_template(&sub.name, old_name, new_name)
                    .await;

                // Avoid useless updates
                if sub.name != new_sub_name {
                    sqlx::query_as::<_, Substitute>(
                        "UPDATE substitutes SET name = $1 WHERE id = $2 RETURNING *",
                    )
                    .bind(&new_sub_name)
                    .bind(sub.id)
                    .fetch_one(&mut *tx)
                    .await?;
                }
            }
        }

        Ok(tx)
    }

    pub async fn update_template_by_id(
        &self,
        id: KeySize,
        new_name: &str,
    ) -> Result<Option<Template>, Error> {
        let mut tx = self.pool.begin().await?;

        // Check if template actually exists
        let old_template = self.read_template_by_id(id).await?;
        let old_template = match old_template {
            Some(old_template) => old_template,
            None => {
                return Ok(None);
            }
        };

        // Rename template
        let template = sqlx::query_as::<_, Template>(
            "UPDATE templates SET name = $1 WHERE name = $2 RETURNING *",
        )
        .bind(new_name)
        .bind(&old_template.name)
        .fetch_optional(&mut *tx)
        .await?;

        let tx = self
            .update_template_references_in_substitutes(tx, &old_template.name, new_name)
            .await?;

        tx.commit().await?;

        Ok(template)
    }

    pub async fn update_template_by_name(
        &self,
        old_name: &str,
        new_name: &str,
    ) -> Result<Option<Template>, Error> {
        let mut tx = self.pool.begin().await?;

        // Check if template actually exists
        self.read_template_by_name(old_name).await?;

        // Rename template
        let template = sqlx::query_as::<_, Template>(
            "UPDATE templates SET name = $1 WHERE name = $2 RETURNING *",
        )
        .bind(new_name)
        .bind(old_name)
        .fetch_optional(&mut *tx)
        .await?;

        let tx = self
            .update_template_references_in_substitutes(tx, old_name, new_name)
            .await?;

        tx.commit().await?;

        Ok(template)
    }

    pub async fn read_template_by_name(
        &self,
        template_name: &str,
    ) -> Result<Option<Template>, Error> {
        let template = sqlx::query_as::<_, Template>("SELECT * FROM templates WHERE name = $1")
            .bind(template_name)
            .fetch_optional(self.pool.as_ref())
            .await?;

        Ok(template)
    }

    pub async fn read_template_by_id(&self, id: KeySize) -> Result<Option<Template>, Error> {
        let template = sqlx::query_as::<_, Template>("SELECT * FROM templates WHERE id = $1")
            .bind(id)
            .fetch_optional(self.pool.as_ref())
            .await?;

        Ok(template)
    }

    pub async fn read_templates(
        &self,
        search_term: Option<&str>,
        order_by: OrderBy,
        limit: Limit,
    ) -> Result<Vec<Template>, Error> {
        let search_term = match search_term {
            Some(search_term) => format!("%{}%", search_term),
            None => "%".to_string(),
        };

        let templates = sqlx::query_as::<_, Template>(&format!(
            "SELECT * FROM templates WHERE name LIKE $1 ORDER BY {} LIMIT {}",
            order_by.as_sql(None),
            limit.as_sql(),
        ))
        .bind(search_term)
        .fetch_all(self.pool.as_ref())
        .await?;

        Ok(templates)
    }

    pub async fn delete_template_by_id(&self, id: KeySize) -> Result<Option<Template>, Error> {
        let template =
            sqlx::query_as::<_, Template>("DELETE FROM templates WHERE id = $1 RETURNING *")
                .bind(id)
                .fetch_optional(self.pool.as_ref())
                .await?;

        Ok(template)
    }

    pub async fn delete_template_by_name(&self, name: &str) -> Result<Option<Template>, Error> {
        let template =
            sqlx::query_as::<_, Template>("DELETE FROM templates WHERE name = $1 RETURNING *")
                .bind(name)
                .fetch_optional(self.pool.as_ref())
                .await?;

        Ok(template)
    }

    pub async fn delete_templates_by_name(&self, names: &[&str]) -> Result<TemplateReceipt, Error> {
        let mut template_receipt = TemplateReceipt::new();
        template_receipt.updated =
            sqlx::query_as::<_, Template>("DELETE FROM templates WHERE name = ANY($1) RETURNING *")
                .bind(names)
                .fetch_all(self.pool.as_ref())
                .await?;

        let deleted: HashSet<&String> = template_receipt.updated.iter().map(|t| &t.name).collect();

        template_receipt.ignored = names
            .iter()
            .map(|t| t.to_string())
            .filter(|t| !deleted.contains(&t))
            .collect::<Vec<String>>();

        Ok(template_receipt)
    }

    async fn read_or_create_template(&self, template_name: &str) -> Result<Template, Error> {
        let template = sqlx::query_as::<_, Template>(
            "INSERT INTO templates (name) VALUES ($1)
             ON CONFLICT (name) DO UPDATE SET name = EXCLUDED.name
             RETURNING *",
        )
        .bind(template_name)
        .fetch_one(self.pool.as_ref())
        .await?;
        Ok(template)
    }

    pub async fn create_substitute(
        &self,
        template_name: &str,
        substitute_name: &str,
    ) -> Result<Option<Substitute>, Error> {
        let template = self.read_or_create_template(template_name).await?;

        let substitute = sqlx::query_as::<_, Substitute>(
            "INSERT INTO substitutes (name, template_id) VALUES ($1, $2) RETURNING *",
        )
        .bind(substitute_name)
        .bind(template.id)
        .fetch_optional(self.pool.as_ref())
        .await?;

        Ok(substitute)
    }

    pub async fn create_substitutes<'a>(
        &self,
        template_name: &str,
        substitute_names: &[&'a str],
    ) -> Result<SubstituteReceipt, Error> {
        let mut tx = self.pool.as_ref().begin().await?;
        let mut sub_record = SubstituteReceipt::new();

        let template = self.read_or_create_template(template_name).await?;

        for substitute_name in substitute_names {
            let substitute = sqlx::query_as::<_, Substitute>(
                "
                    INSERT INTO substitutes (name, template_id) VALUES ($1, $2)
                    ON CONFLICT (name, template_id) DO NOTHING
                    RETURNING *
                ",
            )
            .bind(substitute_name)
            .bind(template.id)
            .fetch_optional(&mut *tx)
            .await?;

            match substitute {
                Some(sub) => sub_record.updated.push(sub),
                None => sub_record.ignored.push(substitute_name.to_string()),
            }
        }

        tx.commit().await?;
        Ok(sub_record)
    }

    pub async fn copy_substitutes_from_template_to_template<'a>(
        &self,
        from_template: &str,
        to_template: &str,
    ) -> Result<Vec<Substitute>, Error> {
        let copied_subs = sqlx::query_as::<_, Substitute>(
            "
                INSERT INTO substitutes (name, template_id)
                SELECT s.name, t_dest.id
                FROM substitutes s
                JOIN templates t_source ON s.template_id = t_source.id
                JOIN templates t_dest ON t_dest.name = $1
                WHERE t_source.name = $2
                ON CONFLICT (name, template_id) DO NOTHING
                RETURNING *
            ",
        )
        .bind(to_template)
        .bind(from_template)
        .fetch_all(self.pool.as_ref())
        .await?;

        Ok(copied_subs)
    }

    pub async fn read_substitutes_from_template(
        &self,
        template_name: &str,
        search_term: Option<&str>,
        order_by: OrderBy,
        limit: Limit,
    ) -> Result<Vec<Substitute>, Error> {
        let search_term = match search_term {
            Some(search_term) => format!("%{}%", search_term),
            None => "%".to_string(),
        };

        let substitutes = sqlx::query_as::<_, Substitute>(&format!(
            "
                 SELECT s.*
                 FROM substitutes s
                 JOIN templates t ON s.template_id = t.id
                 WHERE t.name = $1
                 AND s.name LIKE $2
                 ORDER BY {}
                 LIMIT {}
             ",
            order_by.as_sql(Some("s")),
            limit.as_sql(),
        ))
        .bind(template_name)
        .bind(search_term)
        .fetch_all(self.pool.as_ref())
        .await?;

        Ok(substitutes)
    }

    pub async fn read_substitute_from_template_by_name(
        &self,
        template_name: &str,
        substitute_name: &str,
    ) -> Result<Option<Substitute>, Error> {
        let substitute = sqlx::query_as::<_, Substitute>(&format!(
            "
                 SELECT s.*
                 FROM substitutes s
                 JOIN templates t ON s.template_id = t.id
                 WHERE t.name = $1
                 AND s.name = $2
             ",
        ))
        .bind(template_name)
        .bind(substitute_name)
        .fetch_optional(self.pool.as_ref())
        .await?;

        Ok(substitute)
    }

    pub async fn read_substitute_by_id(
        &self,
        substitute_id: KeySize,
    ) -> Result<Option<Substitute>, Error> {
        let substitute = sqlx::query_as::<_, Substitute>("SELECT * FROM substitutes WHERE id = $1")
            .bind(substitute_id)
            .fetch_optional(self.pool.as_ref())
            .await?;

        Ok(substitute)
    }

    pub async fn update_substitute_by_id(
        &self,
        id: KeySize,
        new_name: &str,
    ) -> Result<Option<Substitute>, Error> {
        let substitute = sqlx::query_as::<_, Substitute>(
            "UPDATE substitutes SET name = $1 WHERE id = $2 RETURNING *",
        )
        .bind(new_name)
        .bind(id)
        .fetch_optional(self.pool.as_ref())
        .await?;

        Ok(substitute)
    }

    pub async fn update_substitute_by_name(
        &self,
        template_name: &str,
        old_name: &str,
        new_name: &str,
    ) -> Result<Option<Substitute>, Error> {
        let substitute = sqlx::query_as::<_, Substitute>(
            "
                UPDATE substitutes s
                SET name = $1
                FROM templates t
                WHERE s.template_id = t.id
                AND t.name = $2
                AND s.name = $3
                RETURNING s.*
            ",
        )
        .bind(new_name)
        .bind(template_name)
        .bind(old_name)
        .fetch_optional(self.pool.as_ref())
        .await?;

        Ok(substitute)
    }

    pub async fn delete_substitute_by_id(&self, id: KeySize) -> Result<Option<Substitute>, Error> {
        let deleted_sub =
            sqlx::query_as::<_, Substitute>("DELETE FROM substitutes WHERE id = $1 RETURNING *")
                .bind(id)
                .fetch_optional(self.pool.as_ref())
                .await?;

        Ok(deleted_sub)
    }

    pub async fn delete_substitutes_by_id(
        &self,
        ids: &[KeySize],
    ) -> Result<SubstituteReceipt, Error> {
        let mut sub_record = SubstituteReceipt::new();
        sub_record.updated = sqlx::query_as::<_, Substitute>(
            "DELETE FROM substitutes WHERE id = ANY($1) RETURNING *",
        )
        .bind(ids)
        .fetch_all(self.pool.as_ref())
        .await?;

        let deleted: HashSet<String> = sub_record
            .updated
            .iter()
            .map(|s| s.id.to_string())
            .collect();

        sub_record.ignored = ids
            .iter()
            .map(|s| s.to_string())
            .filter(|sub| !deleted.contains(sub))
            .collect::<Vec<String>>();

        Ok(sub_record)
    }

    pub async fn delete_substitute_by_name(
        &self,
        template_name: &str,
        substitute_name: &str,
    ) -> Result<Option<Substitute>, Error> {
        let deleted_sub = sqlx::query_as::<_, Substitute>(
            "
                 DELETE FROM substitutes s
                 USING templates t        
                 WHERE s.template_id = t.id
                 AND t.name = $1
                 AND s.name = $2
                 RETURNING s.*
            ",
        )
        .bind(template_name)
        .bind(substitute_name)
        .fetch_optional(self.pool.as_ref())
        .await?;

        Ok(deleted_sub)
    }

    pub async fn delete_substitutes_by_name<'a>(
        &self,
        template_name: &str,
        substitute_names: &[&'a str],
    ) -> Result<SubstituteReceipt, Error> {
        let mut sub_record = SubstituteReceipt::new();
        sub_record.updated = sqlx::query_as::<_, Substitute>(
            "
                 DELETE FROM substitutes s
                 USING templates t        
                 WHERE s.template_id = t.id
                 AND t.name = $1
                 AND s.name = ANY($2)
                 RETURNING s.*
            ",
        )
        .bind(template_name)
        .bind(substitute_names)
        .fetch_all(self.pool.as_ref())
        .await?;

        let deleted: HashSet<&String> = sub_record.updated.iter().map(|s| &s.name).collect();

        sub_record.ignored = substitute_names
            .iter()
            .map(|s| s.to_string())
            .filter(|sub| !deleted.contains(&sub))
            .collect::<Vec<String>>();

        Ok(sub_record)
    }
}

#[cfg(test)]
pub mod test {
    use crate::template_database::*;

    /// Creates a connection with the debug database used for testing
    pub async fn create_debug_db(pool: PgPool) -> Result<TemplateDatabase, sqlx::Error> {
        TemplateDatabase::migrate(&pool).await?;
        let debug_db = TemplateDatabase::new(Arc::new(pool));

        sqlx::query("ALTER SEQUENCE templates_id_seq RESTART WITH 1")
            .execute(debug_db.pool.as_ref())
            .await?;

        sqlx::query("ALTER SEQUENCE substitutes_id_seq RESTART WITH 1")
            .execute(debug_db.pool.as_ref())
            .await?;

        sqlx::query("TRUNCATE TABLE templates CASCADE")
            .execute(debug_db.pool.as_ref())
            .await?;

        Ok(debug_db)
    }

    #[tokio::test]
    async fn connect_to_database() {
        let pool = PgPool::connect(DEBUG_DB_URL).await.unwrap();
        let db = create_debug_db(pool).await.unwrap();
        dbg!(db);
    }

    #[tokio::test]
    async fn crud_template_by_id() {
        let pool = PgPool::connect(DEBUG_DB_URL).await.unwrap();
        let db = create_debug_db(pool).await.unwrap();
        let noun = db.create_template("noun").await.unwrap().unwrap();
        let verb = db.create_template("verb").await.unwrap().unwrap();
        let adj = db.create_template("adj").await.unwrap().unwrap();
        dbg!(
            db.read_templates(None, OrderBy::Default, Limit::None)
                .await
                .unwrap()
        );
        assert!(
            db.read_templates(None, OrderBy::Default, Limit::None)
                .await
                .unwrap()
                .len()
                == 3
        );
        let sustantivo = db
            .update_template_by_id(noun.id, "sustantivo")
            .await
            .unwrap()
            .unwrap();
        assert!(sustantivo.name == "sustantivo");
        db.delete_template_by_id(sustantivo.id).await.unwrap();
        db.delete_template_by_id(verb.id).await.unwrap();
        db.delete_template_by_id(adj.id).await.unwrap();
        dbg!(
            db.read_templates(None, OrderBy::Default, Limit::None)
                .await
                .unwrap()
        );
        assert!(
            db.read_templates(None, OrderBy::Default, Limit::None)
                .await
                .unwrap()
                .len()
                == 0
        );
    }

    #[tokio::test]
    async fn crud_template_by_name() {
        let pool = PgPool::connect(DEBUG_DB_URL).await.unwrap();
        let db = create_debug_db(pool).await.unwrap();
        let noun = db.create_template("noun").await.unwrap().unwrap();
        let verb = db.create_template("verb").await.unwrap().unwrap();
        let adj = db.create_template("adj").await.unwrap().unwrap();
        dbg!(
            db.read_templates(None, OrderBy::Default, Limit::None)
                .await
                .unwrap()
        );
        assert!(
            db.read_templates(None, OrderBy::Default, Limit::None)
                .await
                .unwrap()
                .len()
                == 3
        );
        let sustantivo = db
            .update_template_by_name(&noun.name, "sustantivo")
            .await
            .unwrap()
            .unwrap();
        assert!(sustantivo.name == "sustantivo");
        db.delete_template_by_name(&sustantivo.name).await.unwrap();
        db.delete_template_by_name(&verb.name).await.unwrap();
        db.delete_template_by_name(&adj.name).await.unwrap();
        dbg!(
            db.read_templates(None, OrderBy::Default, Limit::None)
                .await
                .unwrap()
        );
        assert!(
            db.read_templates(None, OrderBy::Default, Limit::None)
                .await
                .unwrap()
                .len()
                == 0
        );
    }

    #[tokio::test]
    async fn crud_substitute_by_id() {
        let pool = PgPool::connect(DEBUG_DB_URL).await.unwrap();
        let db = create_debug_db(pool).await.unwrap();
        let noun_template = db.create_template("animal").await.unwrap().unwrap();
        for name in ["cat", "dog", "bat"] {
            let substitute = db.create_substitute("animal", name).await.unwrap().unwrap();
            assert!(substitute.name == name);
        }
        let substitutes = db
            .read_substitutes_from_template("animal", None, OrderBy::Default, Limit::None)
            .await
            .unwrap();
        dbg!(&substitutes);
        assert!(substitutes.len() == 3);
        for substitute in &substitutes {
            let prev_name = substitute.name.clone();
            let substitute = db
                .update_substitute_by_id(substitute.id, &substitute.name.to_uppercase())
                .await
                .unwrap()
                .unwrap();
            assert!(substitute.name == prev_name.to_uppercase());
        }
        for substitute in &substitutes {
            db.delete_substitute_by_id(substitute.id).await.unwrap();
        }
        dbg!(&substitutes);
        dbg!(
            db.read_templates(None, OrderBy::Default, Limit::None)
                .await
                .unwrap()
        );
        assert!(
            db.read_substitutes_from_template("animal", None, OrderBy::Default, Limit::None)
                .await
                .unwrap()
                .len()
                == 0
        );
        db.delete_template_by_name(&noun_template.name)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn crud_substitute_by_name() {
        let pool = PgPool::connect(DEBUG_DB_URL).await.unwrap();
        let db = create_debug_db(pool).await.unwrap();
        let _ = db.create_template("fruit").await.unwrap();
        let banana = db.create_substitute("fruit", "banana").await.unwrap();
        dbg!(&banana);
        let apple = db
            .update_substitute_by_name("fruit", "banana", "apple")
            .await
            .unwrap();
        dbg!(&apple);
        assert!(
            db.read_substitutes_from_template("fruit", None, OrderBy::Default, Limit::None)
                .await
                .unwrap()
                .len()
                == 1
        );
        db.delete_substitute_by_name("fruit", "apple")
            .await
            .unwrap();
        assert!(
            db.read_substitutes_from_template("fruit", None, OrderBy::Default, Limit::None)
                .await
                .unwrap()
                .len()
                == 0
        );
        db.delete_template_by_name("fruit").await.unwrap();
    }

    #[tokio::test]
    async fn ripple_rename_template_by_name() {
        for delim in TemplateDelimiter::iter() {
            let pool = PgPool::connect(DEBUG_DB_URL).await.unwrap();
            let db = create_debug_db(pool).await.unwrap();
            let fruit_template = db.create_template("fruit").await.unwrap().unwrap();
            db.create_template("references_fruit").await.unwrap();
            db.create_substitute(
                "references_fruit",
                &"^fruit fruit ^fruit^^fruit^^fruit deeplyembedded^fruit^template ^fruit_extra"
                    .replace("^", &delim.to_char().to_string()),
            )
            .await
            .unwrap();

            db.update_template_by_name("fruit", "new_fruit")
                .await
                .unwrap();

            let fruit_template = db
                .read_template_by_id(fruit_template.id)
                .await
                .unwrap()
                .unwrap();
            dbg!(&fruit_template);
            assert!(fruit_template.name == "new_fruit");

            let fruit_reference = &db
                .read_substitutes_from_template(
                    "references_fruit",
                    None,
                    OrderBy::Default,
                    Limit::None,
                )
                .await
                .unwrap()[0];

            dbg!(fruit_reference);
            assert!(
                fruit_reference.name
                    == "^new_fruit fruit ^new_fruit^^new_fruit^^new_fruit deeplyembedded^new_fruit^template ^fruit_extra".replace("^", &delim.to_char().to_string())
            );
        }
    }

    #[tokio::test]
    async fn sort_templates() {
        let pool = PgPool::connect(DEBUG_DB_URL).await.unwrap();
        let db = create_debug_db(pool).await.unwrap();
        let templates = [
            "food", "vehicle", "clothes", "number", "adj", "noun", "verb",
        ];

        for template in templates {
            db.create_template(template).await.unwrap();
        }

        let templates_by_name_asc = db
            .read_templates(None, OrderBy::Name(SortOrder::Ascending), Limit::None)
            .await
            .unwrap();

        dbg!(&templates_by_name_asc);
        for (i, template) in templates_by_name_asc.iter().enumerate() {
            match i {
                0 => assert!(template.name == "adj"),
                1 => assert!(template.name == "clothes"),
                2 => assert!(template.name == "food"),
                3 => assert!(template.name == "noun"),
                4 => assert!(template.name == "number"),
                5 => assert!(template.name == "vehicle"),
                6 => assert!(template.name == "verb"),
                _ => panic!("Should only be 7 templates"),
            }
        }

        for template in templates_by_name_asc {
            db.delete_template_by_id(template.id).await.unwrap();
        }
    }

    #[tokio::test]
    async fn delete_substitutes_by_name() {
        let pool = PgPool::connect(DEBUG_DB_URL).await.unwrap();
        let db = create_debug_db(pool).await.unwrap();
        let subs = ["mouse", "keyboard", "monitor", "microphone"];
        for sub in subs {
            db.create_substitute("computer_part", sub).await.unwrap();
        }

        dbg!(
            db.read_substitutes_from_template("computer_part", None, OrderBy::Default, Limit::None)
                .await
                .unwrap()
        );
        assert!(
            db.read_substitutes_from_template("computer_part", None, OrderBy::Default, Limit::None)
                .await
                .unwrap()
                .len()
                == 4
        );
        db.delete_substitutes_by_name("computer_part", &subs)
            .await
            .unwrap();
        assert!(
            db.read_substitutes_from_template("computer_part", None, OrderBy::Default, Limit::None)
                .await
                .unwrap()
                .len()
                == 0
        );
        dbg!(
            db.read_substitutes_from_template("computer_part", None, OrderBy::Default, Limit::None)
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn delete_substitutes_by_id() {
        let pool = PgPool::connect(DEBUG_DB_URL).await.unwrap();
        let db = create_debug_db(pool).await.unwrap();
        let subs = ["mouse", "keyboard", "monitor", "microphone"];
        for sub in subs {
            db.create_substitute("computer_part", sub).await.unwrap();
        }

        let subs = db
            .read_substitutes_from_template("computer_part", None, OrderBy::Default, Limit::None)
            .await
            .unwrap();

        dbg!(&subs);
        assert!(subs.len() == 4);
        let subs: Vec<KeySize> = subs.iter().map(|sub| sub.id).collect();
        db.delete_substitutes_by_id(&subs).await.unwrap();
        assert!(
            db.read_substitutes_from_template("computer_part", None, OrderBy::Default, Limit::None)
                .await
                .unwrap()
                .len()
                == 0
        );
        dbg!(
            db.read_substitutes_from_template("computer_part", None, OrderBy::Default, Limit::None)
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn create_substitutes() {
        let pool = PgPool::connect(DEBUG_DB_URL).await.unwrap();
        let db = create_debug_db(pool).await.unwrap();

        let sub_names = ["a", "b", "c", "d"];

        let subs = db
            .create_substitutes("example", &sub_names)
            .await
            .unwrap()
            .updated;
        assert!(subs.len() == 4);
        dbg!(&subs);
        db.delete_substitutes_by_name("example", &sub_names)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn template_collision() {
        let pool = PgPool::connect(DEBUG_DB_URL).await.unwrap();
        let db = create_debug_db(pool).await.unwrap();

        db.create_template("template_collision").await.unwrap();
        match db.create_template("template_collision").await.unwrap() {
            Some(_) => panic!("Template collision should cause None to be returned"),
            None => {}
        };
        db.delete_template_by_name("template_collision")
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn substitute_collision() {
        let pool = PgPool::connect(DEBUG_DB_URL).await.unwrap();
        let db = create_debug_db(pool).await.unwrap();
        db.create_template("template").await.unwrap();
        db.create_substitute("template", "substitute_collision")
            .await
            .unwrap();
        match db
            .create_substitute("template", "substitute_collision")
            .await
        {
            Ok(_) => panic!("Substitute collision should return error"),
            Err(e) => dbg!(e),
        };
    }

    #[tokio::test]
    async fn read_single_template() {
        let pool = PgPool::connect(DEBUG_DB_URL).await.unwrap();
        let db = create_debug_db(pool).await.unwrap();
        let test = db.create_template("test").await.unwrap().unwrap();
        assert!(db.read_template_by_name("test").await.unwrap().unwrap().id == test.id);
        assert!(db.read_template_by_id(test.id).await.unwrap().unwrap().id == test.id);
    }

    #[tokio::test]
    async fn read_single_substitute() {
        let pool = PgPool::connect(DEBUG_DB_URL).await.unwrap();
        let db = create_debug_db(pool).await.unwrap();
        db.create_template("test").await.unwrap();
        let test_sub = db
            .create_substitute("test", "test_sub")
            .await
            .unwrap()
            .unwrap();
        assert!(
            db.read_substitute_from_template_by_name("test", "test_sub")
                .await
                .unwrap()
                .unwrap()
                .id
                == test_sub.id
        );
        assert!(
            db.read_substitute_by_id(test_sub.id)
                .await
                .unwrap()
                .unwrap()
                .id
                == test_sub.id
        );
    }

    #[tokio::test]
    async fn cascade_on_delete_template() {
        let pool = PgPool::connect(DEBUG_DB_URL).await.unwrap();
        let db = create_debug_db(pool).await.unwrap();
        let test_template = db.create_template("test").await.unwrap().unwrap();
        let test_subs = db
            .create_substitutes("test", &["test1", "test2", "test3"])
            .await
            .unwrap();
        db.delete_template_by_id(test_template.id).await.unwrap();

        assert!(
            db.read_templates(None, OrderBy::Default, Limit::None)
                .await
                .unwrap()
                .len()
                == 0
        );

        for sub in test_subs.updated {
            assert!(db.read_substitute_by_id(sub.id).await.unwrap().is_none());
        }
    }

    #[tokio::test]
    async fn copy_subs_from_one_template_to_another() {
        let pool = PgPool::connect(DEBUG_DB_URL).await.unwrap();
        let db = create_debug_db(pool).await.unwrap();
        let _ = db.create_template("from_template").await.unwrap();
        let _ = db.create_template("to_template").await.unwrap();
        let _ = db
            .create_substitutes("from_template", &["one", "two", "three", "four"])
            .await
            .unwrap();
        db.copy_substitutes_from_template_to_template("from_template", "to_template")
            .await
            .unwrap();

        let subs = db
            .read_substitutes_from_template(
                "from_template",
                None,
                OrderBy::Name(SortOrder::Ascending),
                Limit::None,
            )
            .await
            .unwrap();

        let copied_subs = db
            .read_substitutes_from_template(
                "to_template",
                None,
                OrderBy::Name(SortOrder::Ascending),
                Limit::None,
            )
            .await
            .unwrap();

        println!(
            "ORIGINAL SUBS: \n{:?}\nCOPIED SUBS: \n{:?}\n",
            &subs, &copied_subs
        );
        for i in 0..subs.len() {
            assert!(subs[i].name == copied_subs[i].name);
        }
    }

    #[tokio::test]
    async fn valid_template_names() {
        let pool = PgPool::connect(DEBUG_DB_URL).await.unwrap();
        let db = create_debug_db(pool).await.unwrap();
        assert!(db.create_template("good_name").await.is_ok());
        assert!(db.create_template("Bad_name").await.is_err());
        assert!(db.create_template("gr34t_n4m3").await.is_ok());
        assert!(db.create_template("horrible-name").await.is_err());
        assert!(db.create_template("*horrible-name").await.is_err());
    }

    #[tokio::test]
    async fn delete_receipt_templates() {
        let pool = PgPool::connect(DEBUG_DB_URL).await.unwrap();
        let db = create_debug_db(pool).await.unwrap();
        db.create_template("stuff").await.unwrap();
        db.create_template("stuff2").await.unwrap();
        db.create_template("stuff3").await.unwrap();
        db.create_template("stuff4").await.unwrap();
        db.create_template("stuff5").await.unwrap();
        db.create_template("stuff6").await.unwrap();
        db.create_template("stuff7").await.unwrap();

        db.delete_templates_by_name(&["stuff2", "stuff7", "stuff5"])
            .await
            .unwrap();

        let templates = db
            .read_templates(None, OrderBy::Default, Limit::None)
            .await
            .unwrap();
        let templates: Vec<&str> = templates
            .iter()
            .map(|t| t.name.as_str())
            .collect::<Vec<&str>>();

        dbg!(&templates);
        assert!(templates.contains(&"stuff"));
        assert!(templates.contains(&"stuff3"));
        assert!(templates.contains(&"stuff4"));
        assert!(templates.contains(&"stuff6"));
    }
}
