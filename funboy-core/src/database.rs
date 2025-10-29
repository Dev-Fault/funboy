use sqlx::{Error, FromRow, Pool, Postgres, Row};

#[derive(Debug)]
pub struct FunboyDatabase {
    url: String,
    pool: Pool<Postgres>,
}

#[derive(Debug, FromRow)]
pub struct Template {
    pub id: i32,
    pub name: String,
}

#[derive(Debug, FromRow)]
pub struct Substitute {
    pub id: i32,
    pub name: String,
    pub template_id: i32,
}

impl FunboyDatabase {
    pub async fn new(url: String) -> Result<Self, sqlx::Error> {
        let pool = sqlx::postgres::PgPool::connect(&url).await?;

        sqlx::migrate!("./migrations").run(&pool).await?;

        Ok(FunboyDatabase { url, pool })
    }

    pub async fn create_template(&self, name: &str) -> Result<Template, Error> {
        let template =
            sqlx::query_as::<_, Template>("INSERT INTO templates (name) VALUES ($1) RETURNING *")
                .bind(name)
                .fetch_one(&self.pool)
                .await?;

        Ok(template)
    }

    pub async fn update_template(&self, old_name: &str, new_name: &str) -> Result<Template, Error> {
        let template = sqlx::query_as::<_, Template>(
            "UPDATE templates SET name = $1 WHERE name = $2 RETURNING *",
        )
        .bind(new_name)
        .bind(old_name)
        .fetch_one(&self.pool)
        .await?;

        Ok(template)
    }

    pub async fn read_templates(&self) -> Result<Vec<Template>, Error> {
        let templates = sqlx::query_as::<_, Template>("SELECT * FROM templates")
            .fetch_all(&self.pool)
            .await?;

        Ok(templates)
    }

    pub async fn delete_template(&self, name: &str) -> Result<(), Error> {
        sqlx::query("DELETE FROM templates WHERE name = $1")
            .bind(name)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    pub async fn create_substitute(
        &self,
        template_name: &str,
        substitute_name: &str,
    ) -> Result<Substitute, Error> {
        let template = sqlx::query_as::<_, Template>(
            "INSERT INTO templates (name) VALUES ($1)
             ON CONFLICT (name) DO UPDATE SET name = EXCLUDED.name
             RETURNING *",
        )
        .bind(template_name)
        .fetch_one(&self.pool)
        .await?;

        let substitute = sqlx::query_as::<_, Substitute>(
            "INSERT INTO substitutes (name, template_id) VALUES ($1, $2) RETURNING *",
        )
        .bind(substitute_name)
        .bind(template.id)
        .fetch_one(&self.pool)
        .await?;

        Ok(substitute)
    }

    pub async fn read_substitutes_from_template(
        &self,
        template_name: &str,
    ) -> Result<Vec<Substitute>, Error> {
        let substitutes = sqlx::query_as::<_, Substitute>(
            "SELECT s.*
             FROM SUBSTITUTES s
             JOIN templates t ON s.template_id = t.id
             WHERE t.name = $1",
        )
        .bind(template_name)
        .fetch_all(&self.pool)
        .await?;

        Ok(substitutes)
    }

    pub async fn update_substitute(&self, id: i32, name: &str) -> Result<Substitute, Error> {
        let substitute = sqlx::query_as::<_, Substitute>(
            "UPDATE substitutes SET name = $1 WHERE id = $2 RETURNING *",
        )
        .bind(name)
        .bind(id)
        .fetch_one(&self.pool)
        .await?;

        Ok(substitute)
    }

    pub async fn delete_substitute(&self, id: i32) -> Result<(), Error> {
        sqlx::query("DELETE FROM substitutes WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }
}

#[cfg(test)]
mod dbtest {
    use crate::database::*;

    async fn get_db_conn() -> FunboyDatabase {
        // TODO: Connect to designated testing database to not affect production data
        let db = FunboyDatabase::new("postgres://funboy:funboy@localhost/funboy_db".to_string())
            .await
            .unwrap();

        sqlx::query("ALTER SEQUENCE templates_id_seq RESTART WITH 1")
            .execute(&db.pool)
            .await
            .unwrap();

        sqlx::query("ALTER SEQUENCE substitutes_id_seq RESTART WITH 1")
            .execute(&db.pool)
            .await
            .unwrap();

        db
    }

    #[tokio::test]
    async fn database_makes_connection() {
        let db = get_db_conn().await;
        dbg!(db);
    }

    #[tokio::test]
    async fn template_crud() {
        let db = get_db_conn().await;
        let noun_template = db.create_template("noun").await.unwrap();
        let verb_template = db.create_template("verb").await.unwrap();
        let adj_template = db.create_template("adj").await.unwrap();
        dbg!(db.read_templates().await.unwrap());
        db.delete_template(&noun_template.name).await.unwrap();
        db.delete_template(&verb_template.name).await.unwrap();
        db.delete_template(&adj_template.name).await.unwrap();
        dbg!(db.read_templates().await.unwrap());
    }

    #[tokio::test]
    async fn substitute_crud() {
        let db = get_db_conn().await;
        let noun_template = db.create_template("animal").await.unwrap();
        for name in ["cat", "dog", "bat"] {
            let substitute = db.create_substitute("animal", name).await.unwrap();
            assert!(substitute.name == name);
        }
        let substitutes = db.read_substitutes_from_template("animal").await.unwrap();
        dbg!(&substitutes);
        for substitute in &substitutes {
            let substitute = db
                .update_substitute(substitute.id, &substitute.name.to_uppercase())
                .await
                .unwrap();
        }
        dbg!(&substitutes);
        dbg!(db.read_templates().await.unwrap());
        db.delete_template(&noun_template.name).await.unwrap();
        assert!(db.read_templates().await.unwrap().len() == 0);
    }
}
