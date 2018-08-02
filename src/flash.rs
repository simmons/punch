use actix_web;
use actix_web::http::Cookie;
use actix_web::middleware::{Middleware, Response, Started};
use actix_web::{HttpRequest, HttpResponse};
use serde_json;

use std::time::SystemTime;

static FLASH_COOKIE_NAME: &str = "flash";
static FLASH_COOKIE_PATH: &str = "/";
static MAX_ELAPSED_SECS: u64 = 60;

/// Middleware to manage "flash" messages that allow errors to be displayed to the user after a
/// redirect.  This isn't a watertight solution, but the need may go away in the future if Punch is
/// migrated to full-AJAX with a proper web API.
pub struct FlashService {}

impl FlashService {
    pub fn new() -> FlashService {
        FlashService {}
    }

    fn parse_cookie<S>(&self, request: &HttpRequest<S>) -> Option<Message> {
        let cookie = request.cookie(FLASH_COOKIE_NAME)?;
        let message: Message = serde_json::from_str(cookie.value()).ok()?;

        // Enforce freshness
        if message.time.elapsed().ok()?.as_secs() > MAX_ELAPSED_SECS {
            return None;
        }

        Some(message)
    }
}

impl<S> Middleware<S> for FlashService {
    fn start(&self, request: &HttpRequest<S>) -> actix_web::error::Result<Started> {
        if let Some(message) = self.parse_cookie(request) {
            request.extensions_mut().insert(message);
        }
        Ok(Started::Done)
    }

    fn response(
        &self,
        req: &HttpRequest<S>,
        mut response: HttpResponse,
    ) -> actix_web::error::Result<Response> {
        match req.extensions().get::<Message>() {
            Some(message) => {
                if message.delete {
                    // Actually deleting a cookie from the browser is problematic, but this should
                    // at least invalidate it.
                    let mut cookie = Cookie::named(FLASH_COOKIE_NAME);
                    cookie.set_path(FLASH_COOKIE_PATH);
                    response.add_cookie(&cookie)?;
                } else if message.create {
                    // This message is newly created, so add a fresh cookie.
                    let json = serde_json::to_string(message)?;
                    let mut cookie = Cookie::new(FLASH_COOKIE_NAME, json);
                    cookie.set_path(FLASH_COOKIE_PATH);
                    response.add_cookie(&cookie)?;
                }
            }
            None => {}
        }

        Ok(Response::Done(response))
    }
}

#[derive(Serialize, Deserialize, Debug)]
struct Message {
    time: SystemTime,
    text: String,
    #[serde(skip_serializing, skip_deserializing)]
    delete: bool,
    #[serde(skip_serializing, skip_deserializing)]
    create: bool,
}
impl Message {
    fn new<T: Into<String>>(text: T) -> Message {
        Message {
            time: SystemTime::now(),
            text: text.into(),
            delete: false,
            create: true,
        }
    }
}

pub trait RequestFlash {
    fn set_flash_message<T: Into<String>>(&mut self, text: T);
    fn get_flash_message(&self) -> Option<String>;
}

impl<S> RequestFlash for HttpRequest<S> {
    fn set_flash_message<T: Into<String>>(&mut self, text: T) {
        self.extensions_mut().insert(Message::new(text));
    }

    fn get_flash_message(&self) -> Option<String> {
        let mut extensions = self.extensions_mut();
        let message: &mut Message = extensions.get_mut()?;
        if message.delete {
            None
        } else {
            message.delete = true;
            Some(message.text.clone())
        }
    }
}
