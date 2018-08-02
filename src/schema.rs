use models::EventTypeMapping;

table! {
    config (id) {
        id -> BigInt,
        secret -> Binary,
    }
}

table! {
    use diesel::sql_types::{BigInt,Timestamp};
    use super::EventTypeMapping;
    events (id) {
        id -> BigInt,
        project_id -> BigInt,
        event_type -> EventTypeMapping,
        clock -> Timestamp,
    }
}

table! {
    projects (id) {
        id -> BigInt,
        user_id -> BigInt,
        name -> Text,
        overhead -> Integer,
    }
}

table! {
    users (id) {
        id -> BigInt,
        name -> Text,
        password -> Nullable<Text>,
        admin -> Bool,
    }
}

joinable!(events -> projects (project_id));
joinable!(projects -> users (user_id));

allow_tables_to_appear_in_same_query!(config, events, projects, users,);
