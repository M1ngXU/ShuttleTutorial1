use rocket::http::Status;
use rocket::response::Responder;

pub type ResponseResult<T> = Result<T, Error>;

pub struct Error(String);

impl<'r> Responder<'r, 'static> for Error {
	fn respond_to(self, request: &'r rocket::Request<'_>) -> rocket::response::Result<'static> {
		error!("Request to {} failed with: {}", request.uri(), self.0);
		Err(Status::InternalServerError)
	}
}

impl<T: std::fmt::Debug> From<T> for Error {
	fn from(value: T) -> Self {
		Self(format!("{:?}", value))
	}
}
