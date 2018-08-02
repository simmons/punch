use actix::prelude::*;
use bcrypt;
use chrono;
use diesel;
use diesel::prelude::*;
use diesel::r2d2::{ConnectionManager, CustomizeConnection, Pool};
use diesel_migrations;
use r2d2;

use models::{self, PunchDirection};
use report::SummaryReport;
use schema;
use time::*;

const NUM_DB_CONNECTIONS: u32 = 3;
const NUM_SYNC_THREADS: usize = 3;

// TODO: Use transactions.

// When the bcrypt crate is compiled in debug, it is excruciatingly slow.  So we'll use a cost of 6
// for development, which is not very secure, but a more reasonable cost of 12 for production.
#[cfg(debug_assertions)]
const BCRYPT_COST: u32 = 6;
#[cfg(not(debug_assertions))]
const BCRYPT_COST: u32 = 12;

#[derive(Fail, Debug)]
pub enum DatabaseError {
    #[fail(display = "Database error: {}", _0)]
    Diesel(diesel::result::Error),
    #[fail(display = "Password error: {}", _0)]
    Password(bcrypt::BcryptError),
    #[fail(display = "Transaction error: Inconsistent State")]
    BadState,
    #[fail(display = "Bad time encountered")]
    BadTime,
    #[fail(display = "Project not found")]
    BadProject,
}
impl From<diesel::result::Error> for DatabaseError {
    fn from(e: diesel::result::Error) -> DatabaseError {
        DatabaseError::Diesel(e)
    }
}
impl From<bcrypt::BcryptError> for DatabaseError {
    fn from(e: bcrypt::BcryptError) -> DatabaseError {
        DatabaseError::Password(e)
    }
}

/// The sync actor responsible for accessing the database.
pub struct DbExecutor(pub Pool<ConnectionManager<SqliteConnection>>);

impl Actor for DbExecutor {
    type Context = SyncContext<Self>;
}

/// Retrieve the row id of the last insert.
fn last_insert_rowid(connection: &SqliteConnection) -> i64 {
    no_arg_sql_function!(last_insert_rowid, diesel::sql_types::BigInt);
    diesel::select(last_insert_rowid)
        .first::<i64>(connection)
        .unwrap()
}

/// This customizes Sqlite connections from the R2D2 pool such that foreign keys are enabled.
#[derive(Debug)]
struct SqliteConnectionCustomizer {}

impl<C> CustomizeConnection<C, diesel::r2d2::Error> for SqliteConnectionCustomizer
where
    C: diesel::connection::Connection,
{
    fn on_acquire(&self, conn: &mut C) -> Result<(), diesel::r2d2::Error> {
        conn.execute("PRAGMA foreign_keys = ON")
            .map(|_| ())
            .map_err(|e| diesel::r2d2::Error::QueryError(e))
    }
}

/// Create a pool of connections to the database.
fn database_pool(
    database: &str,
) -> r2d2::Pool<diesel::r2d2::ConnectionManager<diesel::SqliteConnection>> {
    // Create an R2D2 pool
    let manager = ConnectionManager::<SqliteConnection>::new(database);
    r2d2::Pool::builder()
        .max_size(NUM_DB_CONNECTIONS)
        .connection_customizer(Box::new(SqliteConnectionCustomizer {}))
        .build(manager)
        .expect("Failed to create pool.")
}

/// Perform migrations to update the database's schema, if needed.
fn database_migrate(connection: &impl diesel_migrations::MigrationConnection) {
    // Allowing unused_imports is only needed to avoid a warning until
    // https://github.com/diesel-rs/diesel/issues/1739
    // is rolled into a stable diesel version.
    #[allow(unused_imports)]
    {
        embed_migrations!();
        embedded_migrations::run(connection).unwrap();
    }
}

const DEFAULT_OVERHEAD_MINUTES: i32 = 15;

