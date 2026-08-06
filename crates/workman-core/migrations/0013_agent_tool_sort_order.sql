ALTER TABLE agent_tools ADD COLUMN sort_order INTEGER NOT NULL DEFAULT 0 CHECK (sort_order >= 0);

UPDATE agent_tools
SET sort_order = (
    SELECT COUNT(*)
    FROM agent_tools AS earlier
    WHERE earlier.id < agent_tools.id
);

CREATE INDEX agent_tools_sort_order_idx ON agent_tools(sort_order, id);
