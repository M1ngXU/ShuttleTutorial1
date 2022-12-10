use sqlx::{Executor, MySqlPool};
use toml::Value;

fn main() {
	let secrets: Value = include_str!("Secrets.toml").parse().unwrap();

	initialize_database(&secrets);
}

fn initialize_database(secrets: &Value) {
	let database_url = secrets[if cfg!(debug_assertions) {
		"DATABASE_URL_LOCAL"
	} else {
		"DATABASE_URL_DEPLOY"
	}]
	.as_str()
	.expect(
		"Couldn't fine `DATABASE_URL_LOCAL` (if running locally)/`DATABASE_URL_DEPLOY` (if \
		 deploying) in `Secrets.toml`.",
	);
	println!("cargo:rustc-env=DATABASE_URL={database_url}",);

	run_schema(database_url);
}

#[tokio::main]
async fn run_schema(database_url: &str) {
	MySqlPool::connect(database_url)
		.await
		.expect("Failed to get database pool.")
		.execute(include_str!("schema.sql"))
		.await
		.expect("Failed to run `schema.sql`.");
}
