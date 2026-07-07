-- Schedules can now carry their own recurring cost budget directly (previously
-- only reachable indirectly through an ink's `budget_cost_usd`).
ALTER TABLE schedules ADD COLUMN budget_cost_usd REAL;
