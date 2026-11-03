\set ON_ERROR_STOP on
BEGIN;
CREATE ROLE whorl_queue_spike_enqueue NOLOGIN;
GRANT USAGE ON SCHEMA queue_spike TO whorl_queue_spike_enqueue;
GRANT SELECT, INSERT ON queue_spike._private_tasks, queue_spike._private_job_queues TO whorl_queue_spike_enqueue;
GRANT INSERT ON queue_spike._private_job_retirements TO whorl_queue_spike_enqueue;
GRANT SELECT, INSERT, UPDATE ON queue_spike._private_jobs TO whorl_queue_spike_enqueue;
GRANT USAGE ON ALL SEQUENCES IN SCHEMA queue_spike TO whorl_queue_spike_enqueue;
CREATE POLICY spike_enqueue_tasks ON queue_spike._private_tasks TO whorl_queue_spike_enqueue USING (true) WITH CHECK (true);
CREATE POLICY spike_enqueue_queues ON queue_spike._private_job_queues TO whorl_queue_spike_enqueue USING (true) WITH CHECK (true);
CREATE POLICY spike_enqueue_jobs ON queue_spike._private_jobs TO whorl_queue_spike_enqueue USING (true) WITH CHECK (true);
CREATE POLICY spike_enqueue_retirements ON queue_spike._private_job_retirements TO whorl_queue_spike_enqueue USING (true) WITH CHECK (true);
SET LOCAL ROLE whorl_queue_spike_enqueue;
SELECT (queue_spike.add_job('privilege_probe', '{}'::json, queue_name := 'privilege_queue', job_key := 'privilege-key')).id;
SELECT (queue_spike.add_job('privilege_probe', '{}'::json, queue_name := 'privilege_queue', job_key := 'privilege-key')).id;
DO $$ BEGIN
    IF has_schema_privilege(current_user, 'queue_spike', 'CREATE') THEN
        RAISE EXCEPTION 'enqueue role unexpectedly owns DDL';
    END IF;
    IF has_table_privilege(current_user, 'queue_spike._private_jobs', 'DELETE') THEN
        RAISE EXCEPTION 'enqueue role unexpectedly can delete jobs';
    END IF;
END $$;
ROLLBACK;
