use std::collections::HashMap;

use rocket::http::{Cookie, CookieJar};
use rocket::request::{FromRequest, Outcome};
use rocket::response::content::RawHtml;
use rocket::response::Redirect;
use rocket::serde::Deserialize;
use rocket::{Request, State};
use shuttle_service::Context;
use url::Url;

use crate::error::ResponseResult;
use crate::index::*;
use crate::state::ManagedState;

const DISCORD_BASE_URL: &str = "https://discord.com/api/v10";

#[get("/authorize")]
pub async fn authorize(state: &State<ManagedState>) -> Redirect {
	let mut url: Url = "https://discord.com/oauth2/authorize".parse().unwrap();
	url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", &state.get_client_id())
        .append_pair("scope", "identify")
        // TODO add state for security
        .append_pair("redirect_uri", &state.get_redirect_uri())
        .append_pair("prompt", "none");

	Redirect::temporary(url.to_string())
}

#[get("/try_authorize?<code>")]
pub async fn try_authorize(
	cookies: &CookieJar<'_>,
	state: &State<ManagedState>,
	code: &str,
) -> ResponseResult<RawHtml<String>> {
	let mut url: Url = DISCORD_BASE_URL.parse().unwrap();
	url.path_segments_mut()
		.unwrap()
		.push("oauth2")
		.push("token");
	let client = reqwest::Client::new();

	let mut params = HashMap::new();
	params.insert("client_id", state.get_client_id());
	params.insert("client_secret", state.get_client_secret());
	params.insert("grant_type", "authorization_code".to_string());
	params.insert("code", code.to_string());
	params.insert("redirect_uri", state.get_redirect_uri());

	#[derive(Deserialize)]
	#[serde(crate = "rocket::serde")]
	struct OAuthResponse {
		access_token: String,
		token_type:   String,
	}

	let response = client
		.post(url)
		.form(&params)
		.send()
		.await
		.context("Failed to get tokens by code.")?
		.text()
		.await
		.context("Failed to read response of tokens.")?;
	let Ok(oauth_response) = serde_json::from_str::<OAuthResponse>(&response) else {Err(format!("Failed to read token information from response.\nResponse: {response}"))?};
	if &oauth_response.token_type != "Bearer" {
		Err(format!(
			"Only accepting bearer tokens, got `{}`.",
			oauth_response.token_type
		))?;
	}

	let mut url: Url = DISCORD_BASE_URL.parse().unwrap();
	url.path_segments_mut().unwrap().push("users").push("@me");
	let id = &client
		.get(url)
		.bearer_auth(&oauth_response.access_token)
		.send()
		.await
		.context("Failed to get user by authorization token.")?
		.json::<serde_json::Value>()
		.await
		.context("Failed read id from discord identity response.")?["id"];
	let id = id
		.as_str()
		.ok_or_else(|| format!("Id (`{id}`) is not a string."))?;
	cookies.add_private(
		Cookie::build("token", oauth_response.access_token)
			.permanent()
			.finish(),
	);
	cookies.add_private(Cookie::build("id", id.to_string()).permanent().finish());
	let redirect = uri!(index_authorized).to_string();
	// TODO put into tera file
	Ok(RawHtml(format!(
		"<html><head><meta http-equiv=\"refresh\" content=\"5; url={redirect}\"></head><body><a \
		 href=\"{redirect}\">Click here if you don't get redirected \
		 ...</a></body><script>window.location.href = '{redirect}';</script></html>"
	)))
}

pub struct AuthorizedDiscord {
	pub id:    u64,
	pub token: String,
}

const PRIVATE_COOKIES: [&str; 2] = ["token", "id"];

#[async_trait]
impl<'r> FromRequest<'r> for AuthorizedDiscord {
	type Error = ();

	async fn from_request(request: &'r Request<'_>) -> Outcome<Self, Self::Error> {
		let cookies = request.cookies();
		match PRIVATE_COOKIES.map(|key| cookies.get_private(key).map(|c| c.value().to_string())) {
			// TODO
			[Some(token), Some(id)] if id.parse::<u64>().is_ok() => Outcome::Success(Self {
				token,
				id: id.parse().unwrap(),
			}),
			maybe_nonexisting_cookies => {
				for key in maybe_nonexisting_cookies
					.iter()
					.enumerate()
					.filter(|(_i, value)| value.is_some())
					.map(|(i, _value)| PRIVATE_COOKIES[i])
				{
					cookies.remove_private(Cookie::named(key))
				}
				Outcome::Forward(())
			}
		}
	}
}
