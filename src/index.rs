use rocket::form::Form;
use rocket::response::content::RawHtml;
use rocket::response::status::BadRequest;
use rocket::response::Redirect;
use rocket::State;
use serenity::http::CacheHttp;
use serenity::model::prelude::{Channel, ChannelId};

use crate::authorization::{AuthorizedDiscord, *};
use crate::error::ResponseResult;
use crate::state::ManagedState;

#[get("/")]
pub async fn index_authorized(
	_state: &State<ManagedState>,
	_token: AuthorizedDiscord,
) -> RawHtml<String> {
	RawHtml(format!(
		r#"<html><form method="post"><input type="hidden" name="_method" value="put"><label for="channel_id">Channel id:</label><br><input type="number" id="channel_id" name="channel_id"><br><label for="message">Message:</label><br><input type="text" id="message" name="message"><br><input type="submit"></form></html>"#
	))
}

#[get("/", rank = 99)]
pub async fn index_redirect() -> Redirect {
	Redirect::temporary(uri!(authorize))
}

#[derive(FromForm)]
pub struct Update<'r> {
	channel_id: u64,
	message:    &'r str,
}
#[put("/", data = "<update>")]
pub async fn update_greeting(
	state: &State<ManagedState>,
	authorized: AuthorizedDiscord,
	update: Form<Update<'_>>,
) -> ResponseResult<Result<&'static str, BadRequest<&'static str>>> {
	let Update {
		channel_id,
		message,
	} = update.into_inner();
	let Ok(channel) = ChannelId(channel_id).to_channel(&state.bot.cache_and_http).await else {return Ok(Err(BadRequest(Some("Unknown channel id."))))};
	let Channel::Guild(guild_channel) = channel else {return Ok(Err(BadRequest(Some("Channel is not inside a guild."))))};
	let guild_id = guild_channel.guild_id;
	let Ok(member) = guild_id.member(&state.bot.cache_and_http, authorized.id).await else {return Ok(Err(BadRequest(Some("Couldn't find your user in this guild."))))};
	let Ok(guild) = guild_id.to_partial_guild(state.bot.cache_and_http.http()).await  else {return Ok(Err(BadRequest(Some("Couldn't find guild."))))};
	if guild.owner_id != member.user.id
		&& member.roles.iter().any(|role_id| {
			guild
				.roles
				.get(role_id)
				.filter(|role| role.permissions.administrator())
				.is_some()
		}) {
		return Ok(Err(BadRequest(Some("You must be an administrator."))));
	}

	query!(
		"\
            INSERT INTO DiscordGreetings (GuildId, ChannelId, Message) VALUES (?, ?, ?)
            ON DUPLICATE KEY UPDATE GuildId=GuildId, ChannelId=ChannelId, Message=Message ",
		guild_id.0,
		channel_id,
		message
	)
	.execute(&mut state.acquire_connection().await?)
	.await?;

	Ok(Ok("Done."))
}
