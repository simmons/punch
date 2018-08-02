CREATE TABLE config (
    id INTEGER NOT NULL PRIMARY KEY,
    secret BLOB NOT NULL, -- 32-byte secret used to encrypt cookies
    CHECK (id == 1) -- only allow a single row
);

CREATE TABLE users (
    id INTEGER NOT NULL PRIMARY KEY,
    name VARCHAR UNIQUE NOT NULL,
    password VARCHAR(60) UNIQUE,
    admin BOOLEAN NOT NULL DEFAULT 0
);

CREATE TABLE projects (
    id INTEGER NOT NULL PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id),
    name VARCHAR UNIQUE NOT NULL,
    overhead INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE events (
    id INTEGER NOT NULL PRIMARY KEY,
    project_id INTEGER NOT NULL REFERENCES projects(id),
    event_type TEXT CHECK(event_type IN ('in', 'out', 'note')) NOT NULL,
    clock DATETIME NOT NULL
);
