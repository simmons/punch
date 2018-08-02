use std::path::PathBuf;

use actix::prelude::*;
use actix_web::middleware::identity::{CookieIdentityPolicy, IdentityService, RequestIdentity};
use actix_web::middleware::{Middleware, Started};
use actix_web::{
    self, middleware, App, AsyncResponder, Form, FutureResponse, HttpRequest, HttpResponse, State,
};
use askama::{self, Template};
use futures::Future;

use db::{self, AuthenticateUser, DatabaseError, DbExecutor, GetSummaryReport, PunchCommand};
use flash::{self, RequestFlash};
use models::PunchDirection;
use report::SummaryReport;

const ROOT_PATH: &str = "/";
const STATIC_PATH: &str = "/static";
const LOGIN_PATH: &str = "/login";
const LOGOUT_PATH: &str = "/logout";
const PUNCH_PATH: &str = "/punch";

/// Launch the Actix-web web server.
pub fn do_server(database: &str, bind: &str, static_path: &str) {
    let sys = actix::System::new("punch");

    let (db_addr, config) = db::database_init(database).unwrap();
    let static_path: PathBuf = PathBuf::from(static_path);

    // Start http server
    actix_web::server::new(move || {
        App::with_state(AppState{db: db_addr.clone()})
            .handler(STATIC_PATH,
                     actix_web::fs::StaticFiles::new(&static_path).unwrap()
                        .show_files_listing()
                     )
            // logger
            .middleware(middleware::Logger::default())
            // cookie-auth example
            .middleware(IdentityService::new(
                CookieIdentityPolicy::new(&config.secret.data)
                    .name("auth")
                    .secure(false),
            ))
            // authentication
            .middleware(AuthService::new())
            // flash messages
            .middleware(flash::FlashService::new())
            // resources
            .resource(LOGIN_PATH, |r| {
                r.get().f(|req| login_get(req));
                r.post().with(login_post);
            })
            .resource(LOGOUT_PATH, |r| r.f(logout))
            .resource(PUNCH_PATH, |r| {
                r.post().with(punch);
            })
            .resource(ROOT_PATH, |r| r.get().with(index))
    }).bind(bind)
        .unwrap()
        .start();

    println!("Started http server: {}", bind);
    let _ = sys.run();
}

/// Render an Askama template as an HttpResponse.
/// TODO: Investigate the use of the "with-actix-web" Askama feature which may eliminate the need
/// for this function.
fn render_html(template: impl askama::Template) -> HttpResponse {
    match template.render().map_err(|e| TemplateError(e)) {
        Ok(s) => HttpResponse::Ok().content_type("text/html").body(s),
        Err(e) => {
            error!("{}", e);
            HttpResponse::InternalServerError().into()
        }
    }
}

/// Application state with DbExecutor address
struct AppState {
    db: Addr<DbExecutor>,
}

////////////////////////////////////////////////////////////////////////

/// Middleware to confirm that an identity is present, and redirect to the login page if not.
struct AuthService {}

impl AuthService {
    fn new() -> AuthService {
        AuthService {}
    }
}

impl Middleware<AppState> for AuthService {
    fn start(&self, req: &HttpRequest<AppState>) -> actix_web::error::Result<Started> {
        match req.identity() {
            Some(_) => Ok(Started::Done), // User is authenticated
            None => {
                let path = req.path();
                if path == LOGIN_PATH || path.starts_with(STATIC_PATH) {
                    // No authentication is needed to get to the login page itself or the static
                    // assets.
                    Ok(Started::Done)
                } else {
                    // Redirect to the login page.
                    Ok(Started::Response(
                        HttpResponse::Found()
                            .header("location", LOGIN_PATH)
                            .finish(),
                    ))
                }
            }
        }
    }
}

////////////////////////////////////////////////////////////////////////
// Templates
////////////////////////////////////////////////////////////////////////

#[derive(Fail, Debug)]
#[fail(display = "Template error: {}", _0)]
pub struct TemplateError(askama::Error);

#[derive(Template)]
#[template(path = "login.html")]
struct LoginTemplate<'a> {
    error_message: Option<&'a str>,
}

#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate<'a> {
    username: &'a str,
    error_message: Option<String>,
    report: Option<SummaryReport>,
}

////////////////////////////////////////////////////////////////////////
// Endpoint handlers
////////////////////////////////////////////////////////////////////////

fn index(
    (request, state): (HttpRequest<AppState>, State<AppState>),
) -> FutureResponse<HttpResponse> {
    state
        .db
        .send(GetSummaryReport {})
        .from_err()
        .and_then(move |report| {
            let error_message = request.get_flash_message();
            let report = match report {
                Ok(report) => Some(report),
                Err(e) => {
                    error!("Unable to produce report: {}", e);
                    None
                }
            };
            Ok(render_html(IndexTemplate {
                username: &request.identity().unwrap_or("".to_string()),
                error_message,
                report,
            }))
        })
        .responder()
}

#[derive(Deserialize)]
struct LoginForm {
    username: String,
    password: String,
}

fn login_get(_: &HttpRequest<AppState>) -> HttpResponse {
    render_html(LoginTemplate {
        error_message: None,
    })
}

fn login_post(
    (req, state, params): (HttpRequest<AppState>, State<AppState>, Form<LoginForm>),
) -> FutureResponse<HttpResponse> {
    let LoginForm { username, password } = params.into_inner();
    state
        .db
        .send(AuthenticateUser {
            username: username.clone(),
            password,
        })
        .from_err()
        .and_then(move |res| match res {
            Ok(true) => {
                // Login successful
                req.remember(username);
                Ok(HttpResponse::Found().header("location", "/").finish())
            }
            Ok(false) | Err(_) => {
                // Bad username or password
                Ok(render_html(LoginTemplate {
                    error_message: Some("Invalid username and/or password."),
                }))
            }
        })
        .responder()
}

fn logout(req: &HttpRequest<AppState>) -> HttpResponse {
    req.forget();
    HttpResponse::Found().header("location", "/").finish()
}

#[derive(Deserialize, Debug)]
struct PunchForm {
    // project_id: String,
    //direction: bool, // true = punch-in
    direction: PunchDirection,

    note: Option<String>,
}

fn punch(
    (mut req, state, params): (HttpRequest<AppState>, State<AppState>, Form<PunchForm>),
) -> FutureResponse<HttpResponse> {
    let form = params.into_inner();
    state
        .db
        .send(PunchCommand {
            username: req.identity().unwrap_or("".to_string()),
            direction: form.direction,
            note: form.note,
        })
        .from_err()
        .and_then(move |res| {
            match res {
                Err(DatabaseError::BadState) => {
                    req.set_flash_message(
                        "You were already punched in/out.  Try refreshing the browser.",
                    );
                }
                Err(e) => {
                    req.set_flash_message(format!("{}", e));
                }
                Ok(_) => {}
            };
            Ok(HttpResponse::Found().header("location", "/").finish())
        })
        .responder()
}
