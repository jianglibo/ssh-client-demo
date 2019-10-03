use super::{DbAccess, DbAction, RemoteFileItemInDb};
use chrono::{DateTime, Utc};
use failure;
use log::*;
use r2d2;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{params, Row, NO_PARAMS};
use std::path::Path;

pub type SqlitePool = r2d2::Pool<SqliteConnectionManager>;

#[derive(Debug)]
pub struct SqliteDbAccess(SqlitePool);

impl Clone for SqliteDbAccess {
    fn clone(&self) -> SqliteDbAccess {
        SqliteDbAccess(self.0.clone())
    }
}

impl SqliteDbAccess {
    pub fn new(db_file: impl AsRef<Path>) -> Self {
        let db_file = db_file.as_ref();
        let manager = SqliteConnectionManager::file(db_file)
            .with_init(|c| c.execute_batch("PRAGMA foreign_keys=1;"));
        let pool = r2d2::Pool::new(manager).unwrap();
        Self(pool)
    }
    #[allow(dead_code)]
    pub fn new_mem() -> Self {
        let manager = SqliteConnectionManager::memory()
            .with_init(|c| c.execute_batch("PRAGMA foreign_keys=1;"));
        let pool = r2d2::Pool::new(manager).unwrap();
        Self(pool)
    }

    pub fn get_pool(&self) -> &SqlitePool {
        &self.0
    }
}

fn map_to_file_item(row: &Row) -> Result<RemoteFileItemInDb, rusqlite::Error> {
    Ok(RemoteFileItemInDb {
        id: row.get(0)?,
        path: row.get(1)?,
        sha1: row.get(2)?,
        len: row.get(3)?,
        modified: row.get(4)?,
        created: row.get(5)?,
        dir_id: row.get(6)?,
        changed: row.get(7)?,
    })
}

impl DbAccess<SqliteConnectionManager> for SqliteDbAccess {
    fn insert_directory(&self, path: impl AsRef<str>) -> Result<i64, failure::Error> {
        let conn = self.get_pool().get().unwrap();
        let path = path.as_ref();
        if let Err(err) = conn.execute("INSERT INTO directory (path) VALUES (?1)", params![path]) {
            if let rusqlite::Error::SqliteFailure(_, Some(desc)) = &err {
                if desc.contains("UNIQUE constraint failed") {
                    warn!("{}", err);
                    return self.find_directory(path);
                } else {
                    bail!("{}", err);
                }
            } else {
                bail!("{}", err);
            }
        }
        Ok(conn.last_insert_rowid())
    }

    fn find_directory(&self, path: impl AsRef<str>) -> Result<i64, failure::Error> {
        let conn = self.0.get().unwrap();
        let mut stmt = conn.prepare("SELECT id FROM directory where path = ?1")?;
        Ok(stmt.query_row(params![path.as_ref()], |row| row.get(0))?)
    }

    fn get_file_item(&self, num: usize) -> Result<Vec<RemoteFileItemInDb>, failure::Error> {
        let conn = self.0.get().unwrap();
        let mut stmt = conn.prepare("SELECT id, path, sha1, len, time_modified, time_created, dir_id, changed FROM remote_file_item LIMIT :num")?;
        let r = stmt
            .query_map_named(
                named_params! {
                    ":num": num as i64,
                },
                map_to_file_item,
            )?
            .filter_map(|it| match it {
                Ok(it) => Some(it),
                Err(err) => {
                    error!("get_file_item: {:?}", err);
                    None
                }
            })
            .collect();
        Ok(r)
    }

    fn find_remote_file_item(
        &self,
        dir_id: i64,
        path: impl AsRef<str>,
    ) -> Result<RemoteFileItemInDb, failure::Error> {
        let conn = self.0.get().unwrap();
        let path = path.as_ref();
        let r = match conn.prepare("SELECT id, path, sha1, len, time_modified, time_created, dir_id, changed FROM remote_file_item where dir_id = :dir_id and path = :path") {
            Ok(mut stmt) => {
        match stmt.query_row_named(
            named_params! {
                ":dir_id": dir_id,
                ":path": path,
            },
            map_to_file_item
        ){
            Ok(r) => r,
            Err(e) => {
                trace!("find file failed: {:?}", e);
                bail!("find file failed: {:?}", e);
            }
        }

            }
            Err(e) => {
                trace!("prepare find stmt failed: {:?}", e);
                bail!("prepare find stmt failed: {:?}", e);

            }
        };
        Ok(r)
    }

