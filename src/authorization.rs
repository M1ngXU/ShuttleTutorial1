use std::collections::HashMap;
use std::net::SocketAddr;

use rand::Rng;
use rocket::http::{Cookie, CookieJar, Status};
use rocket::request::{FromRequest, Outcome};
use rocket::response::content::RawHtml;
use rocket::response::status::Custom;
use rocket::response::Redirect;
use rocket::serde::Deserialize;
use rocket::Request;
use shuttle_service::Context;
use url::Url;

use crate::error::ResponseResult;
use crate::index::*;
use crate::managed_state::ManagedState;

const DISCORD_BASE_URL: &str = "https://discord.com/api/v10";

fn generate_state() -> String {
	const AUTHORIZATION_STATE_SIZE: usize = 20;
	const BASE62: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

	let mut authorization_state = String::with_capacity(AUTHORIZATION_STATE_SIZE);

	let mut rng = rand::thread_rng();
	for _ in 0..AUTHORIZATION_STATE_SIZE {
		authorization_state.push(BASE62[rng.gen::<usize>() % BASE62.len()] as char);
	}

	authorization_state
}

#[get("/authorize")]
pub async fn authorize(
	managed_state: &ManagedState,
	client_ip: SocketAddr,
) -> ResponseResult<Redirect> {
	let authorization_state = generate_state();

	query!(
		"\
			INSERT INTO DiscordAuthorizationState (Id, Ip) VALUES (?, ?) ",
		&authorization_state,
		client_ip.to_string()
	)
	.execute(&mut managed_state.acquire_connection().await?)
	.await
	.context("Failed to insert authorization id.")?;

	let mut url: Url = "https://discord.com/oauth2/authorize".parse().unwrap();
	url.query_pairs_mut()
		.append_pair("response_type", "code")
		.append_pair("client_id", &managed_state.get_client_id())
		.append_pair("scope", "identify")
		.append_pair("state", &authorization_state)
		.append_pair("redirect_uri", &managed_state.get_redirect_uri())
		.append_pair("prompt", "none");

	Ok(Redirect::temporary(url.to_string()))
}

#[get("/try_authorize?<code>&<state>")]
pub async fn try_authorize(
	cookies: &CookieJar<'_>,
	managed_state: &ManagedState,
	code: &str,
	state: &str,
	client_ip: SocketAddr,
) -> ResponseResult<Result<RawHtml<String>, Custom<&'static str>>> {
	let mut connection = managed_state.acquire_connection().await?;
	struct Ip {
		ip: String,
	}
	match query_as!(
		Ip,
		"\
			SELECT Ip as ip FROM DiscordAuthorizationState WHERE Id = ?",
		state
	)
	.fetch_one(&mut connection)
	.await
	{
		Ok(Ip { ip }) if ip == client_ip.to_string() => {}
		Ok(_) => return Ok(Err(Custom(Status::Unauthorized, "Ip mismatch"))),
		_ => return Ok(Err(Custom(Status::Unauthorized, "Unknown id."))),
	};
	query!("DELETE FROM DiscordAuthorizationState WHERE Id = ?", state)
		.execute(&mut connection)
		.await?;

	let mut url: Url = DISCORD_BASE_URL.parse().unwrap();
	url.path_segments_mut()
		.unwrap()
		.push("oauth2")
		.push("token");
	let client = reqwest::Client::new();

	let mut params = HashMap::new();
	params.insert("client_id", managed_state.get_client_id());
	params.insert("client_secret", managed_state.get_client_secret());
	params.insert("grant_type", "authorization_code".to_string());
	params.insert("code", code.to_string());
	params.insert("redirect_uri", managed_state.get_redirect_uri());

	#[derive(Deserialize)]
	#[serde(crate = "rocket::serde")]
	struct OAuthResponse {
		access_token: String,
		token_type: String,
		scope: String,
	}

	let oauth_response: OAuthResponse = client
		.post(url)
		.form(&params)
		.send()
		.await
		.context("Failed to get tokens by code.")?
		.json()
		.await
		.context("Failed to read response of tokens.")?;
	if &oauth_response.token_type != "Bearer" {
		error!("Got {} instead of Bearer token.", oauth_response.token_type);
		return Ok(Err(Custom(
			Status::BadRequest,
			"Only accepting `Bearer` tokens.",
		)));
	}
	if &oauth_response.scope != "identify" {
		return Ok(Err(Custom(
			Status::BadRequest,
			"Only accepting `identify` as the scope.",
		)));
	}

	#[derive(Deserialize)]
	#[serde(crate = "rocket::serde")]
	struct UserId {
		id: String,
	}
	let mut url: Url = DISCORD_BASE_URL.parse().unwrap();
	url.path_segments_mut().unwrap().push("users").push("@me");
	let UserId { id } = &client
		.get(url)
		.bearer_auth(&oauth_response.access_token)
		.send()
		.await
		.context("Failed to get user by authorization token.")?
		.json()
		.await
		.context("Failed read id from discord identity response.")?;
	cookies.add_private(Cookie::build("id", id.to_string()).permanent().finish());
	let redirect = uri!(index_authorized).to_string();
	// TODO put into tera file
	Ok(Ok(RawHtml(format!(
		"<html><head><meta http-equiv=\"refresh\" content=\"5; url={redirect}\"></head><body><a \
		 href=\"{redirect}\">Click here if you don't get redirected \
		 ...</a></body><script>window.location.href = '{redirect}';</script></html>"
	))))
}

// todo in doc
pub struct AuthorizedDiscord {
	pub id: u64,
}
#[async_trait]
impl<'r> FromRequest<'r> for AuthorizedDiscord {
	type Error = ();

	async fn from_request(request: &'r Request<'_>) -> Outcome<Self, Self::Error> {
		let cookies = request.cookies();
		match cookies.get_private("id").map(|c| c.to_string()) {
			Some(id) if id.parse::<u64>().is_ok() => Outcome::Success(Self {
				id: id.parse().unwrap(),
			}),
			c => {
				if c.is_some() {
					cookies.remove_private(Cookie::named("id"));
				}
				Outcome::Forward(())
			}
		}
	}
}
