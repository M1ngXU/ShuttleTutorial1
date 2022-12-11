use rocket::form::Form;
use rocket::response::content::RawHtml;
use rocket::response::status::BadRequest;
use rocket::response::Redirect;
use serenity::model::prelude::{Channel, ChannelId};

use crate::authorization::{AuthorizedDiscord, *};
use crate::error::ResponseResult;
use crate::managed_state::ManagedState;
use crate::utils::is_admin;

#[get("/")]
pub async fn index_authorized(
	managed_state: &ManagedState,
	token: AuthorizedDiscord,
) -> Result<RawHtml<String>, BadRequest<&'static str>> {
	let mut admin_guilds = String::new();
	let mut channels = String::new();
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
			channels.push_str(&format!(
				r#"
					<form method="post" class="channel hide" id="channel-{}">
						<input type="hidden" name="_method" value="put">
						<label for="channel_id">Channel id:</label>
						<br>
						<select name="channel_id">{}</select>
						<br>
						<label for="message">Message:</label>
						<br>
						<input type="text" name="message">
						<br>
						<input type="submit">
					</form>
				"#,
				guild_info.id,
				guild_info
					.id
					.channels(managed_state.bot_http())
					.await
					.unwrap()
					.into_iter()
					.filter(|(_id, channel)| channel.is_text_based())
					.map(|(id, channel)| format!(
						r#"<option value="{id}">{}</option>"#,
						channel.name
					))
					.collect::<Vec<_>>()
					.join("")
			));
			if is_admin(managed_state.bot_http(), &member).await? {
				admin_guilds.push_str(&format!(
					r#"<option value="{}">{}</option>"#,
					guild_info.id, guild_info.name
				));
			}
		}
	}

	Ok(RawHtml(format!(
		r#"
			<html>
				<label for="guild_id">Select a guild: </label>
				<select name="guild_id" onchange="updateChannelDropdown(this);">{}</select>
				{}
				<style>
					.hide {{
						display: none;
					}}
				</style>
				<script>
					function updateChannelDropdown(e) {{
						document.querySelectorAll('.channel').forEach(e => e.classList.add('hide'));
						document.getElementById('channel-' + e.value).classList.remove('hide');
					}}
				</script>
			</html>
		"#,
		admin_guilds, channels
	)))
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
	match is_admin(&managed_state.bot_http(), &member).await {
		Ok(true) => {}
		Ok(false) => return Ok(Err(BadRequest(Some("You must be an administrator.")))),
		Err(e) => return Ok(Err(e)),
	}

	query!(
		"\
            INSERT INTO DiscordGreeting (GuildId, ChannelId, Message) VALUES (?, ?, ?)
            ON DUPLICATE KEY UPDATE GuildId=GuildId, ChannelId=ChannelId, Message=Message ",
		guild_id.0,
		channel_id,
		message
	)
	.execute(&mut managed_state.acquire_connection().await?)
	.await?;

	Ok(Ok("Done."))
}