/// Initialize a new punch database.
pub fn database_setup(database: &str, username: &str, password: &str) -> Result<(), DatabaseError> {
    use self::schema::projects::dsl as projects_dsl;
    use self::schema::users::dsl as users_dsl;

    let pool = database_pool(database);
    let connection = pool.get().unwrap();
    database_migrate(&connection);

    // Is the database already set up?
    let admin_users = users_dsl::users
        .filter(users_dsl::admin.eq(true))
        .limit(1)
        .load::<models::User>(&connection)?
        .len();
    if admin_users > 0 {
        panic!("Database is already set up.  (One or more admin users exist.)");
    }

    // Create the initial user
    let hashed_password = match bcrypt::hash(password, BCRYPT_COST) {
        Ok(h) => h,
        Err(e) => {
            error!("{}", e);
            panic!("Cannot bcrypt-hash password: {}", e);
        }
    };
    let new_user = models::NewUser {
        name: username,
        password: Some(&hashed_password),
        admin: true,
    };
    diesel::insert_into(users_dsl::users)
        .values(&new_user)
        .execute(&connection)?;

    // Fetch the newly created user
    let rowid = last_insert_rowid(&connection);
    let new_user = users_dsl::users
        .filter(users_dsl::id.eq(rowid as i64))
        .first::<models::User>(&connection)?;

    // Create the initial project
    let new_project = models::NewProject {
        user_id: new_user.id,
        name: "Project",
        overhead: DEFAULT_OVERHEAD_MINUTES,
    };
    diesel::insert_into(projects_dsl::projects)
        .values(&new_project)
        .execute(&connection)?;

    Ok(())
}

/// Initialize a new punch database, and populate it with random test data.
pub fn database_setup_test(
    database: &str,
    username: &str,
    password: &str,
) -> Result<(), DatabaseError> {
    database_setup(database, username, password)?;

    let pool = database_pool(database);
    let connection = pool.get().unwrap();
    let user = load_singleton_user(&connection)?;
    let project = load_project_for_user(&connection, user.id)?;

    use chrono::offset::Local;
    use chrono::{Datelike, Duration, NaiveDateTime, NaiveTime, Weekday};
    use models::{EventType, NewEvent};
    use rand::{self, Rng, XorShiftRng};

    const RNG_SEED: [u8; 16] = [
        0x04, 0xC1, 0x1D, 0xB7, 0x1E, 0xDC, 0x6F, 0x41, 0x74, 0x1B, 0x8C, 0xD7, 0x32, 0x58, 0x34,
        0x99,
    ];
    let mut rng: XorShiftRng = rand::SeedableRng::from_seed(RNG_SEED);

    const START_DAYS_IN_PAST: i64 = 38;
    const MIN_SESSION: i64 = 60 * 60;
    const MAX_SESSION: i64 = 60 * 60 * 6;
    const MIN_TIME_PER_DAY: i64 = 60 * 60 * 7;
    const MAX_FUZZ_TIME: i64 = 3600;
    let earliest_start_time = NaiveTime::from_num_seconds_from_midnight(60 * 60 * 7, 0); // 7:00am

    // Determine the Monday at or before 38 days ago.
    let today = Local::now().naive_local().date();
    let mut day = today - Duration::days(START_DAYS_IN_PAST);
    while day.weekday() != Weekday::Mon {
        day -= Duration::days(1);
    }

    while day < today {
        println!("day: {}", day);

        // Seldom work on weekends.
        if day.weekday() == Weekday::Sat || day.weekday() == Weekday::Sun {
            // 30% chance of working
            if rng.gen_range(0, 100) >= 30 {
                day += Duration::days(1);
                continue;
            }
        }

        let mut time_today: i64 = 0;
        let mut tod = earliest_start_time;
        while time_today < MIN_TIME_PER_DAY {
            // Fuzz start time
            let seconds_left_in_day = (NaiveTime::from_hms(23, 59, 59) - tod).num_seconds();
            let max_fuzz_time = MAX_FUZZ_TIME.min(seconds_left_in_day);
            tod += Duration::seconds(rng.gen_range(0, max_fuzz_time));
            let start_time = tod;

            // Calculate session length
            let seconds_left_in_day = (NaiveTime::from_hms(23, 59, 59) - tod).num_seconds();
            if seconds_left_in_day < 60 {
                break;
            }
            let max_session = MAX_SESSION.min(seconds_left_in_day);
            if max_session <= MIN_SESSION {
                break;
            }
            let length = rng.gen_range(MIN_SESSION, max_session);

            // Calculate end time
            tod += Duration::seconds(length);
            let end_time = tod;

            // Create events
            let punch_in = NewEvent {
                project_id: project.id,
                event_type: EventType::In,
                clock: to_utc(&NaiveDateTime::new(day, start_time))?,
            };
            let punch_out = NewEvent {
                project_id: project.id,
                event_type: EventType::Out,
                clock: to_utc(&NaiveDateTime::new(day, end_time))?,
            };

            // Persist
            use self::schema::events::dsl::*;
            diesel::insert_into(events)
                .values(&punch_in)
                .execute(&connection)?;
            diesel::insert_into(events)
                .values(&punch_out)
                .execute(&connection)?;

            println!(
                "\t{} -> {} ({:.2} hours)",
                start_time,
                end_time,
                (length as f64) / 60.0 / 60.0
            );

            time_today += length;
        }
        println!(
            "\tTotal hours for day: {:.2} hours",
            (time_today as f64) / 60.0 / 60.0
        );

        day += Duration::days(1);
    }

    Ok(())
}

