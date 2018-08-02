//! Punch is a minimally viable time tracking web app.  *Very* minimally viable.  In fact, all it
//! does at this time is allow the user to punch-in, punch-out, and show a report of accumulated
//! hours over recent days and weeks.
//!
//! ![](https://raw.githubusercontent.com/simmons/punch/master/screenshot.png)
//!
//! Punch has the notion of "gross time" versus "net time", the former being the total time elapsed
//! between punch-in and punch-out events, and the latter subtracting a fixed amount of overhead
//! time per work session (currently 15 minutes) to account for the ramp-up period before one is
//! productive after starting work for the day or after an interruption.
//!
//! Punch is written in [Rust](https://www.rust-lang.org/) using the [actix-web](https://actix.rs/)
//! web framework, [Diesel](http://diesel.rs/) for database ORM, and numerous other crates.  When
//! you build punch with "cargo build", be sure to have the SQLite libraries installed on your
//! system.  On Ubuntu, for example:
//! ```
//! apt install sqlite3 libsqlite3 libsqlite3-dev
//! ```
//!
//! To start using punch, first initialize its SQLite database with a username and password for the
//! user:
//! ```
//! punch-web init --database-url=/path/to/punch.db myusername mypassword
//! ```
//! The `--database-url` argument is optional, and defaults to "punch.db" in the current directory.
//!
//! To run the web server, use the "server" subcommand:
//! ```
//! punch-web server --bind 127.0.0.1:8080 \
//!     --database-url=/path/to/punch.db --static-path=/path/to/static/files
//! ```
//! The bind address and port defaults to `127.0.0.1:8080`, the database URL defaults once again
//! to "punch.db" in the current directory, and the path to static resources defaults to "static/"
//! in the current directory.
//!
//! ## Ideas for future improvements
//!
//! For a glorified notepad with aspirations of being a time tracker, what *couldn't* be improved?
//! A few possible ideas are:
//!
//! * Support adding text notes to punch-in and punch-out events.  Also support a "note" event for
//! adding timestamped notes without punching in or out.
//! * Support multiple projects and users.  The database schema is in place for this, but this
//! minimally viable code currently looks for a singleton user and project.
//! * Dates are always stored in the database as UTC, but we currently use the server's local time
//! zone when interpreting dates.  This may or may not be the user's preferred time zone.  We
//! should support per-user or per-project configurable time zones.
//! * A proper frontend with AJAX calls could lead to a cleaner implementation, at the expense of
//! having to develop such frontend code.  (For example, this could avoid the hokey system of
//! storing error messages in a cookie to survive the redirect after a form post.)
//! * Numerous per-project parameters could be added to alter time accounting.  For example:
//!   * Configurable overhead time.
//!   * Rounding time up, down, or to the nearest hour (or half hour, quarter hour, etc.) on a
//!     per-session, per-week, or per-day basis.
//!   * Accumulation of "vacation" time at specified rates to allow the user to reward himself or
//!   herself after logging enough productive time.
//! * A command-line interface, which could be implemented as HTTP client calls to REST endpoints.
//! * More reports.
//!
//! ## License
//!
//! Punch is distributed under the terms of both the MIT license and the
//! Apache License (Version 2.0).  See LICENSE-APACHE and LICENSE-MIT for details.
//!
//! ### Contributing
//!
//! Unless you explicitly state otherwise, any contribution you intentionally submit
//! for inclusion in the work, as defined in the Apache-2.0 license, shall be
//! dual-licensed as above, without any additional terms or conditions.

extern crate actix;
extern crate actix_web;
extern crate bcrypt;
extern crate clap;
#[macro_use]
extern crate diesel;
#[macro_use]
extern crate diesel_migrations;
extern crate failure;
#[macro_use]
extern crate failure_derive;
extern crate futures;
#[macro_use]
extern crate log;
extern crate r2d2;
extern crate serde;
extern crate serde_json;
#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate askama;
extern crate chrono;
extern crate rand;
#[macro_use]
extern crate diesel_derive_enum;

