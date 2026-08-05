CREATE TABLE projects (
    id           INTEGER PRIMARY KEY,
    path         TEXT NOT NULL UNIQUE,
    name         TEXT NOT NULL,
    display_name TEXT,
    icon         TEXT,
    selected     INTEGER NOT NULL DEFAULT 0 CHECK (selected IN (0, 1))
);

CREATE TABLE agent_tools (
    id        INTEGER PRIMARY KEY,
    name      TEXT NOT NULL UNIQUE,
    command   TEXT NOT NULL,
    tool_type TEXT NOT NULL,
    enabled   INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1))
);

CREATE TABLE processes (
    id                       INTEGER PRIMARY KEY,
    project_id               INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    kind                     TEXT NOT NULL CHECK (kind IN ('command', 'terminal', 'agent')),
    name                     TEXT NOT NULL,
    command                  TEXT,
    working_dir              TEXT NOT NULL,
    env                      TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(env)),
    auto_start               INTEGER NOT NULL DEFAULT 0 CHECK (auto_start IN (0, 1)),
    auto_restart             INTEGER NOT NULL DEFAULT 0 CHECK (auto_restart IN (0, 1)),
    restart_when_changed     TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(restart_when_changed)),
    source                   TEXT NOT NULL CHECK (source IN ('yml', 'local')),
    trust_hash               TEXT,
    status                   TEXT NOT NULL CHECK (status IN ('stopped', 'starting', 'running', 'exited', 'crashed')),
    pid                      INTEGER,
    exit_code                INTEGER,
    exit_signal              INTEGER,
    exited_at                INTEGER,
    agent_tool_id            INTEGER REFERENCES agent_tools(id) ON DELETE SET NULL,
    UNIQUE (project_id, name)
);

CREATE INDEX processes_project_id_idx ON processes(project_id);
CREATE INDEX processes_agent_tool_id_idx ON processes(agent_tool_id);

CREATE TABLE todos (
    id          INTEGER PRIMARY KEY,
    project_id  INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    title       TEXT NOT NULL,
    body        TEXT NOT NULL DEFAULT '',
    status      TEXT NOT NULL CHECK (status IN ('open', 'in_progress', 'backlog', 'completed')),
    priority    TEXT NOT NULL CHECK (priority IN ('high', 'medium', 'low')),
    completed   INTEGER NOT NULL DEFAULT 0 CHECK (completed IN (0, 1)),
    lock_actor  TEXT,
    lock_expiry INTEGER
);

CREATE INDEX todos_project_id_idx ON todos(project_id);
CREATE INDEX todos_status_idx ON todos(project_id, status);

CREATE TABLE todo_tags (
    todo_id INTEGER NOT NULL REFERENCES todos(id) ON DELETE CASCADE,
    tag     TEXT NOT NULL,
    position INTEGER NOT NULL CHECK (position >= 0),
    PRIMARY KEY (todo_id, tag),
    UNIQUE (todo_id, position)
);

CREATE INDEX todo_tags_tag_idx ON todo_tags(tag);

CREATE TABLE todo_blockers (
    todo_id            INTEGER NOT NULL REFERENCES todos(id) ON DELETE CASCADE,
    blocked_by_todo_id INTEGER NOT NULL REFERENCES todos(id) ON DELETE CASCADE,
    PRIMARY KEY (todo_id, blocked_by_todo_id),
    CHECK (todo_id <> blocked_by_todo_id)
);

CREATE INDEX todo_blockers_blocked_by_idx ON todo_blockers(blocked_by_todo_id);

CREATE TABLE todo_comments (
    id         INTEGER PRIMARY KEY,
    todo_id    INTEGER NOT NULL REFERENCES todos(id) ON DELETE CASCADE,
    actor      TEXT NOT NULL,
    body       TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX todo_comments_todo_id_idx ON todo_comments(todo_id, created_at);

CREATE TABLE scratchpads (
    id         INTEGER PRIMARY KEY,
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    content    TEXT NOT NULL DEFAULT '',
    revision   INTEGER NOT NULL DEFAULT 0 CHECK (revision >= 0),
    archived   INTEGER NOT NULL DEFAULT 0 CHECK (archived IN (0, 1)),
    UNIQUE (project_id, name)
);

CREATE INDEX scratchpads_project_id_idx ON scratchpads(project_id);

CREATE TABLE scratchpad_tags (
    scratchpad_id INTEGER NOT NULL REFERENCES scratchpads(id) ON DELETE CASCADE,
    tag           TEXT NOT NULL,
    position      INTEGER NOT NULL CHECK (position >= 0),
    PRIMARY KEY (scratchpad_id, tag),
    UNIQUE (scratchpad_id, position)
);

CREATE INDEX scratchpad_tags_tag_idx ON scratchpad_tags(tag);

CREATE TABLE locks (
    project_id  INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    key         TEXT NOT NULL,
    owner_actor TEXT NOT NULL,
    acquired_at INTEGER NOT NULL,
    ttl         INTEGER NOT NULL CHECK (ttl > 0),
    PRIMARY KEY (project_id, key)
);

CREATE TABLE timers (
    id                  INTEGER PRIMARY KEY,
    owner_actor         TEXT NOT NULL,
    delivery_process_id INTEGER NOT NULL REFERENCES processes(id) ON DELETE CASCADE,
    body                TEXT NOT NULL,
    kind                TEXT NOT NULL CHECK (kind IN ('delay', 'idle_any', 'idle_all')),
    watch_list          TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(watch_list)),
    interval            INTEGER,
    loop                INTEGER NOT NULL DEFAULT 0 CHECK (loop IN (0, 1)),
    max_wait_deadline   INTEGER,
    paused              INTEGER NOT NULL DEFAULT 0 CHECK (paused IN (0, 1)),
    fired               INTEGER NOT NULL DEFAULT 0 CHECK (fired IN (0, 1)),
    fired_at            INTEGER,
    created_at          INTEGER NOT NULL
);

CREATE INDEX timers_delivery_process_id_idx ON timers(delivery_process_id);
CREATE INDEX timers_due_idx ON timers(paused, fired, max_wait_deadline);

CREATE TABLE actors (
    id                  TEXT PRIMARY KEY,
    session_id          TEXT NOT NULL UNIQUE,
    process_id          INTEGER REFERENCES processes(id) ON DELETE SET NULL,
    selected_project_id INTEGER REFERENCES projects(id) ON DELETE SET NULL,
    created_at          INTEGER NOT NULL,
    last_seen_at        INTEGER NOT NULL
);

CREATE INDEX actors_process_id_idx ON actors(process_id);
