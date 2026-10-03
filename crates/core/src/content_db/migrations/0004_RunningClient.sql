-- Running game clients. Content of a version in use must not be removed.
CREATE TABLE RunningClient(
    ProcessId INTEGER PRIMARY KEY NOT NULL,
    -- Main module of the process, to tell a reused PID from the real client.
    MainModule TEXT NOT NULL,
    UsedVersion INTEGER NOT NULL REFERENCES ContentVersion(Id) ON DELETE RESTRICT
);