    fn iterate_all_file_items<P>(&self, _processor: P) -> (usize, usize)
    where
        P: Fn(RemoteFileItemInDb) -> (),
    {
        let _conn = self.0.get().unwrap();
        // let mut stmt = conn.prepare("SELECT id, path, sha1, len, time_modified, time_created, dir_id FROM directory where dir_id = :dir_id and path = :path")?;
        (0, 0)
    }

    fn iterate_files_by_directory<F>(&self, mut processor: F) -> Result<(), failure::Error>
    where
        F: FnMut((Option<RemoteFileItemInDb>, Option<String>)) -> (),
    {
        let conn = self.get_pool().get().unwrap();
        let mut stmt = conn.prepare("SELECT id, path FROM directory")?;
        let dirs = stmt.query_map(NO_PARAMS, |row| Ok((row.get(0)?, row.get(1)?)))?;

        for dir in dirs {
            let dir = dir?;
            let dir_id: i64 = dir.0;
            let path = dir.1;
            processor((None, Some(path)));
            let mut stmt = conn.prepare("SELECT id, path, sha1, len, time_modified, time_created, dir_id, changed FROM remote_file_item where dir_id = :dir_id")?;
            let files = stmt.query_map_named(
                named_params! {
                    ":dir_id": dir_id
                },
                map_to_file_item,
            )?;

            files
                .filter_map(|fi| fi.ok())
                .for_each(|fi| processor((Some(fi), None)));
        }

        Ok(())
    }

    fn count_directory(&self) -> Result<u64, failure::Error> {
        let conn = self.0.get().unwrap();
        let mut stmt = conn.prepare("SELECT count(id) FROM directory")?;
        let i: i64 = stmt.query_row(NO_PARAMS, |row| row.get(0))?;
        Ok(i as u64)
    }

    fn count_remote_file_item(&self, changed: Option<bool>) -> Result<u64, failure::Error> {
        let conn = self.0.get().unwrap();
        let i: i64 = if let Some(b) = changed {
            let mut stmt =
                conn.prepare("SELECT count(id) FROM remote_file_item WHERE changed = :changed")?;
            stmt.query_row_named(
                named_params! {
                    ":changed": b
                },
                |row| row.get(0),
            )?
        } else {
            let mut stmt = conn.prepare("SELECT count(id) FROM remote_file_item")?;
            stmt.query_row(NO_PARAMS, |row| row.get(0))?
        };
        Ok(i as u64)
    }

    fn insert_or_update_remote_file_item(
        &self,
        mut rfi: RemoteFileItemInDb,
        batch: bool,
    ) -> Option<(RemoteFileItemInDb, DbAction)> {
        let conn = self.0.get().unwrap();
        if let Ok(rfi_db) = self.find_remote_file_item(rfi.dir_id, rfi.path.as_str()) {
            if rfi_db.len != rfi.len || rfi_db.sha1 != rfi.sha1 || rfi_db.modified != rfi.modified {
                if !batch {
                    let sql_mark_changed = "UPDATE remote_file_item SET len = :len, sha1 = :sha1, time_modified = :modified, changed = :changed where id = :id";
                    let mut stmt_mark_changed = conn
                        .prepare(sql_mark_changed)
                        .expect("prepare sql_mark_changed failed.");
                    if let Err(err) = stmt_mark_changed.execute_named(named_params! {
                        ":len": rfi.len,
                        ":sha1": rfi.sha1,
                        ":modified": rfi.modified,
                        ":id": rfi_db.id,
                        ":changed": true,
                    }) {
                        error!("update remote file item failed: {:?}", err);
                    } else {
                        trace!("update changed item successfully.");
                    }
                }
                rfi.id = rfi_db.id;
                rfi.changed = true;
                return Some((rfi, DbAction::Update));
            } else if rfi_db.changed {
                if !batch {
                    // make changed unchanged.
                    let sql_unmark_changed =
                        "UPDATE remote_file_item SET changed = :changed where id = :id";
                    let mut stmt_unmark = conn
                        .prepare(sql_unmark_changed)
                        .expect("prepare sql_unmark_changed failed");
                    if let Err(err) = stmt_unmark.execute_named(named_params! {
                        ":id": rfi_db.id,
                        ":changed": false,
                    }) {
                        error!("update remote file item failed: {:?}", err);
                    } else {
                        trace!("update changed item successfully.");
                    }
                }
                rfi.id = rfi_db.id;
                rfi.changed = false;
                return Some((rfi, DbAction::UpdateChangedField));
            }
        } else if !batch {
            let sql_insert = "INSERT INTO remote_file_item (path, sha1, len, time_modified, time_created, dir_id, changed) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)";
            let mut stmt_insert = conn
                .prepare(sql_insert)
                .expect("prepare sql_insert failed.");
            match stmt_insert.execute(params![
                rfi.path,
                rfi.sha1,
                rfi.len,
                rfi.modified,
                rfi.created,
                rfi.dir_id,
                rfi.changed
            ]) {
                Ok(count) => {
                    if count != 1 {
                        error!(
                            "insert remote file failed, execute return not eq to 1. {:?}",
                            rfi
                        );
                    } else {
                        rfi.id = conn.last_insert_rowid();
                        return Some((rfi, DbAction::Insert));
                    }
                }
                Err(e) => error!("insert remote file failed: {:?}, {:?}", rfi, e),
            }
        } else {
            return Some((rfi, DbAction::Insert));
        }
        None
    }

