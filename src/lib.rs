#[macro_use]
extern crate rocket;
#[macro_use]
extern crate sqlx;

mod authorization;
mod bot;
mod error;
mod index;
mod managed_state;

use authorization::*;
use bot::Bot;
use index::*;
use managed_state::ManagedStateInner;
use rocket::tokio::spawn;
use rocket::Config;
use rocket_dyn_templates::Template;
use serenity::prelude::GatewayIntents;
use serenity::Client;
use shuttle_secrets::SecretStore;
use shuttle_service::Context;
use sqlx::MySqlPool;

#[shuttle_service::main]
async fn rocket(
	#[shuttle_aws_rds::MariaDB] database_pool: MySqlPool,
	#[shuttle_secrets::Secrets] secret_store: SecretStore,
) -> shuttle_service::ShuttleRocket {
	let bot_token = secret_store
		.get("DISCORD_BOT_TOKEN")
		.context("No `DISCORD_BOT_TOKEN` in `Shuttle.toml`.")?;

	let mut bot = Client::builder(&bot_token, GatewayIntents::non_privileged() | GatewayIntents::GUILD_MEMBERS)
		.event_handler(Bot::new(database_pool.clone()))
		.await
		.context("Failed to create discord bot.")?;
	spawn(async move { bot.start().await.expect("Failed to run bot.") });

	let rocket = rocket::custom(
		Config::figment().merge((
			Config::SECRET_KEY,
			secret_store
				.get("ROCKET_SECRET_KEY")
				.context("No `ROCKET_SECRET_KEY` in `Secrets.toml`.")?,
		)),
	)
	.attach(Template::fairing())
	.manage(ManagedStateInner::new(
		secret_store,
		database_pool,
		Client::builder(&bot_token, GatewayIntents::non_privileged() | GatewayIntents::GUILD_MEMBERS)
			.await
			.context("Failed to create discord bot.")?,
	))
	.mount("/", routes![
		index_authorized,
		index_redirect,
		authorize,
		try_authorize,
		update_greeting
	]);

	Ok(rocket)
}
