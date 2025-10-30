use sqlx::{Error, FromRow, Pool, Postgres};

pub type TdbId = i32;

#[derive(Debug)]
pub struct TemplateDatabase {
    url: String,
    pool: Pool<Postgres>,
}

#[derive(Debug, FromRow)]
pub struct Template {
    pub id: TdbId,
    pub name: String,
}

#[derive(Debug, FromRow)]
pub struct Substitute {
    pub id: TdbId,
    pub name: String,
    pub template_id: TdbId,
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
pub enum OrderBy {
    Id(SortOrder),
    Name(SortOrder),
    NameIgnoreCase(SortOrder),
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
                OrderBy::Default => format!("{}.id ASC", alias),
            },
            None => match self {
                OrderBy::Id(sort_order) => format!("id {}", sort_order.as_sql()),
                OrderBy::Name(sort_order) => format!("name {}", sort_order.as_sql()),
                OrderBy::NameIgnoreCase(sort_order) => {
                    format!("LOWER(name) {}", sort_order.as_sql())
                }
                OrderBy::Default => format!("id ASC"),
            },
        }
    }
}

impl TemplateDatabase {
    pub async fn new(url: String) -> Result<Self, sqlx::Error> {
        let pool = sqlx::postgres::PgPool::connect(&url).await?;

        sqlx::migrate!("./migrations").run(&pool).await?;

        Ok(TemplateDatabase { url, pool })
    }

    pub async fn create_template(&self, name: &str) -> Result<Template, Error> {
        let template =
            sqlx::query_as::<_, Template>("INSERT INTO templates (name) VALUES ($1) RETURNING *")
                .bind(name)
                .fetch_one(&self.pool)
                .await?;

        Ok(template)
    }

    pub async fn update_template_by_id(
        &self,
        id: TdbId,
        new_name: &str,
    ) -> Result<Template, Error> {
        let template = sqlx::query_as::<_, Template>(
            "UPDATE templates SET name = $1 WHERE id = $2 RETURNING *",
        )
        .bind(new_name)
        .bind(id)
        .fetch_one(&self.pool)
        .await?;

        Ok(template)
    }

    pub async fn update_template_by_name(
        &self,
        old_name: &str,
        new_name: &str,
    ) -> Result<Template, Error> {
        let template = sqlx::query_as::<_, Template>(
            "UPDATE templates SET name = $1 WHERE name = $2 RETURNING *",
        )
        .bind(new_name)
        .bind(old_name)
        .fetch_one(&self.pool)
        .await?;

        Ok(template)
    }

    pub async fn read_templates(&self, order_by: OrderBy) -> Result<Vec<Template>, Error> {
        let templates = sqlx::query_as::<_, Template>(&format!(
            "SELECT * FROM templates ORDER BY {}",
            order_by.as_sql(None)
        ))
        .fetch_all(&self.pool)
        .await?;

        Ok(templates)
    }

