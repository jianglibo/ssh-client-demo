pub mod app_conf;
pub mod file_item;
pub mod remote_file_item;
pub mod rolling_files;
pub mod server;
pub mod string_path;
pub mod disk_directory;
pub mod writer_with_progress;
pub mod sha1_reader;
pub mod count_reader;
pub mod indicator;

pub use app_conf::{AppConf, MailConf, CONF_FILE_NAME, demo_app_conf, MiniAppConf, AppRole};
pub use file_item::{FileItem, FileItemProcessResult, FileItemProcessResultStats, SyncType};
pub use remote_file_item::{load_remote_item, RemoteFileItem, load_remote_item_to_sqlite};
pub use server::{Server, ServerYml};
pub use disk_directory::{Directory};
pub use writer_with_progress::{ProgressWriter};
pub use count_reader::{CountReader};
pub use indicator::{Indicator, PbProperties};
pub use sha1_reader::{Sha1Readr};

use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Debug)]
pub struct ScheduleItem {
    pub name: String,
    pub cron: String,
}

#[derive(Builder, Deserialize, Serialize, Debug)]
#[builder(setter(into))]
pub struct PruneStrategy {
    #[builder(default = "1")]
    pub yearly: u8,
    #[builder(default = "1")]
    pub monthly: u8,
    #[builder(default = "0")]
    pub weekly: u8,
    #[builder(default = "1")]
    pub daily: u8,
    #[builder(default = "1")]
    pub hourly: u8,
    #[builder(default = "1")]
    pub minutely: u8,
}

#[derive(Deserialize, Serialize, Debug)]
pub enum AuthMethod {
    Password,
    Agent,
    IdentityFile,
}
