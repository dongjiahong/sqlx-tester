use std::{path::Path, thread};

use sqlx::{
    migrate::{MigrationSource, Migrator},
    Connection, Executor, PgConnection, PgPool,
};
use tokio::runtime::Runtime;
use uuid::Uuid;

#[derive(Debug)]
pub struct TestPg {
    pub server_url: String,
    pub dbname: String,
}

impl TestPg {
    pub fn new<S>(server_url: String, migrations: S) -> Self
    where
        S: MigrationSource<'static> + Send + Sync + 'static,
    {
        let uuid = Uuid::new_v4();
        let dbname = format!("test_{}", uuid);
        let dbname_cloned = dbname.clone();

        let tdb = Self { server_url, dbname };

        let server_url = tdb.server_url();
        let url = tdb.url();

        // create database dbname
        thread::spawn(move || {
            let rt = Runtime::new().unwrap();
            rt.block_on(async move {
                // use server url to crate database
                let mut conn = PgConnection::connect(&server_url).await.unwrap();
                conn.execute(format!(r#"CREATE DATABASE "{}""#, dbname_cloned).as_str())
                    .await
                    .unwrap();

                // now connect to test database for migration
                let mut conn = PgConnection::connect(&url).await.unwrap();
                let m = Migrator::new(migrations).await.unwrap();
                m.run(&mut conn).await.unwrap();
            });
        })
        .join()
        .expect("failed to create database");

        tdb
    }

    pub fn server_url(&self) -> String {
        self.server_url.clone()
    }

    pub fn url(&self) -> String {
        format!("{}/{}", self.server_url, self.dbname)
    }

    pub async fn get_pool(&self) -> PgPool {
        PgPool::connect(&self.url()).await.unwrap()
    }
}

impl Drop for TestPg {
    fn drop(&mut self) {
        let server_url = self.server_url();
        let dbname = self.dbname.clone();
        thread::spawn(move ||{
            let rt = Runtime::new().unwrap();
            rt.block_on(async move{
                let mut conn = PgConnection::connect(&server_url).await.unwrap();
                // terminate existing connections
                sqlx::query(&format!(r#"SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE pid <> pg_backend_pid() AND datname = '{}'"#, dbname))
                .execute(&mut conn)
                .await
                .expect("Terminate all other connections");
                conn.execute(format!(r#"DROP DATABASE "{}""#, dbname).as_str())
                .await
                .expect("Error while querying the drop database");
            });
        })
        .join()
        .expect("failed to drop database");
    }
}

impl Default for TestPg {
    fn default() -> Self {
        Self::new(
            "postgres://postgres:admin123@172.18.3.1:5432".to_string(),
            Path::new("./migrations"),
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::postgres::TestPg;

    #[tokio::test]
    async fn test_postgres_should_create_and_drop() {
        let tdb = TestPg::default();
        let pool = tdb.get_pool().await;
        // insert todo
        sqlx::query("INSERT INTO todos (title) VALUES ('test')")
            .execute(&pool)
            .await
            .unwrap();

        // get todo
        let (id, title) = sqlx::query_as::<_, (i32, String)>("SELECT id, title FROM todos")
            .fetch_one(&pool)
            .await
            .unwrap();

        assert_eq!(id, 1);
        assert_eq!(title, "test");
    }
}
