use chrono::NaiveDateTime;

use super::schema::config;
use super::schema::events;
use super::schema::projects;
use super::schema::users;

//////////////////////////////////////////////////////////////////////
// Configuration
//////////////////////////////////////////////////////////////////////

const CONFIG_FIXED_ID: i64 = 1;

#[derive(Queryable, Insertable)]
#[table_name = "config"]
pub struct ConfigRow {
    pub id: i64, // always 1
    pub secret: Vec<u8>,
}

impl ConfigRow {
    pub fn new() -> Self {
        ConfigRow {
            id: CONFIG_FIXED_ID,
            secret: Secret::generate().into(),
        }
    }

    pub fn sanitize(&mut self) {
        for i in 0..self.secret.len() {
            self.secret[i] = 0;
        }
    }
}

pub struct Config {
    pub secret: Secret,
}

impl Config {
    pub fn parse_row(config_row: &ConfigRow) -> Result<Config, ()> {
        if config_row.secret.len() != SECRET_KEY_SIZE {
            return Err(());
        }
        let mut secret_key: [u8; 32] = [0; 32];
        secret_key.copy_from_slice(&config_row.secret);
        Ok(Config {
            secret: Secret { data: secret_key },
        })
    }
}

const SECRET_KEY_SIZE: usize = 32;

pub struct Secret {
    pub data: [u8; SECRET_KEY_SIZE],
}

impl Secret {
    fn generate() -> Secret {
        Secret {
            data: ::rand::random(),
        }
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        for i in 0..SECRET_KEY_SIZE {
            self.data[i] = 0;
        }
    }
}

impl From<Secret> for Vec<u8> {
    fn from(secret: Secret) -> Vec<u8> {
        secret.data.to_vec()
    }
}

//////////////////////////////////////////////////////////////////////
// Model
//////////////////////////////////////////////////////////////////////

#[derive(Serialize, Queryable)]
pub struct User {
    pub id: i64,
    pub name: String,
    pub password: Option<String>,
    pub admin: bool,
}

#[derive(Insertable)]
#[table_name = "users"]
pub struct NewUser<'a> {
    pub name: &'a str,
    pub password: Option<&'a str>,
    pub admin: bool,
}

#[derive(Queryable)]
pub struct Project {
    pub id: i64,
    pub user_id: i64,
    pub name: String,
    pub overhead: i32,
}

#[derive(Insertable)]
#[table_name = "projects"]
pub struct NewProject<'a> {
    pub user_id: i64,
    pub name: &'a str,
    pub overhead: i32,
}

#[derive(DbEnum, Debug, PartialEq, Clone)]
pub enum EventType {
    In,
    Out,
    Note,
}

/// PunchDirection is effectively a subset of EventType that only includes in and out types.
#[derive(Deserialize, Debug, PartialEq)]
pub enum PunchDirection {
    In,
    Out,
}
impl From<PunchDirection> for EventType {
    fn from(direction: PunchDirection) -> EventType {
        match direction {
            PunchDirection::In => EventType::In,
            PunchDirection::Out => EventType::Out,
        }
    }
}

#[derive(Queryable, Debug, PartialEq, Clone)]
pub struct Event {
    pub id: i64,
    pub project_id: i64,
    pub event_type: EventType,
    // We store date/time as UTC without a time zone (a NaiveDateTime), because that's what Diesel
    // supports out of the box.  This is less than ideal.  In the future, this should be refactored
    // to provide custom row deserialization to convert the database value into a DateTime
    // reflecting UTC, to reduce the likelihood of time zone mistakes.
    // Also, we are currently assuming the server's local time zone is the user's preferred time
    // zone for the purposes of allocating work intervals to days and weeks.  We should instead
    // allow per-user or per-project time zones.
    pub clock: NaiveDateTime,
}

#[derive(Insertable)]
#[table_name = "events"]
pub struct NewEvent {
    pub project_id: i64,
    pub event_type: EventType,
    pub clock: NaiveDateTime,
}
