CREATE TABLE IF NOT EXISTS script_infos (
    id integer PRIMARY KEY NOT NULL,
    name text NOT NULL,
    category varchar(10) NOT NULL,
    tags text NOT NULL,
    created_time timestamp NOT NULL DEFAULT (datetime ('now', 'localtime'))
);