    fn update_next_execute_done(&self, id: i64) -> Result<(), failure::Error> {
        let conn = self.get_pool().get().unwrap();
        let mut stmt = conn.prepare("UPDATE schedule_done set done = :done WHERE id = :id")?;
        stmt.execute_named(named_params! {
            ":done": true,
            ":id": id,
        })?;

        Ok(())
    }

    #[allow(clippy::let_and_return)]
    fn find_next_execute(
        &self,
        server_yml_path: impl AsRef<str>,
        task_name: impl AsRef<str>,
    ) -> Option<(i64, DateTime<Utc>, bool)> {
        let conn = self.get_pool().get().unwrap();
        let r = match conn.prepare("SELECT id, time_execution, done FROM schedule_done
                                         WHERE server_yml_path = :server_yml_path AND task_name = :task_name
                                         ORDER BY time_execution DESC
                                         LIMIT 1") {
            Ok(mut stmt) => {
                stmt.query_map_named(named_params!{
                    ":server_yml_path": server_yml_path.as_ref(),
                    ":task_name": task_name.as_ref(),
                }, |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))
                ).expect("query schedule_done should success.").next().map(|it|it.expect("schedule_done item should success."))
            }
            Err(err) => {
                error!("prepare find_next_execute stmt failed: {:?}", err);
                    None
                }
        };
        r
    }

    fn count_next_execute(&self) -> Result<u64, failure::Error> {
        let conn = self.get_pool().get().unwrap();
        let mut stmt = conn.prepare("SELECT count(id) FROM schedule_done;")?;
        let i: i64 = stmt.query_row(NO_PARAMS, |row| row.get(0))?;
        Ok(i as u64)
    }

    fn delete_next_execute(&self, id: i64) -> Result<(), failure::Error> {
        let conn = self.get_pool().get().unwrap();
        let mut stmt = conn.prepare("DELETE FROM schedule_done WHERE id = :id")?;
        stmt.execute_named(named_params! {
            ":id": id,
        })?;
        Ok(())
    }

    fn insert_next_execute(
        &self,
        server_yml_path: impl AsRef<str>,
        task_name: impl AsRef<str>,
        time_execution: DateTime<Utc>,
    ) {
        let conn = self.get_pool().get().unwrap();
        match conn.prepare("INSERT INTO schedule_done (server_yml_path, task_name, time_execution, done) VALUES (?1, ?2, ?3, ?4)") {
            Ok(mut stmt) => {
                if let Err(err) = stmt.execute(params![server_yml_path.as_ref(), task_name.as_ref(), time_execution, false]) {
                    error!("insert_next_execute failed: {:?}", err);
                }
            },
            Err(err) => error!("prepare insert_next_execute stmt failed: {:?}", err)
        };
    }

    fn execute_batch(&self, sit: impl Iterator<Item = String>) {
        let conn = self.get_pool().get().unwrap();
        let mut ss = sit.fold(String::from("BEGIN;"), |mut acc, ref line| {
            acc.push_str(line);
            acc
        });
        ss.push_str("COMMIT;");
        if let Err(err) = conn.execute_batch(ss.as_str()) {
            eprintln!("{:?}", err);
        }
    }

    fn create_database(&self) -> Result<(), failure::Error> {
        let conn = self.0.get().unwrap();
        conn.execute_batch(
            "BEGIN;
            CREATE TABLE directory (
                id  INTEGER PRIMARY KEY,
                path TEXT NOT NULL UNIQUE
            );
             CREATE TABLE remote_file_item (
                   id              INTEGER PRIMARY KEY,
                   path            TEXT NOT NULL,
                   sha1            TEXT,
                   len             INTEGER DEFAULT 0,
                   time_modified   TEXT,
                   time_created    TEXT,
                   changed         BOOLEAN,
                   dir_id          INTEGER NOT NULL,
                   FOREIGN KEY(dir_id) REFERENCES directory(id)
                   );
             CREATE UNIQUE INDEX dir_path ON remote_file_item (path, dir_id);
             CREATE TABLE schedule_done (
                  id  INTEGER PRIMARY KEY,
                  server_yml_path TEXT NOT NULL,
                  task_name TEXT NOT NULL,
                  time_execution   TEXT,
                  done BOOLEAN,
                  CONSTRAINT server_task_name UNIQUE (server_yml_path, task_name)
                  );
                COMMIT;",
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_shape::{load_remote_item_to_sqlite, Directory};
    use crate::develope::tutil;
    use crate::log_util;
    use chrono::{offset::TimeZone, Utc};
    use itertools::Itertools;
    use rand::distributions::Alphanumeric;
    use rand::{self, Rng};
    use rusqlite::NO_PARAMS;
    use std::fs;
    use std::io::{Read, Write};
    use std::sync::{Arc, Mutex};
    use std::time::Instant;

    fn log() {
        log_util::setup_logger_detail(
            true,
            "output.log",
            vec!["db_accesses::sqlite_access"],
            Some(vec!["ssh2"]),
            "",
        )
        .expect("init log should success.");
    }

    fn rfi_gen(dir_id: i64) -> impl FnMut() -> RemoteFileItemInDb {
        let mut rng = rand::thread_rng();
        let t = Utc.ymd(1970, 1, 1).and_hms(0, 1, 1);
        move || RemoteFileItemInDb {
            id: 0,
            path: std::iter::repeat(())
                .map(|()| rng.sample(Alphanumeric))
                .take(50)
                .collect::<String>(),
            sha1: None,
            len: 55,
            modified: Some(t),
            created: None,
            dir_id,
            changed: false,
        }
    }

    /// batch insert may partly success. it will stop at the first failure.
    /// in the batch proccess, later inserting can't see previous inserting. which may cause unique violation.
    #[test]
    fn t_individual_insert() -> Result<(), failure::Error> {
        log();
        let db_dir = tutil::TestDir::new();
        let db_access = tutil::create_a_sqlite_file_db(&db_dir)?;
        let dir_id = db_access.insert_directory("abc")?;

        let now = Instant::now();
        let _c = std::iter::repeat_with(rfi_gen(dir_id))
            .take(10)
            .enumerate()
            .map(|(idx, mut it)| {
                if idx == 2 {
                    let mut it1 = it.duplicate_self();
                    it1.len = it.len + 1;
                    vec![it, it1]
                } else if idx == 4 {
                    let it1 = it.duplicate_self();
                    it.changed = true;
                    vec![it, it1]
                } else {
                    vec![it]
                }
            })
            .flat_map(|its| its)
            .filter_map(|line| db_access.insert_or_update_remote_file_item(line, false))
            .count();
        eprintln!("elapsed: {}", now.elapsed().as_secs());
        Ok(())
    }

    #[test]
    fn t_batch_insert() -> Result<(), failure::Error> {
        log();
        let db_dir = tutil::TestDir::new();
        let db_access = tutil::create_a_sqlite_file_db(&db_dir)?;
        let dir_id = db_access.insert_directory("abc")?;

        let now = Instant::now();
        std::iter::repeat_with(rfi_gen(dir_id))
            .take(10)
            .filter_map(|it| db_access.insert_or_update_remote_file_item(it, true))
            .map(|(it, da)| {
                let s = it.to_sql_string(&da);
                eprintln!("{:?}, {}", da, s);
                s
            })
            .chunks(100_000)
            .into_iter()
            .for_each(|ck| db_access.execute_batch(ck));

        assert_eq!(db_access.count_remote_file_item(None)?, 10);

        let mut rfis = db_access.get_file_item(2)?;
        rfis.get_mut(0).unwrap().len = 333;

        assert!(rfis.get(1).unwrap().changed);

        rfis.into_iter()
            .filter_map(|it| db_access.insert_or_update_remote_file_item(it, true))
            .map(|(it, da)| {
                let s = it.to_sql_string(&da);
                eprintln!("{:?}, {}", da, s);
                s
            })
            .chunks(100_000)
            .into_iter()
            .for_each(|ck| db_access.execute_batch(ck));
        eprintln!("elapsed: {}", now.elapsed().as_secs());
        eprintln!("{:?}", rfi_gen(dir_id)());
        Ok(())
    }

    #[test]
    fn t_to_sql_insert_string() -> Result<(), failure::Error> {
        let db_dir = tutil::TestDir::new();
        let db_access = tutil::create_a_sqlite_file_db(&db_dir)?;
        let dir_id = db_access.insert_directory("abc")?;
        let conn = db_access.get_pool().get().unwrap();
        let now = Utc::now();

        let rfi = RemoteFileItemInDb {
            id: 0,
            path: "a".to_owned(),
            sha1: None,
            len: 55,
            modified: None,
            created: None,
            changed: false,
            dir_id,
        };
        conn.execute(rfi.to_insert_sql_string().as_str(), NO_PARAMS)?;

        let rfi = RemoteFileItemInDb {
            id: 0,
            path: "a1".to_owned(),
            sha1: Some("b".to_string()),
            len: 55,
            modified: None,
            created: None,
            changed: false,
            dir_id,
        };
        conn.execute(rfi.to_insert_sql_string().as_str(), NO_PARAMS)?;

        let rfi = RemoteFileItemInDb {
            id: 0,
            path: "a2".to_owned(),
            sha1: Some("b".to_string()),
            len: 55,
            modified: Some(now),
            created: None,
            changed: false,
            dir_id,
        };
        conn.execute(rfi.to_insert_sql_string().as_str(), NO_PARAMS)?;

        let rfi = RemoteFileItemInDb {
            id: 0,
            path: "a3".to_owned(),
            sha1: Some("b".to_string()),
            len: 55,
            modified: Some(now),
            created: None,
            changed: false,
            dir_id,
        };
        let (mut rfi, _) = db_access
            .insert_or_update_remote_file_item(rfi, false)
            .expect("insert should success.");
        rfi.changed = true;
        conn.execute(rfi.to_update_changed_sql_string().as_str(), NO_PARAMS)?;
        Ok(())
    }

    #[test]
    fn t_directory_to_db() -> Result<(), failure::Error> {
        log();
        let a_file = "abc_20130110120009.tar";
        let t_dir = tutil::create_a_dir_and_a_file_with_content("abc_20130101010155.tar", "abc")?;
        t_dir.make_a_file_with_content(a_file, "abc")?;
        t_dir.make_a_file_with_content("abc_20130117120009.tar", "abc")?;
        let dir = Directory {
            remote_dir: t_dir.tmp_dir_str().to_owned(),
            ..Default::default()
        };
        let db_dir = tutil::TestDir::new();
        let db_access = tutil::create_a_sqlite_file_db(&db_dir)?;

        load_remote_item_to_sqlite(&dir, &db_access, false, 50000)?;

        assert_eq!(db_access.count_directory()?, 1);
        assert_eq!(db_access.count_remote_file_item(None)?, 3);

        {
            let mut f = fs::OpenOptions::new()
                .write(true)
                .open(t_dir.get_file_path(a_file))?;
            f.write_all(b"abc")?;
        }
        {
            let mut f = fs::OpenOptions::new()
                .read(true)
                .open(t_dir.get_file_path(a_file))?;
            let mut buf = String::new();
            f.read_to_string(&mut buf)?;
            assert_eq!(buf.as_str(), "abc")
        }

        load_remote_item_to_sqlite(&dir, &db_access, false, 50000)?;

        assert_eq!(db_access.count_directory()?, 1);
        assert_eq!(db_access.count_remote_file_item(Some(true))?, 1);
        assert_eq!(db_access.count_remote_file_item(Some(false))?, 2);

        Ok(())
    }

    #[test]
    fn t_scan_chunk() {
        let holder: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(vec![]));

        let holder1 = holder.clone();
        let holder2 = holder.clone();

        let mut aa: Vec<Vec<u8>> = (0..10_u8)
            .peekable()
            .scan(0_u8, move |st, i| {
                holder1.lock().unwrap().push(i);
                *st += 1;
                if *st == 3 {
                    *st = 0;
                    Some(1)
                } else {
                    Some(0)
                }
            })
            .filter_map(move |v| {
                if v == 0 {
                    None
                } else {
                    Some(holder2.lock().unwrap().drain(..).collect())
                }
            })
            .collect();

        if holder.lock().unwrap().len() > 0 {
            aa.push(holder.lock().unwrap().drain(..).collect());
        }

        assert_eq!(aa.len(), 4);
        assert_eq!(aa.get(3).unwrap().len(), 1);
    }
}
