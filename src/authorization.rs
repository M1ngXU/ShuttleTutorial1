use std::collections::HashMap;
use std::net::SocketAddr;

use rand::Rng;
use reqwest::Client;
use rocket::http::{Cookie, CookieJar, Status};
use rocket::request::{FromRequest, Outcome};
use rocket::response::status::Custom;
use rocket::response::Redirect;
use rocket::serde::Deserialize;
use rocket::Request;
use rocket_dyn_templates::{context, Template};
use shuttle_service::Context;
use sqlx::pool::PoolConnection;
use sqlx::MySql;
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
		"INSERT INTO DiscordAuthorizationState (Id, Ip) VALUES (?, ?)",
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

async fn verify_state(
	mut connection: PoolConnection<MySql>,
	state: &str,
	client_ip: SocketAddr,
) -> ResponseResult<bool> {
	struct Ip {
		ip: String,
	}
	let is_valid = match query_as!(
		Ip,
		"SELECT Ip as ip FROM DiscordAuthorizationState WHERE Id = ?",
		state
	)
	.fetch_one(&mut connection)
	.await
	{
		Ok(Ip { ip }) if ip == client_ip.to_string() => {
			query!("DELETE FROM DiscordAuthorizationState WHERE Id = ?", state)
				.execute(&mut connection)
				.await?;
			true
		}
		_ => false,
	};

	Ok(is_valid)
}

async fn get_access_token(
	managed_state: &ManagedState,
	client: &Client,
	code: &str,
) -> ResponseResult<Result<String, Custom<&'static str>>> {
	let mut url: Url = DISCORD_BASE_URL.parse().unwrap();
	url.path_segments_mut()
		.unwrap()
		.push("oauth2")
		.push("token");

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
		Ok(Err(Custom(
			Status::BadRequest,
			"Only accepting `identify` as the scope.",
		)))
	} else {
		Ok(Ok(oauth_response.access_token))
	}
}

async fn get_user_id(client: &Client, access_token: &str) -> ResponseResult<String> {
	#[derive(Deserialize)]
	#[serde(crate = "rocket::serde")]
	struct UserId {
		id: String,
	}
	let mut url: Url = DISCORD_BASE_URL.parse().unwrap();
	url.path_segments_mut().unwrap().push("users").push("@me");
	let UserId { id } = client
		.get(url)
		.bearer_auth(access_token)
		.send()
		.await
		.context("Failed to get user by authorization token.")?
		.json()
		.await
		.context("Failed read id from discord identity response.")?;
	Ok(id)
}

#[get("/try_authorize?<code>&<state>")]
pub async fn try_authorize(
	cookies: &CookieJar<'_>,
	managed_state: &ManagedState,
	code: &str,
	state: &str,
	client_ip: SocketAddr,
) -> ResponseResult<Result<Template, Custom<&'static str>>> {
	if !verify_state(managed_state.acquire_connection().await?, state, client_ip).await? {
		return Ok(Err(Custom(
			Status::Unauthorized,
			"This state is not linked to your ip address.",
		)));
	}

	let client = Client::new();

	let access_token = match get_access_token(managed_state, &client, code).await {
		Ok(Ok(access_token)) => access_token,
		e => return e.map(|r| r.map(|_| unreachable!())),
	};

	let user_id = get_user_id(&client, &access_token).await?;

	// permanent cookies, default is 1 week
	cookies.add_private(Cookie::build("id", user_id).permanent().finish());
	Ok(Ok(Template::render(
		"redirect_index",
		context! {redirect_url: uri!(index_authorized).to_string()},
	)))
}

pub struct AuthorizedDiscord {
	pub id: u64,
}
#[async_trait]
impl<'r> FromRequest<'r> for AuthorizedDiscord {
	type Error = ();

	async fn from_request(request: &'r Request<'_>) -> Outcome<Self, Self::Error> {
		let cookies = request.cookies();
		match cookies.get_private("id").map(|c| c.value().parse().ok()) {
			Some(Some(id)) => Outcome::Success(Self { id }),
			o => {
				if o.is_some() {
					cookies.remove_private(Cookie::named("id"));
				}
				Outcome::Forward(())
			}
		}
	}
}
