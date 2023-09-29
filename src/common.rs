use crabrs::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::*;

//pub struct CustomSimpleErr {}
//impl<E: std::fmt::Display> From<E> for CustomSimpleErr {
//    fn from(inner: E) -> Self {
//        use log::*;
//        error!("{}", inner);
//        Self {}
//    }
//}
//
////#[derive(Clone, Debug, Default, PartialEq)]
//pub struct CustomErr {
//    //inner: Error
//}
//impl From<CustomSimpleErr> for CustomErr {
//    fn from(_inner: CustomSimpleErr) -> Self {
//        Self {}
//    }
//}
//
//impl<E: std::fmt::Debug> From<E> for CustomErr {
//    #[track_caller]
//    fn from(inner: E) -> Self {
//        use log::*;
//        use std::backtrace::*;
//        //note sometimes some line numbers are not captured and even some fn names are not captured (optimized out). The fix is to change profile debug=1
//        error!(
//            "{:?}\n{:?}\n{}",
//            inner,
//            std::panic::Location::caller(),
//            Backtrace::force_capture()
//        );
//        Self {}
//    }
//}

/*
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Hardware {
    #[serde(default)]
    id: String,
    #[serde(default)]
    unique_entfo_folder_name: Vec<String>, //empty means entfo folder name is not unique
}*/

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct FsEntfo {
    #[serde(default)]
    pub active: bool,
    //todo add the feature of unique folder name detection. When the entfo folder name is quite unique e.g. MyPortableExternalSSD, the program can automatically match Hardware with entfo at startup
    #[serde(default)]
    pub hardware_id: String, //this is usually empty (meaning inheriting from ancestor), except for entfo folders registered in your settings
    //#[serde(default)]
    //pub entfo_id: String, //this is usually empty (meaning inheriting from ancestor), except for real entfo folders
    #[serde(default)]
    pub tmp_mount_point: bool,
    #[serde(default)]
    pub abs_path: String, //note for tmp_mount_point, this field might be meaningless, because certain hardware storage device can change mount point
    //#[serde(default)]
    //pub folder: FsFolder,
    #[serde(default)]
    pub efolder: FsFolder,
    #[serde(default)]
    pub desce: BTreeMap<String, FsFolder>, //descendents
}

impl FsEntfo {
    pub fn canonicalize_paths_and_sort(&mut self) -> Result<(), CustomErr> {
        if self.active {
            let l_entfo = std::fs::canonicalize(&self.abs_path)?;
            let l_desce = mem::take(&mut self.desce);
            for (entkey, mut entval) in l_desce {
                match entkey.as_str() {
                    "" | "." | "/" => {
                        return Err("Failed to parse rule due to invalid key: descendents cannot be empty, dot, or slash.".into());
                    }
                    _ => {}
                }
                //not using std::fs::canonicalize here because it is normal that descendants can get deleted anytime randomly
                let pbuf = path_clean::clean(l_entfo.join(entkey));
                if pbuf == l_entfo || (!pbuf.starts_with(&l_entfo)) {
                    return Err("Failed to parse rule due to invalid key: descendent(s) are not really under entfo.".into());
                }
                let newkey = pbuf.as_os_str().to_owned().into_string()?;
                entval.pb = pbuf;
                entval.sort_matches();
                if self.desce.insert(newkey, entval).is_some() {
                    return Err(
                        "Failed to parse rule due to invalid key: descendents have duplicates."
                            .into(),
                    );
                }
            }
            self.abs_path = l_entfo.as_os_str().to_owned().into_string()?;
            self.efolder.pb = l_entfo;
        }
        self.efolder.sort_matches();
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct FsFolder {
    #[serde(skip)]
    pub pb: PathBuf,
    #[serde(default)]
    pub include: bool,
    #[serde(default)]
    pub ends_with_include: Vec<String>,
    #[serde(default)]
    pub ends_with_exclude: Vec<String>,
    #[serde(default)]
    pub base_filename_include: Vec<String>,
    #[serde(default)]
    pub base_filename_exclude: Vec<String>,
}

impl FsFolder {
    pub fn sort_matches(&mut self) {
        //self.ends_with_include.sort_unstable();
        //self.ends_with_exclude.sort_unstable();
        self.base_filename_include.sort_unstable();
        self.base_filename_exclude.sort_unstable();
    }
}

//#[derive(Clone, Debug, Default, PartialEq)]
//pub struct WalkDirFrame {
//	pub iter: std::fs::ReadDir,
//	pub folder: FsFolder,
//	pub folder_exists: bool,
//}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct InfoSettings {
    //todo
    #[serde(default)]
    unique_entfo_folder_names: Vec<String>, //empty means entfo folder name is not unique (UNIQUE ENTFO FOLDER NAME means a entfo folder with a univerally unique base name. E.g. uThinkPadT470)
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct LocalSettings {
    #[serde(default)]
    pub default_hardware_id: String, //usually this is your laptop/pc
    #[serde(default)]
    pub sync_method: String,
    #[serde(default)]
    pub auth_secret_d: String,
    #[serde(default)]
    pub auth_secret_m: String,
    #[serde(default)]
    pub auth_secret_g: String,
    //#[serde(default)]
    //pub tmp_mount_points: BTreeMap<String, FsEntfo>,
    //
    //
    //
}
