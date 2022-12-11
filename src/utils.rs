use rocket::response::status::BadRequest;
use serenity::http::Http;
use serenity::model::prelude::Member;

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
