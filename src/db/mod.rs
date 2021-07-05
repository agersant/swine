use anyhow::*;
use diesel::r2d2::{self, ConnectionManager, PooledConnection};
use diesel::sqlite::SqliteConnection;
use diesel::RunQueryDsl;
use std::path::Path;

mod schema;

pub use self::schema::*;

#[allow(dead_code)]
const DB_MIGRATIONS_PATH: &str = "migrations";
embed_migrations!("migrations");

#[derive(Clone)]
pub struct DB {
	pool: r2d2::Pool<ConnectionManager<SqliteConnection>>,
}

#[derive(Debug)]
struct ConnectionCustomizer {}
impl diesel::r2d2::CustomizeConnection<SqliteConnection, diesel::r2d2::Error>
	for ConnectionCustomizer
{
	fn on_acquire(&self, connection: &mut SqliteConnection) -> Result<(), diesel::r2d2::Error> {
		let query = diesel::sql_query(
			r#"
			PRAGMA busy_timeout = 60000;
			PRAGMA journal_mode = WAL;
			PRAGMA synchronous = NORMAL;
			PRAGMA foreign_keys = ON;
		"#,
		);
		query
			.execute(connection)
			.map_err(diesel::r2d2::Error::QueryError)?;
		Ok(())
	}
}

impl DB {
	pub fn new(path: &Path) -> Result<DB> {
		std::fs::create_dir_all(&path.parent().unwrap())?;
		let manager = ConnectionManager::<SqliteConnection>::new(path.to_string_lossy());
		let pool = diesel::r2d2::Pool::builder()
			.connection_customizer(Box::new(ConnectionCustomizer {}))
			.build(manager)?;
		let db = DB { pool };
		db.migrate_up()?;
		Ok(db)
	}

	pub fn connect(&self) -> Result<PooledConnection<ConnectionManager<SqliteConnection>>> {
		self.pool.get().map_err(Error::new)
	}

	#[allow(dead_code)]
	fn migrate_down(&self) -> Result<()> {
		let connection = self.connect().unwrap();
		loop {
			match diesel_migrations::revert_latest_migration_in_directory(
				&connection,
				Path::new(DB_MIGRATIONS_PATH),
			) {
				Ok(_) => (),
				Err(diesel_migrations::RunMigrationsError::MigrationError(
					diesel_migrations::MigrationError::NoMigrationRun,
				)) => break,
				Err(e) => bail!(e),
			}
		}
		Ok(())
	}

	fn migrate_up(&self) -> Result<()> {
		let connection = self.connect().unwrap();
		embedded_migrations::run(&connection)?;
		Ok(())
	}
}

#[test]
fn run_migrations() {
	use crate::test::*;
	use crate::test_name;
	let output_dir = prepare_test_directory(test_name!());
	let db_path = output_dir.join("db.sqlite");
	let db = DB::new(&db_path).unwrap();

	db.migrate_down().unwrap();
	db.migrate_up().unwrap();
}
