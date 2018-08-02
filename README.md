# punch-web

Punch is a minimally viable time tracking web app.  *Very* minimally viable.  In fact, all it
does at this time is allow the user to punch-in, punch-out, and show a report of accumulated
hours over recent days and weeks.

![](https://raw.githubusercontent.com/simmons/punch/master/screenshot.png)

Punch has the notion of "gross time" versus "net time", the former being the total time elapsed
between punch-in and punch-out events, and the latter subtracting a fixed amount of overhead
time per work session (currently 15 minutes) to account for the ramp-up period before one is
productive after starting work for the day or after an interruption.

Punch is written in [Rust](https://www.rust-lang.org/) using the [actix-web](https://actix.rs/)
web framework, [Diesel](http://diesel.rs/) for database ORM, and numerous other crates.  When
you build punch with "cargo build", be sure to have the SQLite libraries installed on your
system.  On Ubuntu, for example:
```rust
apt install sqlite3 libsqlite3 libsqlite3-dev
```

To start using punch, first initialize its SQLite database with a username and password for the
user:
```rust
punch-web init --database-url=/path/to/punch.db myusername mypassword
```
The `--database-url` argument is optional, and defaults to "punch.db" in the current directory.

To run the web server, use the "server" subcommand:
```rust
punch-web server --bind 127.0.0.1:8080 \
    --database-url=/path/to/punch.db --static-path=/path/to/static/files
```
The bind address and port defaults to `127.0.0.1:8080`, the database URL defaults once again
to "punch.db" in the current directory, and the path to static resources defaults to "static/"
in the current directory.

### Ideas for future improvements

For a glorified notepad with aspirations of being a time tracker, what *couldn't* be improved?
A few possible ideas are:

* Support adding text notes to punch-in and punch-out events.  Also support a "note" event for
adding timestamped notes without punching in or out.
* Support multiple projects and users.  The database schema is in place for this, but this
minimally viable code currently looks for a singleton user and project.
* Dates are always stored in the database as UTC, but we currently use the server's local time
zone when interpreting dates.  This may or may not be the user's preferred time zone.  We
should support per-user or per-project configurable time zones.
* A proper frontend with AJAX calls could lead to a cleaner implementation, at the expense of
having to develop such frontend code.  (For example, this could avoid the hokey system of
storing error messages in a cookie to survive the redirect after a form post.)
* Numerous per-project parameters could be added to alter time accounting.  For example:
  * Configurable overhead time.
  * Rounding time up, down, or to the nearest hour (or half hour, quarter hour, etc.) on a
    per-session, per-week, or per-day basis.
  * Accumulation of "vacation" time at specified rates to allow the user to reward himself or
  herself after logging enough productive time.
* A command-line interface, which could be implemented as HTTP client calls to REST endpoints.
* More reports.

### License

Punch is distributed under the terms of both the MIT license and the
Apache License (Version 2.0).  See LICENSE-APACHE and LICENSE-MIT for details.

#### Contributing

Unless you explicitly state otherwise, any contribution you intentionally submit
for inclusion in the work, as defined in the Apache-2.0 license, shall be
dual-licensed as above, without any additional terms or conditions.
