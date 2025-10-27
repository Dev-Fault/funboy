use sqlx::Row;

struct FunboyDatabase {}

impl FunboyDatabase {
    pub async fn new() -> Result<i32, sqlx::Error> {
        let url = "postgres://funboy:funboy@localhost/funboy_db";
        let pool = sqlx::postgres::PgPool::connect(url).await?;

        sqlx::migrate!("./migrations").run(&pool).await?;

        let res = sqlx::query("SELECT 1 + 1 as sum").fetch_one(&pool).await?;

        let sum: i32 = res.get("sum");
        Ok(sum)
    }
}

#[cfg(test)]
mod dbtest {
    use crate::database::FunboyDatabase;

    #[tokio::test]
    async fn database_makes_connection() {
        let result = FunboyDatabase::new().await.unwrap();
        assert!(result == 2);
    }
}