/// Initialize our database sync actor.
pub fn database_init(
    database: &str,
) -> Result<(actix::Addr<DbExecutor>, models::Config), DatabaseError> {
    let pool = database_pool(database);
    let connection = pool.get().unwrap();
    database_migrate(&connection);
    let config = load_config(&connection)?;
    Ok((
        SyncArbiter::start(NUM_SYNC_THREADS, move || DbExecutor(pool.clone())),
        config,
    ))
}

/// Generate a summary report.  This function opens a fresh database connection, and is meant to be
/// used when generating a text report via the "report" command-line argument.
pub fn do_report(database: &str) -> Result<SummaryReport, DatabaseError> {
    let pool = database_pool(database);
    let connection = pool.get().unwrap();
    let user = load_singleton_user(&connection)?;
    let project = load_project_for_user(&connection, user.id)?;
    ::report::summary_report(&connection, project.id)
}

//////////////////////////////////////////////////////////////////////
// AuthenticateUser
//////////////////////////////////////////////////////////////////////

pub struct AuthenticateUser {
    pub username: String,
    pub password: String,
}
impl Message for AuthenticateUser {
    type Result = Result<bool, DatabaseError>;
}
impl Handler<AuthenticateUser> for DbExecutor {
    type Result = Result<bool, DatabaseError>;

    fn handle(&mut self, msg: AuthenticateUser, _: &mut Self::Context) -> Self::Result {
        use self::schema::users::dsl::*;
        let conn: &SqliteConnection = &self.0.get().unwrap();

        let user = users
            .filter(name.eq(msg.username))
            .first::<models::User>(conn)?;
        match user.password {
            Some(p) => Ok(bcrypt::verify(&msg.password, &p)?),
            None => Ok(false),
        }
    }
}

//////////////////////////////////////////////////////////////////////
// GetConfig
//////////////////////////////////////////////////////////////////////

fn load_config(connection: &SqliteConnection) -> Result<models::Config, DatabaseError> {
    use self::schema::config::dsl::*;
    use models::{Config, ConfigRow};

    let row = config.first::<ConfigRow>(connection).optional()?;
    match row {
        Some(ref row) => {
            let c = match Config::parse_row(row) {
                Ok(c) => c,
                Err(_) => {
                    panic!("Cannot parse configuration");
                }
            };
            Ok(c)
        }
        None => {
            // No config row present -- create a new one.
            let row = ConfigRow::new();
            diesel::insert_into(config)
                .values(&row)
                .execute(connection)?;
            Ok(Config::parse_row(&row).unwrap())
        }
    }
}

pub struct GetConfig {}
impl Message for GetConfig {
    type Result = Result<models::Config, DatabaseError>;
}
impl Handler<GetConfig> for DbExecutor {
    type Result = Result<models::Config, DatabaseError>;