    pub async fn delete_template_by_id(&self, id: TdbId) -> Result<(), Error> {
        sqlx::query("DELETE FROM templates WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    pub async fn delete_template_by_name(&self, name: &str) -> Result<(), Error> {
        sqlx::query("DELETE FROM templates WHERE name = $1")
            .bind(name)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    async fn read_or_create_template(&self, template_name: &str) -> Result<Template, Error> {
        let template = sqlx::query_as::<_, Template>(
            "INSERT INTO templates (name) VALUES ($1)
             ON CONFLICT (name) DO UPDATE SET name = EXCLUDED.name
             RETURNING *",
        )
        .bind(template_name)
        .fetch_one(&self.pool)
        .await?;
        Ok(template)
    }

    pub async fn create_substitute(
        &self,
        template_name: &str,
        substitute_name: &str,
    ) -> Result<Substitute, Error> {
        let template = self.read_or_create_template(template_name).await?;

        let substitute = sqlx::query_as::<_, Substitute>(
            "INSERT INTO substitutes (name, template_id) VALUES ($1, $2) RETURNING *",
        )
        .bind(substitute_name)
        .bind(template.id)
        .fetch_one(&self.pool)
        .await?;

        Ok(substitute)
    }

    pub async fn create_substitutes(
        &self,
        template_name: &str,
        substitute_names: &[&str],
    ) -> Result<Vec<Substitute>, Error> {
        let mut tx = self.pool.begin().await?;

        let mut substitutes = Vec::with_capacity(substitute_names.len());

        let template = self.read_or_create_template(template_name).await?;

        for substitute_name in substitute_names {
            let substitute = sqlx::query_as::<_, Substitute>(
                "INSERT INTO substitutes (name, template_id) VALUES ($1, $2) RETURNING *",
            )
            .bind(substitute_name)
            .bind(template.id)
            .fetch_one(&mut *tx)
            .await?;
            substitutes.push(substitute);
        }

        tx.commit().await?;
        Ok(substitutes)
    }

    pub async fn read_substitutes_from_template(
        &self,
        template_name: &str,
        order_by: OrderBy,
    ) -> Result<Vec<Substitute>, Error> {
        let substitutes = sqlx::query_as::<_, Substitute>(&format!(
            "
                 SELECT s.*
                 FROM SUBSTITUTES s
                 JOIN templates t ON s.template_id = t.id
                 WHERE t.name = $1
                 ORDER BY {}
             ",
            order_by.as_sql("s".into())
        ))
        .bind(template_name)
        .fetch_all(&self.pool)
        .await?;

        Ok(substitutes)
    }

    pub async fn update_substitute_by_id(
        &self,
        id: TdbId,
        new_name: &str,
    ) -> Result<Substitute, Error> {
        let substitute = sqlx::query_as::<_, Substitute>(
            "UPDATE substitutes SET name = $1 WHERE id = $2 RETURNING *",
        )
        .bind(new_name)
        .bind(id)
        .fetch_one(&self.pool)
        .await?;

        Ok(substitute)
    }

    pub async fn update_substitute_by_name(
        &self,
        template_name: &str,
        old_name: &str,
        new_name: &str,
    ) -> Result<Substitute, Error> {
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
        .fetch_one(&self.pool)
        .await?;

        Ok(substitute)
    }

    pub async fn delete_substitute_by_id(&self, id: TdbId) -> Result<(), Error> {
        sqlx::query("DELETE FROM substitutes WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    pub async fn delete_substitutes_by_id(&self, id: &[TdbId]) -> Result<(), Error> {
        sqlx::query("DELETE FROM substitutes WHERE id = ANY($1)")
            .bind(id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    pub async fn delete_substitute_by_name(
        &self,
        template_name: &str,
        substitute_name: &str,
    ) -> Result<(), Error> {
        sqlx::query(
            "
                 DELETE FROM substitutes s
                 USING templates t        
                 WHERE s.template_id = t.id
                 AND t.name = $1
                 AND s.name = $2
            ",
        )
        .bind(template_name)
        .bind(substitute_name)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn delete_substitutes_by_name(
        &self,
        template_name: &str,
        substitute_names: &[&str],
    ) -> Result<(), Error> {
        sqlx::query(
            "
                 DELETE FROM substitutes s
                 USING templates t        
                 WHERE s.template_id = t.id
                 AND t.name = $1
                 AND s.name = ANY($2)
            ",
        )
        .bind(template_name)
        .bind(substitute_names)
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}

#[cfg(test)]
mod dbtest {
    use crate::template_database::*;

    const DB_URL: &str = "postgres://funboy:funboy@localhost/funboy_db";

    async fn get_db_conn() -> TemplateDatabase {
        // TODO: Connect to designated testing database to not affect production data

        let db = TemplateDatabase::new(DB_URL.to_string()).await.unwrap();

        sqlx::query("ALTER SEQUENCE templates_id_seq RESTART WITH 1")
            .execute(&db.pool)
            .await
            .unwrap();

        sqlx::query("ALTER SEQUENCE substitutes_id_seq RESTART WITH 1")
            .execute(&db.pool)
            .await
            .unwrap();

        sqlx::query("TRUNCATE TABLE templates CASCADE")
            .execute(&db.pool)
            .await
            .unwrap();

        db
    }

    #[tokio::test]
    async fn connect_to_database() {
        let db = get_db_conn().await;
        dbg!(db);
    }

    #[tokio::test]
    async fn crud_template_by_id() {
        let db = get_db_conn().await;
        let noun = db.create_template("noun").await.unwrap();
        let verb = db.create_template("verb").await.unwrap();
        let adj = db.create_template("adj").await.unwrap();
        dbg!(db.read_templates(OrderBy::Default).await.unwrap());
        assert!(db.read_templates(OrderBy::Default).await.unwrap().len() == 3);
        let sustantivo = db
            .update_template_by_id(noun.id, "sustantivo")
            .await
            .unwrap();
        assert!(sustantivo.name == "sustantivo");
        db.delete_template_by_id(sustantivo.id).await.unwrap();
        db.delete_template_by_id(verb.id).await.unwrap();
        db.delete_template_by_id(adj.id).await.unwrap();
        dbg!(db.read_templates(OrderBy::Default).await.unwrap());
        assert!(db.read_templates(OrderBy::Default).await.unwrap().len() == 0);
    }

    #[tokio::test]
    async fn crud_template_by_name() {
        let db = get_db_conn().await;
        let noun = db.create_template("noun").await.unwrap();
        let verb = db.create_template("verb").await.unwrap();
        let adj = db.create_template("adj").await.unwrap();
        dbg!(db.read_templates(OrderBy::Default).await.unwrap());
        assert!(db.read_templates(OrderBy::Default).await.unwrap().len() == 3);
        let sustantivo = db
            .update_template_by_name(&noun.name, "sustantivo")
            .await
            .unwrap();
        assert!(sustantivo.name == "sustantivo");
        db.delete_template_by_name(&sustantivo.name).await.unwrap();
        db.delete_template_by_name(&verb.name).await.unwrap();
        db.delete_template_by_name(&adj.name).await.unwrap();
        dbg!(db.read_templates(OrderBy::Default).await.unwrap());
        assert!(db.read_templates(OrderBy::Default).await.unwrap().len() == 0);
    }

    #[tokio::test]
    async fn crud_substitute_by_id() {
        let db = get_db_conn().await;
        let noun_template = db.create_template("animal").await.unwrap();
        for name in ["cat", "dog", "bat"] {
            let substitute = db.create_substitute("animal", name).await.unwrap();
            assert!(substitute.name == name);
        }
        let substitutes = db
            .read_substitutes_from_template("animal", OrderBy::Default)
            .await
            .unwrap();
        dbg!(&substitutes);
        assert!(substitutes.len() == 3);
        for substitute in &substitutes {
            let prev_name = substitute.name.clone();
            let substitute = db
                .update_substitute_by_id(substitute.id, &substitute.name.to_uppercase())
                .await
                .unwrap();
            assert!(substitute.name == prev_name.to_uppercase());
        }
        for substitute in &substitutes {
            db.delete_substitute_by_id(substitute.id).await.unwrap();
        }
        dbg!(&substitutes);
        dbg!(db.read_templates(OrderBy::Default).await.unwrap());
        assert!(
            db.read_substitutes_from_template("animal", OrderBy::Default)
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
        let db = get_db_conn().await;
        let _ = db.create_template("fruit").await.unwrap();
        let banana = db.create_substitute("fruit", "banana").await.unwrap();
        dbg!(&banana);
        let apple = db
            .update_substitute_by_name("fruit", "banana", "apple")
            .await
            .unwrap();
        dbg!(&apple);
        assert!(
            db.read_substitutes_from_template("fruit", OrderBy::Default)
                .await
                .unwrap()
                .len()
                == 1
        );
        db.delete_substitute_by_name("fruit", "apple")
            .await
            .unwrap();
        assert!(
            db.read_substitutes_from_template("fruit", OrderBy::Default)
                .await
                .unwrap()
                .len()
                == 0
        );
        db.delete_template_by_name("fruit").await.unwrap();
    }

    #[tokio::test]
    async fn sort_templates() {
        let db = get_db_conn().await;
        let templates = [
            "food", "vehicle", "clothes", "number", "adj", "noun", "verb",
        ];

        for template in templates {
            db.create_template(template).await.unwrap();
        }

        let templates_by_name_asc = db
            .read_templates(OrderBy::Name(SortOrder::Ascending))
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
        let db = get_db_conn().await;
        let subs = ["mouse", "keyboard", "monitor", "microphone"];
        for sub in subs {
            db.create_substitute("computer_part", sub).await.unwrap();
        }

        dbg!(
            db.read_substitutes_from_template("computer_part", OrderBy::Default)
                .await
                .unwrap()
        );
        assert!(
            db.read_substitutes_from_template("computer_part", OrderBy::Default)
                .await
                .unwrap()
                .len()
                == 4
        );
        db.delete_substitutes_by_name("computer_part", &subs)
            .await
            .unwrap();
        assert!(
            db.read_substitutes_from_template("computer_part", OrderBy::Default)
                .await
                .unwrap()
                .len()
                == 0
        );
        dbg!(
            db.read_substitutes_from_template("computer_part", OrderBy::Default)
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn delete_substitutes_by_id() {
        let db = get_db_conn().await;
        let subs = ["mouse", "keyboard", "monitor", "microphone"];
        for sub in subs {
            db.create_substitute("computer_part", sub).await.unwrap();
        }

        let subs = db
            .read_substitutes_from_template("computer_part", OrderBy::Default)
            .await
            .unwrap();

        dbg!(&subs);
        assert!(subs.len() == 4);
        let subs: Vec<TdbId> = subs.iter().map(|sub| sub.id).collect();
        db.delete_substitutes_by_id(&subs).await.unwrap();
        assert!(
            db.read_substitutes_from_template("computer_part", OrderBy::Default)
                .await
                .unwrap()
                .len()
                == 0
        );
        dbg!(
            db.read_substitutes_from_template("computer_part", OrderBy::Default)
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn create_substitutes() {
        let db = get_db_conn().await;

        let sub_names = ["a", "b", "c", "d"];

        let subs = db.create_substitutes("example", &sub_names).await.unwrap();
        assert!(subs.len() == 4);
        dbg!(&subs);
        db.delete_substitutes_by_name("example", &sub_names)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn template_collision() {
        let db = get_db_conn().await;
        db.create_template("template_collision").await.unwrap();
        match db.create_template("template_collision").await {
            Ok(_) => panic!("Template collision should return error"),
            Err(e) => dbg!(e),
        };
    }

    #[tokio::test]
    async fn substitute_collision() {
        let db = get_db_conn().await;
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
}
