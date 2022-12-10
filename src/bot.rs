use serenity::http::CacheHttp;
use serenity::model::prelude::{ChannelId, Member, Ready};
use serenity::model::user::OnlineStatus;
use serenity::prelude::{Context, EventHandler};
use shuttle_service::Context as _Context;
use shuttle_service::error::CustomError;
use sqlx::MySqlPool;

pub struct Bot {
	database_pool: MySqlPool,
}
impl Bot {
	pub fn new(database_pool: MySqlPool) -> Self {
		Self { database_pool }
	}

    async fn greet(&self, ctx: Context, new_member: Member) -> Result<(), CustomError> {
		struct Greeting {
			channel_id: u64,
			message:    String,
		}
		let mut connection = self.database_pool.acquire().await.context("Failed to acquire connection.")?; {
			if let Ok(greeting) = query_as!(
				Greeting,
				"\
                    SELECT ChannelId as channel_id, Message as message \
                    FROM DiscordGreetings \
                    WHERE GuildId = ?
                ",
				new_member.guild_id.0
			)
			.fetch_one(&mut connection)
			.await
			{
				ChannelId(greeting.channel_id)
					.say(ctx.http(), greeting.message)
					.await
                    .context("Failed to send message.")?;
			}
		}

        Ok(())
	}
}

#[async_trait]
impl EventHandler for Bot {
    async fn ready(&self, ctx: Context, _data_about_bot: Ready) {
        ctx.set_presence(None, OnlineStatus::Online).await;
    }

	async fn guild_member_addition(&self, ctx: Context, new_member: Member) {
		if let Err(e) = self.greet(ctx, new_member).await {
            error!("Error while greeting: {e}");
        }
	}
}
