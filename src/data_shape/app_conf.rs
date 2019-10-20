use crate::data_shape::{Indicator, Server, ServerYml};
use crate::db_accesses::DbAccess;
use indicatif::MultiProgress;
use log::{trace, warn};
use serde::{Deserialize, Serialize};
use std::env;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::{fs, io::Read, io::Write};

pub const CONF_FILE_NAME: &str = "bk_over_ssh.yml";
pub const PULL_SERVERS_CONF: &str = "pull-servers-conf";
pub const PUSH_SERVERS_CONF: &str = "push-servers-conf";
pub const PULL_SERVERS_DATA: &str = "pull-servers-data";
pub const PUSH_SERVERS_DATA: &str = "push-servers-data";

#[derive(Debug, Deserialize, Serialize, Default)]
pub struct LogConf {
    pub log_file: String,
    verbose_modules: Vec<String>,
}

impl LogConf {
    pub fn get_verbose_modules(&self) -> &Vec<String> {
        &self.verbose_modules
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, Eq, PartialEq)]
#[serde(rename_all(deserialize = "snake_case"))]
pub enum AppRole {
    Controller,
    Leaf,
    PullHub,
    ReceiveHub,
    PassiveLeaf,
    ActiveLeaf,
}

#[derive(Debug, Deserialize, Serialize, Default)]
pub struct MailConf {
    pub from: String,
    pub username: String,
    pub password: String,
    pub hostname: String,
    pub port: u16,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct AppConfYml {
    data_dir: String,
    log_conf: LogConf,
    pub mail_conf: MailConf,
    role: AppRole,
    archive_cmd: Vec<String>,
}

impl Default for AppConfYml {
    fn default() -> Self {
        Self {
            data_dir: "data".to_string(),
            role: AppRole::Controller,
            mail_conf: MailConf::default(),
            log_conf: LogConf::default(),
            archive_cmd: Vec::new(),
        }
    }
}

/// The data dir is a fixed path no matter the role of the app.
/// 
fn guess_data_dir(data_dir: impl AsRef<str>) -> Result<PathBuf, failure::Error> {
    let data_dir = data_dir.as_ref();
    let data_dir = if data_dir.is_empty() {
        "data"
    } else {
        data_dir
    };

    let mut path_buf = Path::new(data_dir).to_path_buf();

    if !&path_buf.is_absolute() {
        path_buf = env::current_exe()?
            .parent()
            .expect("current_exe parent should exists.")
            .join(path_buf);
    }
    if !&path_buf.exists() {
        if let Err(err) = fs::create_dir_all(&path_buf) {
            bail!("create data_dir {:?}, failed: {:?}", &path_buf, err);
        }
    }
    match path_buf.canonicalize() {
        Ok(ab) => Ok(ab),
        Err(err) => bail!("path_buf {:?} canonicalize failed: {:?}", &path_buf, err),
    }
}

#[derive(Debug, Serialize, Clone)]
pub struct MiniAppConf {
    pub buf_len: Option<usize>,
    pub skip_cron: bool,
    pub skip_sha1: bool,
    pub archive_cmd: Vec<String>,
    pub app_role: AppRole,
}

#[derive(Debug, Serialize)]
pub struct AppConf<M, D>
where
    M: r2d2::ManageConnection,
    D: DbAccess<M>,
{
    inner: AppConfYml,
    pub config_file_path: PathBuf,
    pub data_dir_full_path: PathBuf,
    pub log_full_path: PathBuf,
    pub servers_conf_dir: PathBuf,
    #[serde(skip)]
    pub db_access: Option<D>,
    #[serde(skip)]
    _m: PhantomData<M>,
    #[serde(skip)]
    lock_file: Option<fs::File>,
    #[serde(skip)]
    pub progress_bar: Option<Arc<MultiProgress>>,
    pub mini_app_conf: MiniAppConf,
}

pub fn demo_app_conf<M, D>(data_dir: &str, app_role: AppRole) -> AppConf<M, D>
where
    M: r2d2::ManageConnection,
    D: DbAccess<M>,
{
    let servers_conf_dir_name = match app_role {
        AppRole::PullHub => PULL_SERVERS_CONF,
        AppRole::ActiveLeaf => PUSH_SERVERS_CONF,
        _ => panic!("unexpected app role."),
    };

    AppConf {
        inner: AppConfYml::default(),
        config_file_path: Path::new("abc").to_path_buf(),
        data_dir_full_path: PathBuf::from(data_dir),
        log_full_path: PathBuf::from(data_dir).join("out.log"),
        servers_conf_dir: PathBuf::from("data").join(servers_conf_dir_name),
        _m: PhantomData,
        db_access: None,
        lock_file: None,
        progress_bar: None,
        mini_app_conf: MiniAppConf {
            skip_sha1: true,
            skip_cron: false,
            buf_len: None,
            archive_cmd: Vec::new(),
            app_role,
        },
    }
}

impl<M, D> AppConf<M, D>
where
    M: r2d2::ManageConnection,
    D: DbAccess<M>,
{
    pub fn set_db_access(&mut self, db_access: D) {
        // if let Err(err) = db_access.create_database() {
        //     warn!("create database failed: {:?}", err);
        // }
        self.db_access.replace(db_access);
    }

    pub fn get_sqlite_db_file(&self) -> PathBuf {
        self.data_dir_full_path.join("db.db")
    }

    #[allow(dead_code)]
    pub fn get_inner(&self) -> &AppConfYml {
        &self.inner
    }
    #[allow(dead_code)]
    pub fn get_db_access(&self) -> Option<&D> {
        self.db_access.as_ref()
    }

    pub fn skip_cron(&mut self) {
        self.mini_app_conf.skip_cron = true;
    }

    pub fn not_skip_sha1(&mut self) {
        self.mini_app_conf.skip_sha1 = false;
    }

    fn read_app_conf(file: impl AsRef<Path>,
        app_role: AppRole,
    ) -> Result<Option<AppConf<M, D>>, failure::Error> {
        if !file.as_ref().exists() {
            return Ok(None);
        }
        let file = file.as_ref();
        if let Ok(mut f) = fs::OpenOptions::new().read(true).open(file) {
            let mut buf = String::new();
            if f.read_to_string(&mut buf).is_ok() {
                match serde_yaml::from_str::<AppConfYml>(&buf) {
                    Ok(app_conf_yml) => {
                        let data_dir_full_path = guess_data_dir(app_conf_yml.data_dir.trim())?;

                        let log_full_path = {
                            let log_file = &app_conf_yml.log_conf.log_file;
                            let path = Path::new(log_file);
                            if path.is_absolute() {
                                log_file.clone()
                            } else {
                                data_dir_full_path
                                    .as_path()
                                    .join(path)
                                    .to_str()
                                    .expect("log_file should be a valid string.")
                                    .to_string()
                            }
                        };

                        let log_full_path = Path::new(&log_full_path).to_path_buf();

                        let servers_conf_dir = match app_role {
                            AppRole::PullHub => data_dir_full_path.as_path().join(PULL_SERVERS_CONF),
                            AppRole::ActiveLeaf => data_dir_full_path.as_path().join(PUSH_SERVERS_CONF),
                            _ => bail!("expected AppRole."),
                        };

                        if !servers_conf_dir.exists() {
                            if let Err(err) = fs::create_dir_all(&servers_conf_dir) {
                                bail!("create servers_conf_dir {:?}, failed: {:?}", &servers_conf_dir, err);
                            }
                        }

                        let archive_cmd = app_conf_yml.archive_cmd.clone();

                        let app_conf = AppConf {
                            inner: app_conf_yml,
                            config_file_path: file.to_path_buf(),
                            data_dir_full_path,
                            log_full_path,
                            servers_conf_dir,
                            db_access: None,
                            _m: PhantomData,
                            lock_file: None,
                            progress_bar: None,
                            mini_app_conf: MiniAppConf {
                                skip_sha1: true,
                                skip_cron: false,
                                buf_len: None,
                                archive_cmd,
                                app_role,
                            },
                        };
                        Ok(Some(app_conf))
                    }
                    Err(err) => bail!("deserialize failed: {:?}, {:?}", file, err),
                }
            } else {
                bail!("read_to_string failure: {:?}", file);
            }
        } else {
            bail!("open conf file failed: {:?}", file);
        }
    }

    pub fn get_mail_conf(&self) -> &MailConf {
        &self.inner.mail_conf
    }
    #[allow(dead_code)]
    pub fn write_to_working_dir(&self) -> Result<(), failure::Error> {
        let yml_serialized = serde_yaml::to_string(&self.inner)?;
        let path = env::current_dir()?.join(CONF_FILE_NAME);
        let mut file = fs::OpenOptions::new().write(true).create(true).open(path)?;
        write!(file, "{}", yml_serialized)?;
        Ok(())
    }

    /// If no conf file provided, first look at the same directory as executable, then current working directory.
    /// the app role must be known at this point.
    pub fn guess_conf_file(
        app_conf_file: Option<&str>,
        app_role: AppRole,
    ) -> Result<Option<AppConf<M, D>>, failure::Error> {
        if let Some(af) = app_conf_file {
            return AppConf::read_app_conf(af, app_role);
        } else {
            if let Ok(current_exe) = env::current_exe() {
                if let Some(pp) = current_exe.parent() {
                    let cf = pp.join(CONF_FILE_NAME);
                    trace!("found configuration file: {:?}", &cf);
                    if let Some(af) = AppConf::read_app_conf(&cf, app_role.clone())? {
                        // if it returned None, continue searching.
                        return Ok(Some(af));
                    }
                }
            }

            if let Ok(current_dir) = env::current_dir() {
                let cf = current_dir.join(CONF_FILE_NAME);
                trace!("found configuration file: {:?}", &cf);
                return AppConf::read_app_conf(&cf, app_role);
            }
        }
        bail!("read app_conf failed.")
    }

    pub fn lock_working_file(&mut self) -> Result<(), failure::Error> {
        let lof = self.data_dir_full_path.as_path().join("working.lock");
        trace!("start locking file: {:?}", lof);
        if lof.exists() {
            if fs::remove_file(lof.as_path()).is_err() {
                eprintln!("create lock file failed: {:?}, if you can sure app isn't running, you can delete it manually.", lof);
            }
        } else {
            self.lock_file
                .replace(fs::OpenOptions::new().write(true).create(true).open(&lof)?);
        }
        trace!("locked!");
        Ok(())
    }

    // pub fn load_this_server_yml(&self) -> Result<(Server<M, D>, Indicator), failure::Error> {
    //     let this_server_yml_path = self.data_dir_full_path.join("this_server.yml");
    //     if !this_server_yml_path.exists() {
    //         let bytes = include_bytes!("../server_template.yaml");
    //         let mut file = fs::OpenOptions::new()
    //             .write(true)
    //             .create(true)
    //             .open(&this_server_yml_path)?;
    //         file.write_all(bytes)?;
    //         bail!("this_server.yml doesn't exists, have created one for you, please edit content of it. {:?}", this_server_yml_path);
    //     }
    //     let yml_file_name = this_server_yml_path
    //         .to_str()
    //         .expect("this_server.yml should load succeeded.");
    //     self.load_server_yml(yml_file_name)
    // }

    pub fn load_server_yml(
        &self,
        yml_file_name: impl AsRef<str>,
    ) -> Result<(Server<M, D>, Indicator), failure::Error> {
        let server = self.load_server_from_yml(yml_file_name.as_ref())?;
        eprintln!(
            "load server yml from: {:?}",
            server.yml_location.as_ref().map(|pb| pb.as_os_str())
        );
        let indicator = Indicator::new(self.progress_bar.clone());
        Ok((server, indicator))
    }

    /// load all .yml file under servers directory.
    pub fn load_all_server_yml(&self) -> Vec<(Server<M, D>, Indicator)> {
        if let Ok(read_dir) = self.servers_conf_dir.read_dir() {
            read_dir
                .filter_map(|ery| match ery {
                    Err(err) => {
                        warn!("read_dir entry return error: {:?}", err);
                        None
                    }
                    Ok(entry) => Some(entry.file_name().into_string()),
                })
                .filter_map(|from_os_string| match from_os_string {
                    Err(err) => {
                        warn!("osstring to_string failed: {:?}", err);
                        None
                    }
                    Ok(astr) => Some(astr),
                })
                .map(|astr| self.load_server_yml(astr))
                .filter_map(|rr| match rr {
                    Err(err) => {
                        warn!("load_server_yml failed: {:?}", err);
                        None
                    }
                    Ok(server) => Some(server),
                })
                .collect()
        } else {
            warn!("read_dir failed: {:?}", self.servers_conf_dir);
            Vec::new()
        }
    }

    pub fn get_log_conf(&self) -> &LogConf {
        &self.inner.log_conf
    }

    pub fn load_server_from_yml(
        &self,
        name: impl AsRef<str>,
    ) -> Result<Server<M, D>, failure::Error> {
        let name = name.as_ref();
        let mut server_yml_path = Path::new(name).to_path_buf();
        if (server_yml_path.is_absolute() || name.starts_with('/')) && !server_yml_path.exists() {
            bail!(
                "server yml file doesn't exist, please create one: {:?}",
                server_yml_path
            );
        } else {
            if !(name.contains('/') || name.contains('\\')) {
                server_yml_path = self.servers_conf_dir.as_path().join(name);
            }
            if !server_yml_path.exists() {
                bail!("server yml file doesn't exist: {:?}", server_yml_path);
            }
        }
        trace!("got server yml at: {:?}", server_yml_path);
        let mut f = fs::OpenOptions::new().read(true).open(&server_yml_path)?;
        let mut buf = String::new();
        f.read_to_string(&mut buf)?;
        let server_yml: ServerYml = match serde_yaml::from_str(&buf) {
            Ok(server_yml) => server_yml,
            Err(err) => {
                bail!("parse yml file: {:?} failed: {}", server_yml_path, err);
            }
        };

        let data_dir = self.data_dir_full_path.as_path();

        let servers_data_dir = match self.mini_app_conf.app_role {
            AppRole::PullHub => data_dir.join(PULL_SERVERS_DATA),
            AppRole::ActiveLeaf => data_dir.join(PUSH_SERVERS_DATA),
            _ => bail!("unexpected app role"),
        };

        if !servers_data_dir.exists() {
            fs::create_dir_all(&servers_data_dir)?;
        }

        let mut server = Server::new(
            self.mini_app_conf.clone(),
            servers_data_dir.join(&server_yml.host),
            server_yml,
        )?;

        if let Some(bl) = self.mini_app_conf.buf_len {
            server.server_yml.buf_len = bl;
        }

        let ab = server_yml_path.canonicalize()?;
        server.yml_location.replace(ab);

        trace!(
            "loaded server, directory pairs, [local_dir, remote_dir]: {:?}",
            server
                .server_yml
                .directories
                .iter()
                .map(|d| format!("{}, {}", d.local_dir, d.remote_dir))
                .collect::<Vec<String>>()
        );
        Ok(server)
    }

    #[allow(dead_code)]
    fn get_log_file(data_dir: &Path, inner: &AppConfYml) -> String {
        let log_file = &inner.log_conf.log_file;
        let path = Path::new(log_file);
        if path.is_absolute() {
            log_file.clone()
        } else {
            data_dir
                .join(path)
                .to_str()
                .expect("log_file should be a valid string.")
                .to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::develope::tutil;
    use crate::log_util;
    use std::process::Command;

    fn log() {
        log_util::setup_logger_detail(
            true,
            "output.log",
            vec!["data_shape::app_conf"],
            Some(vec!["ssh2"]),
            "",
        )
        .expect("init log should success.");
    }

    #[test]
    fn t_app_conf_deserd() -> Result<(), failure::Error> {
        let yml = r##"---
role: controller
archive_cmd: 
  - C:/Program Files/7-Zip/7z.exe
  - a
  - archive_file_name
  - files_and_dirs
data_dir: data
log_conf:
  log_file: output.log
  verbose_modules: []
    # - data_shape::server
mail_conf:
  from: xxx@gmail.com
  username: xxx@gmail.com
  password: password
  hostname: xxx.example.com
  port: 587"##;
        let app_conf_yml = serde_yaml::from_str::<AppConfYml>(&yml)?;
        assert_eq!(
            app_conf_yml.archive_cmd,
            vec![
                "C:/Program Files/7-Zip/7z.exe",
                "a",
                "archive_file_name",
                "files_and_dirs"
            ]
        );

        log();
        // create a directory of 3 files.
        let a_file = "a_file.tar";
        let t_dir = tutil::create_a_dir_and_a_file_with_content("abc_20130101010155.tar", "abc")?;
        t_dir.make_a_file_with_content(a_file, "abc")?;
        t_dir.make_a_file_with_content("b.tar", "abc")?;

        let t_dir_name = t_dir.tmp_dir_str();

        let target_dir = tutil::TestDir::new();

        let archive_path = target_dir.tmp_dir_path().join("aa.7z");
        let archive_file_name = archive_path
            .to_str()
            .expect("archive name to str should success.");

        let archive_cmd = app_conf_yml
            .archive_cmd
            .iter()
            .map(|s| {
                if s == "archive_file_name" {
                    archive_file_name.to_owned()
                } else if s == "files_and_dirs" {
                    t_dir_name.to_owned()
                } else {
                    s.to_owned()
                }
            })
            .collect::<Vec<String>>();

        let output = if cfg!(target_os = "windows") {
            let mut c = Command::new("cmd");
            c.arg("/C");
            for seg in archive_cmd {
                c.arg(seg);
            }
            c.output().expect("failed to execute process")
        } else {
            let mut c = Command::new("sh");
            c.arg("-c");
            for seg in archive_cmd {
                c.arg(seg);
            }
            c.output().expect("failed to execute process")
        };
        eprintln!("output: {:?}", output);
        assert!(
            archive_path.metadata()?.len() > 0,
            "archived aa.7z should have a length great than 0."
        );
        Ok(())
    }
}
