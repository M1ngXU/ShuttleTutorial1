use serenity::Client;
use shuttle_secrets::SecretStore;
use sqlx::pool::PoolConnection;
use sqlx::{MySql, MySqlPool};

pub struct ManagedState {
	secret_store: SecretStore,
	database_pool: MySqlPool,
	pub bot: Client,
}
impl ManagedState {
	pub fn new(secret_store: SecretStore, database_pool: MySqlPool, bot: Client) -> Self {
		Self {
			secret_store,
			database_pool,
			bot,
		}
	}

	pub fn get_redirect_uri(&self) -> String {
		// using return statements as for `debug_assertions` another statement is
		// following (#[cfg(...)])
		#[cfg(debug_assertions)]
		return self
			.secret_store
			.get("DISCORD_REDIRECT_LOCAL")
			.expect("Couldn't find `DISCORD_REDIRECT_LOCAL` in `Secrets.toml`.");
		#[cfg(not(debug_assertions))]
		return self
			.secret_store
			.get("DISCORD_REDIRECT_DEPLOY")
			.expect("Couldn't find `DISCORD_REDIRECT_DEPLOY` in `Secrets.toml`.");
	}

	pub fn get_client_id(&self) -> String {
		self.secret_store
			.get("DISCORD_CLIENT_ID")
			.expect("Couldn't find `DISCORD_CLIENT_ID` in `Secrets.toml`.")
	}

	pub fn get_client_secret(&self) -> String {
		self.secret_store
			.get("DISCORD_CLIENT_SECRET")
			.expect("Couldn't find `DISCORD_CLIENT_SECRET` in `Secrets.toml`.")
	}

	pub async fn acquire_connection(&self) -> Result<PoolConnection<MySql>, sqlx::Error> {
		self.database_pool.acquire().await
	}
}