    fn handle(&mut self, _: GetConfig, _: &mut Self::Context) -> Self::Result {
        let conn: &SqliteConnection = &self.0.get().unwrap();
        load_config(conn)
    }
}

//////////////////////////////////////////////////////////////////////
// PunchCommand
//////////////////////////////////////////////////////////////////////

pub struct PunchCommand {
    // project_id: String,
    pub username: String,
    pub direction: PunchDirection,
    pub note: Option<String>,
}
impl Message for PunchCommand {
    type Result = Result<(), DatabaseError>;
}

/// This will load the sole user.  Some day we should support multiple users.
fn load_singleton_user(connection: &SqliteConnection) -> Result<models::User, DatabaseError> {
    use self::schema::users::dsl as users_dsl;
    users_dsl::users
        .order(users_dsl::id)
        .first::<models::User>(connection)
        .map_err(|e| e.into())
}

/// This will load the user's sole project.  Some day we should support multiple projects per user.
fn load_project_for_user(
    connection: &SqliteConnection,
    user_id: i64,
) -> Result<models::Project, DatabaseError> {
    use self::schema::projects::dsl as projects_dsl;
    projects_dsl::projects
        .filter(projects_dsl::user_id.eq(user_id))
        .order(projects_dsl::id)
        .first::<models::Project>(connection)
        .map_err(|e| e.into())
}

/// Determine the next expected punch direction, based on whether the previous punch direction was
/// in, out, or non-existent.
pub fn next_expected_punch_direction(
    connection: &SqliteConnection,
    project_id: i64,
) -> Result<PunchDirection, DatabaseError> {
    use self::schema::events::dsl as events_dsl;
    let last_event = events_dsl::events
        .filter(events_dsl::project_id.eq(project_id))
        .filter(
            events_dsl::event_type
                .eq(models::EventType::In)
                .or(events_dsl::event_type.eq(models::EventType::Out)),
        )
        .order(events_dsl::clock.desc())
        .first::<models::Event>(connection)
        .optional()?;
    let next_direction = match &last_event.map(|e| e.event_type) {
        Some(models::EventType::In) => PunchDirection::Out,
        Some(models::EventType::Out) => PunchDirection::In,
        Some(models::EventType::Note) => unreachable!(),
        None => PunchDirection::In,
    };
    Ok(next_direction)
}

impl Handler<PunchCommand> for DbExecutor {
    type Result = Result<(), DatabaseError>;

    fn handle(&mut self, msg: PunchCommand, _: &mut Self::Context) -> Self::Result {
        use self::schema::events::dsl as events_dsl;
        use self::schema::users::dsl as users_dsl;
        let connection: &SqliteConnection = &self.0.get().unwrap();

        // Load the user and project
        let user = users_dsl::users
            .filter(users_dsl::name.eq(msg.username))
            .first::<models::User>(connection)?;
        let project = load_project_for_user(connection, user.id)?;

        // Confirm that this punch is consistent with the most recent punch.
        if msg.direction != next_expected_punch_direction(connection, project.id)? {
            return Err(DatabaseError::BadState);
        }

        // Create the punch event
        let new_event = models::NewEvent {
            project_id: project.id,
            event_type: msg.direction.into(),
            clock: chrono::offset::Utc::now().naive_utc(),
        };
        diesel::insert_into(events_dsl::events)
            .values(&new_event)
            .execute(connection)?;

        Ok(())
    }
}

//////////////////////////////////////////////////////////////////////
// GetReport
//////////////////////////////////////////////////////////////////////

pub struct GetSummaryReport {}
impl Message for GetSummaryReport {
    type Result = Result<SummaryReport, DatabaseError>;
}
impl Handler<GetSummaryReport> for DbExecutor {
    type Result = Result<SummaryReport, DatabaseError>;

    fn handle(&mut self, _: GetSummaryReport, _: &mut Self::Context) -> Self::Result {
        let connection: &SqliteConnection = &self.0.get().unwrap();
        let user = load_singleton_user(&connection)?;
        let project = load_project_for_user(&connection, user.id)?;
        ::report::summary_report(&connection, project.id)
    }
}
