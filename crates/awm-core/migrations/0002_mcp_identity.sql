CREATE TABLE process_mcp_tokens (
    process_id INTEGER PRIMARY KEY REFERENCES processes(id) ON DELETE CASCADE,
    token      TEXT NOT NULL UNIQUE,
    created_at INTEGER NOT NULL
);

CREATE INDEX process_mcp_tokens_token_idx ON process_mcp_tokens(token);