use clap::{App as Clap, AppSettings, Arg, SubCommand};
use std::process;

mod db;
mod flash;
mod models;
mod report;
mod schema;
mod server;
mod time;

// Possible exit codes
const _EXIT_SUCCESS: i32 = 0;
const EXIT_FAILURE: i32 = 1;

const DEFAULT_DATABASE_URL: &str = "punch.db";
const DEFAULT_BIND: &str = "127.0.0.1:8080";
const DEFAULT_STATIC_PATH: &str = "static/";

fn main() {
    // Parse command-line arguments and dispatch
    let database_arg = Arg::with_name("database")
        .short("d")
        .long("database-url")
        .takes_value(true)
        .default_value(DEFAULT_DATABASE_URL)
        .help("Specify the path to the database")
        .required(false);
    let app = Clap::new("Punch time-tracking tool")
        .version("0.1.0")
        .about("Punch in, punch out, and report on time usage.")
        .setting(AppSettings::SubcommandRequiredElseHelp)
        .global_setting(AppSettings::GlobalVersion)
        .global_setting(AppSettings::VersionlessSubcommands)
        .global_setting(AppSettings::UnifiedHelpMessage)
        .subcommand(
            SubCommand::with_name("init")
                .about("Initialize a new Punch instance.")
                .arg(Arg::with_name("username").required(true))
                .arg(Arg::with_name("password").required(true))
                .arg(database_arg.clone()),
        )
        .subcommand(
            SubCommand::with_name("testdb")
                .about("Create a new Punch database populated with test data.")
                .arg(Arg::with_name("username").required(true))
                .arg(Arg::with_name("password").required(true))
                .arg(database_arg.clone()),
        )
        .subcommand(
            SubCommand::with_name("report")
                .about("Display a summary report.")
                .arg(database_arg.clone()),
        )
        .subcommand(
            SubCommand::with_name("server")
                .about("Start the web server")
                .arg(
                    Arg::with_name("bind")
                        .short("b")
                        .long("bind")
                        .takes_value(true)
                        .default_value(DEFAULT_BIND)
                        .help("Specify the ip:port for binding.")
                        .required(false),
                )
                .arg(
                    Arg::with_name("static_path")
                        .short("s")
                        .long("static-path")
                        .takes_value(true)
                        .default_value(DEFAULT_STATIC_PATH)
                        .help("Path to static resources.")
                        .required(false),
                )
                .arg(database_arg),
        );
    let mut app_clone = app.clone();
    let matches = app.get_matches();
    match matches.subcommand() {
        ("init", Some(m)) => cmd_init(
            m.value_of("database").unwrap(),
            m.value_of("username").unwrap(),
            m.value_of("password").unwrap(),
        ),
        ("testdb", Some(m)) => cmd_testdb(
            m.value_of("database").unwrap(),
            m.value_of("username").unwrap(),
            m.value_of("password").unwrap(),
        ),
        ("report", Some(m)) => cmd_report(m.value_of("database").unwrap()),
        ("server", Some(m)) => cmd_server(
            m.value_of("database").unwrap(),
            m.value_of("bind").unwrap(),
            m.value_of("static_path").unwrap(),
        ),
        _ => {
            app_clone.print_help().unwrap();
            println!();
            process::exit(EXIT_FAILURE);
        }
    };

    std::process::exit(1);
}

/// Initialize a new punch instance.
fn cmd_init(database: &str, username: &str, password: &str) {
    db::database_setup(database, username, password).unwrap();
}

/// Initialize a new punch instance, and populate the database with random test data.
fn cmd_testdb(database: &str, username: &str, password: &str) {
    db::database_setup_test(database, username, password).unwrap();
}

/// Show the current summary report on standard output.
fn cmd_report(database: &str) {
    print!("{}", db::do_report(database).unwrap());
}

/// Run the web server.
fn cmd_server(database: &str, bind: &str, static_path: &str) {
    ::std::env::set_var("RUST_LOG", "actix=info,actix_web=info,punch=trace");
    server::do_server(database, bind, static_path);
}
