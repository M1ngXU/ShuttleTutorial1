use rocket::form::Form;
use rocket::response::status::BadRequest;
use rocket::response::Redirect;
use rocket_dyn_templates::{context, Template};
use serenity::http::Http;
use serenity::model::prelude::{Channel, ChannelId, Member};

use crate::authorization::*;
use crate::error::ResponseResult;
use crate::managed_state::ManagedState;

pub async fn is_admin(http: &Http, member: &Member) -> Result<bool, BadRequest<&'static str>> {
	let Ok(guild) = member.guild_id.to_partial_guild(http).await else {return Err(BadRequest(Some("Couldn't find guild.")))};
	Ok(guild.owner_id == member.user.id
		|| member.roles.iter().any(|role_id| {
			guild
				.roles
				.get(role_id)
				.filter(|role| role.permissions.administrator())
				.is_some()
		}))
}

#[get("/")]
pub async fn index_authorized(
	managed_state: &ManagedState,
	token: AuthorizedDiscord,
) -> Result<Template, BadRequest<&'static str>> {
	let mut guilds = Vec::new();
	let Ok(own_guilds) = managed_state
		.bot_cache()
		.current_user()
		.guilds(managed_state.bot_http())
		.await else {return Err(BadRequest(Some("Failed to fetch own guilds.")))};
	for guild_info in own_guilds {
		if let Ok(member) = guild_info
			.id
			.member(managed_state.bot_cache_and_http(), token.id)
			.await
		{
			if is_admin(managed_state.bot_http(), &member).await? {
				guilds.push(context! {
				id: guild_info.id.0,
				name: guild_info.name,
				channels: guild_info
					.id
					.channels(managed_state.bot_http())
					.await
					.unwrap()
					.into_iter()
					.filter(|(_id, channel)| channel.is_text_based())
					.map(|(id, channel)| context!{id: id.0, name: channel.name})
					.collect::<Vec<_>>()});
			}
		}
	}

	Ok(Template::render("index", context! {guilds}))
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
	managed_state: &ManagedState,
	authorized: AuthorizedDiscord,
	update: Form<Update<'_>>,
) -> ResponseResult<Result<&'static str, BadRequest<&'static str>>> {
	let Update {
		channel_id,
		message,
	} = update.into_inner();
	let Ok(channel) = ChannelId(channel_id).to_channel(&managed_state.bot_cache_and_http()).await else {return Ok(Err(BadRequest(Some("Unknown channel id."))))};
	let Channel::Guild(guild_channel) = channel else {return Ok(Err(BadRequest(Some("Channel is not inside a guild."))))};
	let guild_id = guild_channel.guild_id;
	let Ok(member) = guild_id.member(&managed_state.bot_cache_and_http(), authorized.id).await else {return Ok(Err(BadRequest(Some("Couldn't find your user in this guild."))))};
	match is_admin(managed_state.bot_http(), &member).await {
		Ok(true) => {}
		Ok(false) => return Ok(Err(BadRequest(Some("You must be an administrator.")))),
		Err(e) => return Ok(Err(e)),
	}

	#[rustfmt::skip]
	query!(
		"\
            INSERT INTO DiscordGreeting (GuildId, ChannelId, Message) \
			VALUES (?, ?, ?)
            ON DUPLICATE KEY \
				UPDATE GuildId=GuildId, ChannelId=ChannelId, Message=Message\
		",
		guild_id.0,
		channel_id,
		message
	)
	.execute(&mut managed_state.acquire_connection().await?)
	.await?;

	Ok(Ok("Done."))
}
