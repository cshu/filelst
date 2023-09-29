#![allow(clippy::print_literal)]
#![allow(clippy::needless_return)]
#![allow(dropping_references)]
#![allow(clippy::assertions_on_constants)]
mod common;
mod dirdiff;
mod remote;
mod sqlite_util;
mod util;

use dirdiff::*;

//note unlike ecsbas5, this program is currently using \ for windows. ecsbas5 is using / for every path in sqlite.

//note https://stackoverflow.com/questions/12994870/sqlite-not-using-index-with-like-query
//note COLLATE NOCASE can enable case-insensitive fast searching, but you are not sure if it hurts performance when you want to do exact comparison in query, like `lbl=?1`. Maybe you should just use case-sensitive search more often, e.g. glob
//note glob can use index "For the GLOB operator, the column must be indexed using the built-in BINARY collating sequence" https://www.sqlite.org/optoverview.html

use log::*;
//use serde::{Deserialize, Serialize};
//use std::backtrace::*;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
//use std::ffi::OsString;
use crabrs::*;
use crabsqliters::*;
use std::io::Write;
use std::process::*;
use std::*;

#[macro_use(defer)]
extern crate scopeguard;

const PKG_NAME: &str = env!("CARGO_PKG_NAME");
const _: () = assert!(!PKG_NAME.is_empty(), "Constraint on const");

//note tmpid starts from 1. 0 is reserved. Negative is for representing a SELECTED file

//fixme note ONE big problem is you can use this tool simultaneously on multiple laptops and you do not want conflict (at least you want to minimize conflict)

//note#1 you have fixed fields for entfo (hardware), rel (path), sha256, copyright, etc.
//note#2 you have label (lbl) basically just 1-arity predicate
//todo want to add support for 2-arity or even more complex predicate?
//todo add command `fmt settings` for just formatting config files
//todo add feature of storing empty folder in info

//todo use advisory file lock to avoid running 2 instances. maybe you can use fd-lock crate
//todo add crc32 or md5 or other kind of checksum file to make sure all JSON files match the hash (i.e. not tampered for unknown reason)

//concepts being used: Hardware, Entfo, Folder, File, FsFile
//Hardware is easy to understand, the storage
//Entfos mean folders that are used by your program to scan files (Each Entfo has a unique Entfo ID. This ID is human-friendly and human-readable) (Actually you can argue that Entfo ID is unnecessary, because HardwareID+RelativePath will just work as EntfoID. But HardwareID+RelativePath might be a very long string and greatly increase json size.)
//Folders are descendents under entfo (while Entfo can also be viewed as a kind of Folder, a special kind of Folder)
//File is file with unique sha256
//FsFile is a copy of File on a physical location

//rules of include and exclude:
//- NOTE currently you have 3 kinds of rules. Filename ENDS_WITH/BASE_FILENAME RULES coexist with RELATIVE-PATH RULES (relative-path means a folder relative to a entfo)
//- more detailed rule override less detailed rule (e.g. /foo/bar/ specific rule override /foo/ folder rule)
//- if 2 rules are detailed at same level (or it is hard to compare and decide which is more detailed), then exclude rule override include rule (e.g. EXCLUDE *.txt override INCLUDE !/foo/)
//- despite rules, you can still FORCE add file
//- (it is ok for Entfo folder can be "exclude", so it can just act as a Entfo for several "include" descendents.)
//???? local rules override info rules
//???? entfo-folder-specifc rules override any-folder rules
//???? info entfo-folder-specific override local any-folder

fn init_sqlite(conn: &rusqlite::Connection) -> Result<(), CustomErr> {
    //use rusqlite::{Connection, Result};
    //optimize store lbl name in hashmap, not in sqlite, and use INTEGER lbl column in sqlite
    conn.execute(
        "CREATE TABLE lbls (
	    tmpid integer not null,
	    lbl text not null
        )",
        (),
    )?;
    conn.execute("CREATE INDEX idx_tmpid_lbls ON lbls (tmpid)", ())?;
    conn.execute("CREATE INDEX idx_lbl ON lbls (lbl)", ())?;
    conn.execute(
        "CREATE TABLE oldh (
	    newi integer not null,
	    oldi integer not null
        )",
        (),
    )?;
    conn.execute("CREATE INDEX idx_newi ON oldh (newi)", ())?;
    conn.execute("CREATE INDEX idx_oldi ON oldh (oldi)", ())?;
    conn.execute(
        "CREATE TABLE files (
	    tmpid INTEGER PRIMARY KEY,
	    size integer not null,
	    mtime integer not null,
	    hash blob not null,
	    desc text not null,
	    old_filename text not null,
	    download_url text not null,
	    archive_filename text not null,
	    copyright text not null,
	    tidx integer not null
        )",
        (), //note tidx is the location of json file storing info info for this file
    )?;
    //conn.execute("CREATE UNIQUE INDEX idx_tmpid ON files (tmpid)", ())?;
    conn.execute("CREATE UNIQUE INDEX idx_sha256 ON files (hash)", ())?;
    conn.execute("CREATE INDEX idx_size ON files (size)", ())?;
    conn.execute("CREATE INDEX idx_mtime ON files (mtime)", ())?;
    conn.execute("CREATE INDEX idx_old_filename ON files (old_filename)", ())?;
    conn.execute("CREATE INDEX idx_download_url ON files (download_url)", ())?;
    conn.execute(
        "CREATE INDEX idx_archive_filename ON files (archive_filename)",
        (),
    )?;
    //conn.execute("CREATE INDEX idx_copyright ON files (copyright)", ())?;
    conn.execute("CREATE INDEX idx_tidx ON files (tidx)", ())?;
    conn.execute(
        "CREATE TABLE fsfiles (
	    tmpid INTEGER,
	    entfo text not null,
	    rel text not null,
	    mtime integer not null
        )",
        (),
    )?;
    conn.execute("CREATE INDEX idx_tmpid_fsfiles ON fsfiles (tmpid)", ())?;
    conn.execute("CREATE INDEX idx_rel ON fsfiles (rel)", ())?;
    conn.execute(
        "CREATE TABLE filter_buf (
	    tid INTEGER PRIMARY KEY
        )",
        (),
    )?;
    conn.execute(
        "CREATE TABLE dx (
	    email text not null,
	    id text not null,
	    client_modified text not null,
	    server_modified text not null,
	    size integer not null,
	    path_display text not null
        )",
        (),
    )?;
    Ok(())
}
fn get_avail_tmpid(conn: &rusqlite::Connection) -> Result<i64, CustomErr> {
    let mut cached_stmt =
        conn.prepare_cached("select tmpid from files order by tmpid desc limit 1")?;
    let mut rows = cached_stmt.query([])?;
    if let Some(row) = rows.next()? {
        let retval: i64 = row.get(0)?;
        return Ok(retval + 1);
    } else {
        return Ok(1);
    }
}

pub struct Ctx {
    db: rusqlite::Connection,
    hasher: sha2::Sha256,
    def: CtxDef,
}
impl ops::Deref for Ctx {
    type Target = CtxDef;

    fn deref(&self) -> &CtxDef {
        &self.def
    }
}
impl ops::DerefMut for Ctx {
    fn deref_mut(&mut self) -> &mut CtxDef {
        &mut self.def
    }
}
#[derive(Default)]
pub struct CtxDef {
    stdin_w: StdinWrapper,
    envargs: Vec<String>, //note vec default will not allocate
    tidx_modified: HashSet<u32>,

    //
    already_checked_git_email_n_name: bool,
    //
    home_dir: PathBuf,
    everycom: PathBuf,
    app_support_dir: PathBuf,
    sync_dir: PathBuf,
    info_dir: PathBuf,
    //infod_dir: PathBuf,
    //infom_dir: PathBuf,
    local_settings_file: PathBuf,
    local_settings: common::LocalSettings,
    info_settings_file: PathBuf,
    info_settings: common::InfoSettings,
    //? consider two files storing entfo data, one is sync to git, one is never sync? (custentfos and mentfos)
    custentfos_settings_file: PathBuf,
    //mentfos_settings_file: PathBuf,
    custentfos_settings: BTreeMap<String, common::FsEntfo>, //note key is Entfo folder ID
    //mentfos_settings: BTreeMap<String, common::FsEntfo>, //note key is Entfo folder ID
    tidx_st: util::TIdx,
    iline: String,
    iline_argidx: usize,
    filter_buf: Vec<i64>,
    chosen_lbl: String,
    info_pair: Option<(InfoDir, InfoDir)>,
    info_pair_file: PathBuf,
    listed_at_least_once: HashSet<String>,
    dx_cli: reqwest::blocking::Client,
    dx_email: String,
    //dx_dir: PathBuf,
    //mstok: String,
    mgraph_cli: reqwest::blocking::Client,
    mgraph_upn: String,
    //mgraph_dir: PathBuf,
}

fn chk_base_name_or_suffix_include(
    folder: &common::FsFolder,
    filenm: &std::ffi::OsStr,
    rule_path_len: &mut usize,
    res: &mut bool,
) {
    let folder_len = folder.pb.as_os_str().len();
    if folder_len < *rule_path_len {
        return;
    }
    let l_fstr = filenm.to_string_lossy().into_owned();
    let fstr: String = l_fstr;
    if folder.base_filename_include.binary_search(&fstr).is_ok() {
        *rule_path_len = folder_len;
        *res = true;
    } else {
        for suffix in &folder.ends_with_include {
            if fstr.ends_with(suffix) {
                *rule_path_len = folder_len;
                *res = true;
                break;
            }
        }
    }
    if folder.base_filename_exclude.binary_search(&fstr).is_ok() {
        *rule_path_len = folder_len;
        *res = false;
    } else {
        for suffix in &folder.ends_with_exclude {
            if fstr.ends_with(suffix) {
                *rule_path_len = folder_len;
                *res = false;
                break;
            }
        }
    }
}
impl Ctx {
    pub fn new(args: Vec<String>, conn: rusqlite::Connection) -> Self {
        let retval = CtxDef {
            envargs: args,
            ..Default::default()
        };
        let _ = &retval.envargs; //fixme suppressing warning of: field `envargs` is never read
        let def_hasher = sha2::Sha256::new();
        use sha2::Digest;
        Self {
            db: conn,
            hasher: def_hasher,
            def: retval,
        }
    }
    fn canonicalize_paths_and_sort_matches(&mut self) -> Result<(), CustomErr> {
        for entval in self.custentfos_settings.values_mut() {
            entval.canonicalize_paths_and_sort()?;
        }
        //for entval in self.mentfos_settings.values_mut() {
        //    entval.canonicalize_paths_and_sort()?;
        //}
        Ok(())
    }
    fn chk_entfo_under_entfo(
        &self,
        settings: &BTreeMap<String, common::FsEntfo>,
        entfo: &str,
        direntp: &PathBuf,
    ) -> bool {
        //todo under self, you might have Local settings and Info settings, you should check both?
        for (entkey, entval) in settings {
            if entfo != entkey && direntp == &entval.efolder.pb {
                //? should chk active field?
                //here can be reachable when you have tmp mount point like external disk
                return true;
            }
        }
        false
    }
    //fn chk_entfo_under_entfo(
    //    &self,
    //    settings: &BTreeMap<String, common::FsEntfo>,
    //    entfo: &str,
    //    target: &str,
    //) -> bool {
    //    //todo under self, you might have Local settings and Info settings, you should check both?
    //    for (entkey, entval) in settings {
    //        if entkey == entfo {
    //            continue;
    //        }
    //        if entval.abs_path == target {
    //            return true;
    //        }
    //    }
    //    false
    //}
    fn chk_rules_for_match(&self, fsentfo: &common::FsEntfo, direntp: &Path) -> bool {
        let dirent_fn = direntp.file_name().unwrap(); //impossible to panic cuz you canonicalized it?
        let mut dir_name_or_suffix_path_len = 0usize;
        let mut dir_include_path_len = 0usize;
        let mut base_name_or_suffix_include = true;
        chk_base_name_or_suffix_include(
            &fsentfo.efolder,
            dirent_fn,
            &mut dir_name_or_suffix_path_len,
            &mut base_name_or_suffix_include,
        );
        let mut umbrella_include = fsentfo.efolder.include;
        for (entkey, entval) in &fsentfo.desce {
            if direntp.starts_with(&entval.pb) {
                chk_base_name_or_suffix_include(
                    entval,
                    dirent_fn,
                    &mut dir_name_or_suffix_path_len,
                    &mut base_name_or_suffix_include,
                );
                if entkey.len() > dir_include_path_len {
                    dir_include_path_len = entkey.len();
                    umbrella_include = entval.include;
                }
            }
        }
        umbrella_include && base_name_or_suffix_include
    }
    fn chk_rules_for_file(&self, fsentfo: &common::FsEntfo, direntp: &Path) -> bool {
        for entval in fsentfo.desce.values() {
            if entval.pb.as_os_str() == direntp.as_os_str() {
                return entval.include;
            }
        }
        self.chk_rules_for_match(fsentfo, direntp)
    }
    fn chk_rules_for_dir(&self, fsentfo: &common::FsEntfo, direntp: &PathBuf) -> bool {
        let mut exact_match = false;
        for entval in fsentfo.desce.values() {
            if entval.pb.starts_with(direntp) {
                //if there are detailed finer control under current folder
                if entval.pb.as_path() != direntp.as_path() {
                    //return true only if it is more detail (finer)
                    return true;
                }
                if entval.include {
                    return true;
                }
                exact_match = true;
            }
            //note NO NEED to check `include` field of paths that are SHORTER than current folder path (because they are already checked in previous iterations)
        }
        if exact_match {
            return false;
        }
        self.chk_rules_for_match(fsentfo, direntp)
    }
    fn scan_entfo_chk_registered_files(
        &mut self,
        entfo: &str,
        l_entfo: &std::path::Path,
        tidx_modified: &mut HashSet<u32>,
    ) -> Result<(), CustomErr> {
        //if file path AND size AND name AND physical mtime are the same then skip calculating sha256!!
        struct RelMtime {
            rel: String,
            mtime: i64,
        }
        struct RelMsFlenHash {
            rel: String,
            ms: i64,
            flen: i64,
            hash: [u8; 32],
        }
        let mut to_be_removed: Vec<String> = vec![];
        let mut to_have_new_mtime: Vec<RelMtime> = vec![];
        let mut to_have_new_sha256: Vec<RelMsFlenHash> = vec![]; //rel, millis, flen, chash
        {
            let mut cached_stmt = self.db.prepare_cached("select rel,size,fsfiles.mtime,hash,tidx from fsfiles join files on fsfiles.tmpid=files.tmpid where entfo=?1")?;
            let mut rows = cached_stmt.query((entfo,))?;
            while let Some(row) = rows.next()? {
                let rel: String = row.get(0)?;
                let relp = l_entfo.join(&rel);
                if !real_reg_file_without_symlink(&relp) {
                    to_be_removed.push(rel);
                    let icol: i64 = row.get(4)?;
                    tidx_modified.insert(icol as u32);
                    continue;
                }
                let relmd = relp.metadata()?;
                let millis = systemtime2millis(relmd.modified()?);
                let flen = relmd.len() as i64;
                let size: i64 = row.get(1)?;
                if size == flen {
                    let mtime: i64 = row.get(2)?;
                    if mtime == millis {
                        continue;
                    }
                    debug!("BEFORE calc_hash TIME {}", now_in_millis());
                    let chash = calc_hash(&mut self.hasher, &relp)?;
                    debug!("AFTER calc_hash TIME {}", now_in_millis());
                    let hash: Vec<u8> = row.get(3)?;
                    if chash == hash.as_slice() {
                        to_have_new_mtime.push(RelMtime { rel, mtime: millis });
                        let icol: i64 = row.get(4)?;
                        tidx_modified.insert(icol as u32);
                        continue;
                    }
                    to_have_new_sha256.push(RelMsFlenHash {
                        rel,
                        ms: millis,
                        flen,
                        hash: chash,
                    });
                } else {
                    debug!("BEFORE calc_hash TIME {}", now_in_millis());
                    let chash = calc_hash(&mut self.hasher, &relp)?;
                    debug!("AFTER calc_hash TIME {}", now_in_millis());
                    to_have_new_sha256.push(RelMsFlenHash {
                        rel,
                        ms: millis,
                        flen,
                        hash: chash,
                    });
                }
                let icol: i64 = row.get(4)?;
                tidx_modified.insert(icol as u32);
            }
        }
        {
            let mut cached_stmt = self
                .db
                .prepare_cached("delete from fsfiles where rel=?1 and entfo=?2")?;
            for rel_to_remove in to_be_removed {
                println!("{} {} {}", "REMOVE", entfo, rel_to_remove);
                cached_stmt.execute((rel_to_remove, entfo))?;
            }
        }
        {
            let mut cached_stmt = self
                .db
                .prepare_cached("update fsfiles set mtime=?1 where rel=?2 and entfo=?3")?;
            for f_with_new_mtime in to_have_new_mtime {
                println!("{} {} {}", "MTIME CHANGED", entfo, f_with_new_mtime.rel);
                cached_stmt.execute((f_with_new_mtime.mtime, f_with_new_mtime.rel, entfo))?;
            }
        }
        struct RelMsTmpid {
            rel: String,
            ms: i64,
            tmpid: i64,
        }
        let mut attach_to_diff_file: Vec<RelMsTmpid> = vec![]; //rel+millis+tmpid
        let mut hash_dup = HashMap::<[u8; 32], Vec<RelMsFlenHash>>::new();
        {
            let mut cached_stmt = self
                .db
                .prepare_cached("select tmpid,tidx from files where hash=?1")?;
            for have_new_hash in to_have_new_sha256 {
                let mut rows = cached_stmt.query((have_new_hash.hash,))?;
                if let Some(row) = rows.next()? {
                    println!("{} {} {}", "RE-ATTACH", entfo, have_new_hash.rel);
                    attach_to_diff_file.push(RelMsTmpid {
                        rel: have_new_hash.rel,
                        ms: have_new_hash.ms,
                        tmpid: row.get(0)?,
                    });
                    let icol: i64 = row.get(1)?;
                    tidx_modified.insert(icol as u32);
                } else {
                    match hash_dup.entry(have_new_hash.hash) {
                        std::collections::hash_map::Entry::Occupied(oe) => {
                            oe.into_mut().push(have_new_hash);
                        }
                        std::collections::hash_map::Entry::Vacant(ve) => {
                            ve.insert(vec![have_new_hash]);
                        }
                    }
                }
            }
        }
        {
            let mut cached_stmt = self
                .db
                .prepare_cached("update fsfiles set mtime=?1,tmpid=?2 where rel=?3 and entfo=?4")?;
            for atdf in attach_to_diff_file {
                cached_stmt.execute((atdf.ms, atdf.tmpid, atdf.rel, entfo))?;
            }
        }
        //note! there MIGHT be DUPLICATE hash in hash_dup!!! (i.e. 2 orphan fsfiles with new hash happen to have same hash)
        struct FlenHashLstRelMs {
            min_ms: i64,
            flen: i64,
            hash: [u8; 32],
            lst: Vec<RelMtime>,
        }
        fn normalize_for_lenhashlstrelms(lst: Vec<RelMsFlenHash>) -> FlenHashLstRelMs {
            let one_el = &lst[0];
            let mut retval = FlenHashLstRelMs {
                min_ms: one_el.ms,
                flen: one_el.flen,
                hash: one_el.hash,
                lst: Vec::with_capacity(lst.len()),
            };
            for rmfh in lst {
                if rmfh.ms < retval.min_ms {
                    retval.min_ms = rmfh.ms;
                }
                retval.lst.push(RelMtime {
                    rel: rmfh.rel,
                    mtime: rmfh.ms,
                });
            }
            retval
        }
        let mut clone_to_new_file: Vec<RelMsFlenHash> = vec![]; //rel+millis+flen+chash
        let mut modify_the_file: Vec<RelMsFlenHash> = vec![]; //rel+millis+flen+chash
        let mut hash_same: Vec<FlenHashLstRelMs> = vec![];
        {
            let mut cached_stmt = self
                .db
                .prepare_cached("select 0 from fsfiles where tmpid in(select tmpid from fsfiles where rel=?1 and entfo=?2 limit 1) and (rel<>?1 or entfo<>?2)")?;
            for (_entkey, entval) in hash_dup {
                if 1 == entval.len() {
                    let atnf = entval.into_iter().next().unwrap();
                    let mut rows = cached_stmt.query((&atnf.rel, entfo))?;
                    if (rows.next()?).is_some() {
                        clone_to_new_file.push(atnf);
                    } else {
                        modify_the_file.push(atnf);
                    }
                } else {
                    hash_same.push(normalize_for_lenhashlstrelms(entval));
                }
            }
        }
        {
            let mut cached_stmt = self
                .db
                .prepare_cached("update fsfiles set mtime=?1 where rel=?2 and entfo=?3")?;
            for mtf in &modify_the_file {
                cached_stmt.execute((mtf.ms, &mtf.rel, entfo))?;
            }
        }
        {
            let mut cached_stmt = self
                .db
                .prepare_cached("update files set size=?1,mtime=?2,hash=?3 where tmpid in(select tmpid from fsfiles where rel=?4 and entfo=?5 limit 1)")?;
            for mtf in modify_the_file {
                cached_stmt.execute((mtf.flen, mtf.ms, mtf.hash, mtf.rel, entfo))?;
            }
        }
        let avail_tmpid = get_avail_tmpid(&self.db)?;
        {
            let mut avail_tmpid = avail_tmpid;
            let mut cached_stmt = self
                .db
                .prepare_cached("insert into files select ?1,?2,?3,?4,desc,old_filename,download_url,archive_filename,copyright,0 from files where tmpid in(select tmpid from fsfiles where rel=?5 and entfo=?6 limit 1)")?;
            for ctnf in &clone_to_new_file {
                cached_stmt.execute((
                    avail_tmpid,
                    ctnf.flen,
                    ctnf.ms,
                    ctnf.hash,
                    &ctnf.rel,
                    entfo,
                ))?;
                avail_tmpid += 1;
            }
        }
        {
            let mut avail_tmpid = avail_tmpid;
            let mut cached_stmt = self
                .db
                .prepare_cached("insert into lbls select ?1,lbl from lbls where tmpid in(select tmpid from fsfiles where rel=?2 and entfo=?3 limit 1)")?;
            for ctnf in &clone_to_new_file {
                cached_stmt.execute((avail_tmpid, &ctnf.rel, entfo))?;
                avail_tmpid += 1;
            }
        }
        let mut avail_tmpid = avail_tmpid;
        {
            let mut cached_stmt = self
                .db
                .prepare_cached("update fsfiles set mtime=?1,tmpid=?2 where rel=?3 and entfo=?4")?;
            for ctnf in clone_to_new_file {
                cached_stmt.execute((ctnf.ms, avail_tmpid, ctnf.rel, entfo))?;
                avail_tmpid += 1;
            }
        }
        let avail_tmpid = avail_tmpid;
        {
            let mut avail_tmpid = avail_tmpid;
            let mut cached_stmt = self
                .db
                .prepare_cached("insert into files values(?1,?2,?3,?4,'','','','','',0)")?;
            for hsame in &hash_same {
                cached_stmt.execute((avail_tmpid, hsame.flen, hsame.min_ms, hsame.hash))?;
                avail_tmpid += 1;
            }
        }
        let mut oldi_list: Vec<HashSet<i64>> = Vec::with_capacity(hash_same.len());
        {
            let mut cached_stmt = self
                .db
                .prepare_cached("select tmpid from fsfiles where rel=?1 and entfo=?2 limit 1")?;
            for hsame in &hash_same {
                let mut new_elem = HashSet::<i64>::new();
                for ffile in &hsame.lst {
                    let mut rows = cached_stmt.query((&ffile.rel, entfo))?;
                    let old_id: i64 = rows.next()?.unwrap().get(0)?;
                    new_elem.insert(old_id);
                }
                oldi_list.push(new_elem);
            }
        }
        {
            let mut avail_tmpid = avail_tmpid;
            let mut cached_stmt = self
                .db
                .prepare_cached("update fsfiles set mtime=?1,tmpid=?2 where rel=?3 and entfo=?4")?;
            for hsame in hash_same {
                for ffile in hsame.lst {
                    cached_stmt.execute((ffile.mtime, avail_tmpid, ffile.rel, entfo))?;
                }
                avail_tmpid += 1;
            }
        }
        {
            let mut avail_tmpid = avail_tmpid;
            let mut cached_stmt = self.db.prepare_cached("insert into oldh values(?1,?2)")?;
            for newi_oldi in oldi_list {
                for one_oldi in newi_oldi {
                    let l_oldi: i64 = one_oldi;
                    cached_stmt.execute((avail_tmpid, l_oldi))?;
                }
                avail_tmpid += 1;
            }
        }
        Ok(())
    }
    fn scan_entfo_for_registered(
        &mut self,
        settings: &BTreeMap<String, common::FsEntfo>,
        entfo: &str,
    ) -> Result<(), CustomErr> {
        let fsentfo = &settings[entfo];
        if !fsentfo.active {
            return Ok(());
        }
        if !util::chk_path_valid_as_entfo(&fsentfo.efolder.pb) {
            //? should remove all FsFile records under this entfo from info? AND delete this entfo from config?
            return Ok(());
        }
        //****
        let mut tidx_modified = mem::take(&mut self.tidx_modified);
        let retval: Result<(), CustomErr> =
            self.scan_entfo_chk_registered_files(entfo, &fsentfo.efolder.pb, &mut tidx_modified);
        self.tidx_modified = tidx_modified;
        retval?;
        //****
        Ok(())
    }
    fn scan_entfo_for_new(
        &mut self,
        settings: &BTreeMap<String, common::FsEntfo>,
        entfo: &str,
    ) -> Result<(), CustomErr> {
        let fsentfo = &settings[entfo];
        if !fsentfo.active {
            return Ok(());
        }
        if !util::chk_path_valid_as_entfo(&fsentfo.efolder.pb) {
            //? should remove all FsFile records under this entfo from info? AND delete this entfo from config?
            return Ok(());
        }
        let l_read_dir: std::fs::ReadDir = fsentfo.efolder.pb.read_dir()?;
        //let frames = Vec<WalkDirFrame>::new();
        //frames.push(WalkDirFrame{l_read_dir,fsentfo.folder.clone(),true,});
        let mut iters = Vec::<std::fs::ReadDir>::new();
        iters.push(l_read_dir);
        let l_entfo_len: usize = fsentfo
            .efolder
            .pb
            .to_str()
            .ok_or("Path not valid UTF-8")?
            .len();
        loop {
            //let fr = frames.last().unwrap();
            match iters.last_mut().unwrap().next() {
                None => {
                    iters.pop();
                    if iters.is_empty() {
                        break;
                    }
                    continue;
                }
                Some(Err(err)) => {
                    return Err(err.into());
                }
                Some(Ok(dirent)) => {
                    let filety = dirent.file_type()?; //docs:"will not traverse symlinks"
                    if filety.is_file()
                    /*//note this will not traverse symlinks (unlike Path) */
                    {
                        let direntp = dirent.path().canonicalize()?;
                        if self.chk_rules_for_file(fsentfo, &direntp) {
                            let direntp_utf8 = direntp.to_str().ok_or("Path not valid UTF-8")?;
                            match direntp_utf8
                                .as_bytes()
                                .get(l_entfo_len)
                                .ok_or("Failed to get char of separator")?
                            {
                                b'/' | b'\\' => {}
                                _ => {
                                    return Err("Unrecognized separator".into());
                                }
                            }
                            let rel = direntp_utf8
                                .get((l_entfo_len + 1)..)
                                .ok_or("Failed to get relative path for file under entfo")?;
                            if util::check_fsfile_existence(&self.db, rel, entfo)? {
                                continue;
                            }
                            debug!("file {} TIME {}", direntp.display(), now_in_millis());
                            debug!("BEFORE calc_hash TIME {}", now_in_millis());
                            let chash = calc_hash(&mut self.hasher, &direntp)?;
                            debug!("AFTER calc_hash TIME {}", now_in_millis());
                            let fmd = dirent.metadata()?;
                            let ms: i64 = systemtime2millis(fmd.modified()?);
                            let tmpid: i64;
                            {
                                let mut cached_stmt = self
                                    .db
                                    .prepare_cached("select tmpid,tidx from files where hash=?1")?;
                                let mut rows = cached_stmt.query((chash,))?;
                                if let Some(row) = rows.next()? {
                                    tmpid = row.get(0)?;
                                    let l_tidx: i64 = row.get(1)?;
                                    mem::drop(row);
                                    mem::drop(rows);
                                    mem::drop(cached_stmt);
                                    self.tidx_modified.insert(l_tidx as u32);
                                } else {
                                    tmpid = util::ins_file(&self.db, fmd.len() as i64, ms, chash)?;
                                }
                            }
                            util::ins_fsf(&self.db, tmpid, entfo, rel, ms)?;
                        }
                        continue;
                    } else if filety.is_dir()
                    /*//note this will not traverse symlinks (unlike Path) */
                    {
                        let direntp = dirent.path().canonicalize()?;
                        if !self.chk_entfo_under_entfo(settings, entfo, &direntp)
                            && self.chk_rules_for_dir(fsentfo, &direntp)
                        {
                            let t_read_dir = direntp.read_dir()?;
                            iters.push(t_read_dir);
                            //frames.push(WalkDirFrame{t_read_dir, FsFolder::default(), false,})
                        }
                        continue;
                    } else {
                        continue; //dirent ignored
                    }
                }
            }
        }
        Ok(())
    }
    //fn scan_one_folder(&mut self, entfostr: &str, prefix: &str) -> CustRes<bool>{
    //    if self.custentfos_settings.get(entfostr).is_none() {
    //        return Ok(false)
    //    }
    //    //*****
    //    let l_custentfos_settings = mem::take(&mut self.custentfos_settings);
    //    //let res = self.scan_single_entfo(&l_custentfos_settings);
    //    self.custentfos_settings = l_custentfos_settings;
    //    //res?;
    //    //*****
    //    //
    //    Ok(true)
    //}
    fn scan_single_entfo(
        &mut self,
        settings: &BTreeMap<String, common::FsEntfo>,
        entfo: &str,
    ) -> Result<(), CustomErr> {
        self.scan_entfo_for_new(settings, &entfo)?;
        self.scan_entfo_for_registered(settings, &entfo)?;
        Ok(())
    }
    //note you are doing scanning new file FIRST, and then after that you scan registered files. Note the benefit of doing so is that if you have one old registered file getting modified (so it has new hash) and simultaneously you have one new file that happens to have a hash matching the old file prev hash, given that you scan new file FIRST, your new file can successully inherit all labels/desc/etc. of the old file. (If you scan registered files FIRST, you would end up with a new file inheriting nothing)
    fn scan(&mut self) -> Result<(), CustomErr> {
        let l_custentfos_settings = mem::take(&mut self.custentfos_settings);
        let retval: Result<(), CustomErr> = || -> Result<(), CustomErr> {
            for (entkey, entval) in &l_custentfos_settings {
                if entval.hardware_id == self.local_settings.default_hardware_id {
                    self.scan_entfo_for_new(&l_custentfos_settings, entkey)?;
                }
            }
            for (entkey, entval) in &l_custentfos_settings {
                if entval.hardware_id == self.local_settings.default_hardware_id {
                    self.scan_entfo_for_registered(&l_custentfos_settings, entkey)?;
                }
            }
            Ok(())
        }();
        self.custentfos_settings = l_custentfos_settings;
        retval
    }
    fn cmd_loop(&mut self) -> Result<(), CustomErr> {
        loop {
            self.iline = cout_n_flush_input!(">>> ", self.stdin_w.lines, ());
            match self.iline.as_str() {
                "dir-diff" => {
                    if !dir_diff(self)? {
                        return Ok(());
                    }
                }
                "try-d" => {
                    let mut tok = match read_to_string_with_path_empty_chk(
                        &self.def.local_settings.auth_secret_d,
                    ) {
                        Err(_) => {
                            coutln!("Failed to read from file.");
                            continue;
                        }
                        Ok(inner) => inner,
                    };
                    while tok.ends_with(&['\n', '\r']) {
                        tok.pop();
                    }
                    if tok.is_empty() {
                        coutln!("Cannot be empty");
                        continue;
                    }
                    self.dx_cli = crabdxrs::mk_client(tok)?;
                    self.dx_email = crabdxrs::get_email(&self.dx_cli)?;
                    coutln!("Email: ", self.dx_email);
                    //let dirnm = filename_enc::encode(&self.dx_email);
                    //self.dx_dir = self.infod_dir.join(dirnm);
                    //fs::create_dir_all(&self.dx_dir)?;
                }
                "try-m" => {
                    let mut tok = match read_to_string_with_path_empty_chk(
                        &self.def.local_settings.auth_secret_m,
                    ) {
                        Err(_) => {
                            coutln!("Failed to read from file.");
                            continue;
                        }
                        Ok(inner) => inner,
                    };
                    while tok.ends_with(&['\n', '\r']) {
                        tok.pop();
                    }
                    if tok.is_empty() {
                        coutln!("Cannot be empty");
                        continue;
                    }
                    self.mgraph_cli = crabmgraphrs::mk_client(tok)?;
                    self.mgraph_upn = crabmgraphrs::get_upn(&self.mgraph_cli)?;
                    println!("{}{}", "UPN: ", self.mgraph_upn);
                    //let dirnm = filename_enc::encode(&self.mgraph_upn);
                    //self.mgraph_dir = self.infom_dir.join(dirnm);
                    //fs::create_dir_all(&self.mgraph_dir)?;
                }
                "ss-d" => {
                    if self.dx_email.is_empty() {
                        coutln!("`try-d` needs to successfully run once first.");
                        continue;
                    }
                    remote::list_dx(self)?;
                }
                "ss-m" => {
                    //note this scans superficially (does not calc hash of file)
                    //undone scan m
                }
                "sc" | "scan" => {
                    //scan all entfos with default hardware id
                    self.scan()?;
                }
                "mount-hardware" => {
                    //undone recognize entfos for hardware(s) that are different from your default hardware id
                }
                "l" => {
                    self.show_recs_in_filter_buf()?;
                }
                "entfos" => {
                    self.print_entfos();
                }
                "write" => {
                    self.write_all_to_info()?;
                }
                "reset" => {
                    self.clear_filter_buf()?;
                    self.def.filter_buf.clear();
                }
                "all" => {
                    for selid in &mut self.def.filter_buf {
                        *selid = -selid.abs();
                    }
                }
                "deselect" => {
                    for selid in &mut self.def.filter_buf {
                        *selid = selid.abs();
                    }
                }
                "fsdel" => {
                    self.fsdel()?;
                }
                "del" => {
                    self.del()?;
                }
                "exit" | "quit" => {
                    return Ok(());
                }
                "lbl" => {
                    println!("{}{}", "Chosen label: ", self.def.chosen_lbl);
                }
                "lbls" => {
                    self.lbls()?;
                }
                "+" => {
                    if self.def.chosen_lbl.is_empty() {
                        println!("{}", "Chosen label is empty");
                        continue;
                    }
                    let l_lbl = mem::take(&mut self.def.chosen_lbl);
                    self.lbl_add(&l_lbl)?;
                    self.def.chosen_lbl = l_lbl;
                }
                "-" => {
                    if self.def.chosen_lbl.is_empty() {
                        println!("{}", "Chosen label is empty");
                        continue;
                    }
                    let l_lbl = mem::take(&mut self.def.chosen_lbl);
                    self.lbl_remove(&l_lbl)?;
                    self.def.chosen_lbl = l_lbl;
                }
                "" => {
                    coutln!("Empty input.");
                    //note if you delete "" case here, the .as_bytes()[0] below can panic
                }
                _ => {
                    if self.iline.as_bytes()[0] == b'+' {
                        self.plus()?;
                    } else if self.iline.as_bytes()[0] == b'-' {
                        self.minus()?;
                    } else if self.iline.as_bytes()[0] == b'/' {
                        self.slash()?;
                    } else if self.iline.bytes().all(|c| c.is_ascii_digit()) {
                        let idx: usize = match self.iline.parse::<usize>() {
                            Err(_) => {
                                println!("{}", "Invalid index");
                                continue;
                            }
                            Ok(l_idx) => l_idx,
                        };
                        if let Some(selid) = self.def.filter_buf.get_mut(idx) {
                            *selid *= -1;
                            self.show_recs_in_filter_buf()?;
                        } else {
                            println!("{}", "Invalid index");
                        }
                    } else if let Some(sidx) = self.iline.find(' ') {
                        self.iline_argidx = sidx + 1;
                        struct Tst {
                            sidx: usize,
                        }
                        impl Tst {
                            fn get<'c>(&self, tctx: &'c Ctx) -> &'c str {
                                &tctx.iline[self.sidx + 1..]
                            }
                        }
                        let rest_tstr = Tst { sidx };
                        match &self.iline[..sidx] {
                            "scan-entfo" => {
                                //let c_get_entfoid = move |tctx| -> &str {get(tctx, sidx)};
                                //let get_entfo2: for<'b> fn(&'b CtxDef, usize)-> &'b str = |tctx: &CtxDef, sss: usize| -> &str {&tctx.iline[sss + 1..]};
                                //note this cmd can be utilized for scanning ENTFOs that are not under default hardware id
                                let entfostr: String = self.iline[self.iline_argidx..].to_owned();
                                if let Some(_fsp) = self.custentfos_settings.get(&entfostr) {
                                    //*****
                                    let l_custentfos_settings =
                                        mem::take(&mut self.custentfos_settings);
                                    let res =
                                        self.scan_single_entfo(&l_custentfos_settings, &entfostr);
                                    self.custentfos_settings = l_custentfos_settings;
                                    res?;
                                    //*****
                                } else {
                                    warn!("{}", "Entfo is not recognized.");
                                    continue;
                                }
                            }
                            "where" => {
                                print!("{}", "ORDER BY (unordered if empty): ");
                                io::stdout().flush()?;
                                let mut orderby: String;
                                match self.stdin_w.lines.next() {
                                    None => {
                                        warn!("{}", "Unexpected stdin EOF");
                                        return Ok(());
                                    }
                                    Some(Err(err)) => {
                                        let l_err: std::io::Error = err;
                                        return Err(l_err.into());
                                    }
                                    Some(Ok(linestr)) => {
                                        orderby = linestr;
                                    }
                                }
                                if !orderby.is_empty() {
                                    orderby.insert_str(0, " order by ");
                                }
                                let sqlstr = if self.filter_buf.is_empty() {
                                    "SELECT tmpid from files where ".to_owned()
                                        + rest_tstr.get(self)
                                        + &orderby
                                } else {
                                    "SELECT tmpid from files where tmpid in(select tid from filter_buf) and (".to_owned()
							+ rest_tstr.get(self) +")" + &orderby
                                };
                                let tids = {
                                    let mut stmt = match self.db.prepare(&sqlstr) {
                                        Ok(statem) => statem,
                                        Err(err) => {
                                            warn!("{}{}", "Err during preparing stmt: ", err);
                                            continue;
                                        }
                                    };
                                    //let mut stmt: rusqlite::Statement;
                                    //let mut rows = stmt.query([])?;
                                    //self.iter_rows_to_update_filter_buf(rows)?;
                                    query_n_collect_into_vec_i64(stmt.query([]))?
                                };
                                self.iter_rows_to_update_filter_buf(tids)?;
                            }
                            "lbl" => {
                                self.lbl_input();
                            }
                            "lbls" => {
                                if !self.lbls_search()? {
                                    return Ok(());
                                }
                            }
                            "desc" => {
                                self.set_desc()?;
                            }
                            "oldfilename" => {
                                self.set_oldfilename()?;
                            }
                            "downloadurl" => {
                                self.set_downloadurl()?;
                            }
                            "archivefilename" => {
                                self.set_archivefilename()?;
                            }
                            "copyright" => {
                                self.set_copyright()?;
                            }
                            "fsdel" => {
                                let mut t_eof = false;
                                self.fsdel_pattern(&mut t_eof)?;
                                if t_eof {
                                    return Ok(());
                                }
                            }
                            _ => {
                                warn!("{}", "Command not recognized.");
                                continue;
                            }
                        }
                    } else {
                        warn!("{}", "Command not recognized.");
                        continue;
                    }
                }
            }
        }
    }

    fn hnd_main(&mut self) -> Result<(), CustomErr> {
        use std::fs::*;
        self.home_dir = dirs::home_dir().ok_or("Failed to get home directory.")?;
        if !real_dir_without_symlink(&self.home_dir) {
            error!("{}", "Failed to recognize the home dir as folder.");
            return Err(CustomErr {});
        }
        self.everycom = self.home_dir.join(".everycom");
        self.app_support_dir = self.everycom.join(PKG_NAME);
        self.sync_dir = self.app_support_dir.join("sd");
        self.info_pair_file = self.sync_dir.join("info_pair");
        self.info_dir = self.sync_dir.join("info");
        fs::create_dir_all(&self.info_dir)?;
        //self.infod_dir = self.sync_dir.join("infod");
        //fs::create_dir_all(&self.infod_dir)?;
        //self.infom_dir = self.sync_dir.join("infom");
        //fs::create_dir_all(&self.infom_dir)?;
        self.local_settings_file = self.app_support_dir.join("settings");
        self.info_settings_file = self.info_dir.join("info_settings");
        self.custentfos_settings_file = self.app_support_dir.join("custentfos_settings");
        //self.mentfos_settings_file = self.app_support_dir.join("mentfos_settings");
        if self.local_settings_file.exists() {
            if !self.local_settings_file.is_file()
            /*//note it traverses symlinks*/
            {
                error!(
                    "{}",
                    "Failed to recognize the local settings file as regular file."
                );
                return Err(CustomErr {});
            }
            let settings = File::open(&self.local_settings_file)?;
            let reader = std::io::BufReader::new(settings);
            self.local_settings = serde_json::from_reader(reader)?;
        }
        if self.info_settings_file.exists() {
            if !self.info_settings_file.is_file()
            /*//note it traverses symlinks*/
            {
                error!(
                    "{}",
                    "Failed to recognize the info settings file as regular file."
                );
                return Err(CustomErr {});
            }
            let settings = File::open(&self.info_settings_file)?;
            let reader = std::io::BufReader::new(settings);
            self.info_settings = serde_json::from_reader(reader)?;
        }
        if self.custentfos_settings_file.exists() {
            if !self.custentfos_settings_file.is_file()
            /*//note it traverses symlinks*/
            {
                error!(
                    "{}",
                    "Failed to recognize the custentfos settings file as regular file."
                );
                return Err(CustomErr {});
            }
            let settings = File::open(&self.custentfos_settings_file)?;
            let reader = std::io::BufReader::new(settings);
            self.custentfos_settings = serde_json::from_reader(reader)?;
        } else {
            coutln!("No custentfos_settings! You might want to add one! It should be JSON object containing one or multiple ENTFOs. Here is an ENTFO example: ", r##""opt_filelst":{"active":true, "hardware_id":"MyHardware", "abs_path":"/MySpecial/Folder", "efolder":{"include":true}}"##);
        }
        //if self.mentfos_settings_file.exists() {
        //    if !self.mentfos_settings_file.is_file()
        //    /*//note it traverses symlinks*/
        //    {
        //        error!(
        //            "{}",
        //            "Failed to recognize the mentfos settings file as regular file."
        //        );
        //        return Err(CustomErr {});
        //    }
        //    let settings = File::open(&self.mentfos_settings_file)?;
        //    let reader = std::io::BufReader::new(settings);
        //    self.mentfos_settings = serde_json::from_reader(reader)?;
        //}
        self.canonicalize_paths_and_sort_matches()?;
        //
        self.sync_info()?;
        self.print_entfos_for_default_hardware();
        while self.local_settings.default_hardware_id.is_empty() {
            print!("{}", "Default Hardware ID not set. Please input: ");
            io::stdout().flush()?;
            match self.stdin_w.lines.next() {
                None => {
                    warn!("{}", "Unexpected stdin EOF");
                    return Ok(());
                }
                Some(Err(err)) => {
                    let l_err: std::io::Error = err;
                    return Err(l_err.into());
                }
                Some(Ok(linestr)) => {
                    let l_linestr: String = linestr;
                    if l_linestr.is_empty() {
                        continue;
                    }
                    self.local_settings.default_hardware_id = l_linestr;
                    self.write_local_settings()?;
                }
            }
        }
        init_sqlite(&self.db)?;
        self.tidx_st = util::read_all_infojson(&mut self.db, &self.def.info_dir)?;
        println!("{}", "Finished reading info.");
        self.cmd_loop()?;
        self.write_all_to_info()?; //todo skip if user wants to discard changes?
        self.sync_info()?;
        /*
            if 0 == self.get_num_of_entfos_of_default_hardware() {
                print!(
                    "{}",
                    "Zero entfo folders found for default hardware, Please input a folder path as entfo folder: "
                );
                use std::io::Write;
                io::stdout().flush()?;
                match self.stdin_w.lines.next() {
                    None => {
                        warn!("{}", "Unexpected stdin EOF");
                        return Ok(());
                    }
                    Some(Err(err)) => {
                        let l_err: std::io::Error = err;
                        error!("{:?}\n{}", l_err, Backtrace::force_capture());
                        return Err(CustomErr {});
                    }
                    Some(Ok(linestr)) => {
                        let l_linestr: String = linestr;
                        let new_entfo: PathBuf = fs::canonicalize(l_linestr)?;
                        if new_entfo.is_file() || new_entfo.is_symlink() {
                            warn!("{}", "Specified entfo is regular file or symlink.");
                            return Ok(());
                        }
                        {
                            let _read_dir: std::fs::ReadDir = new_entfo.read_dir()?; //try reading it once
                                                                                    //_read_dir.next();
                        }
                //self.custentfos_settings.insert();
                    }
                }
            }
        */
        return Ok(());
    }
    fn write_local_settings(&self) -> Result<(), CustomErr> {
        use std::fs::*;
        let mut file = File::create(&self.local_settings_file)?;
        serde_json::to_writer_pretty(&file, &self.local_settings)?;
        use std::io::prelude::*;
        file.write_all(b"\n")?;
        Ok(())
    }
    //fn get_num_of_entfos_of_default_hardware(&self) -> u32 {
    //    //fixme include also mentfos_settings
    //    let mut retval: u32 = 0;
    //    for (_entkey, entval) in &self.custentfos_settings {
    //        if entval.hardware_id == self.local_settings.default_hardware_id {
    //            retval += 1
    //        }
    //    }
    //    return retval;
    //}
    fn print_entfos_for_default_hardware(&self) {
        //fixme include also mentfos_settings
        println!("{}", "*** ENTFOS ***");
        for (entkey, entval) in &self.custentfos_settings {
            println!("{}, {}, {}", entval.hardware_id, entkey, entval.abs_path);
        }
        println!("{}", "*************");
    }
    fn print_entfos(&self) {
        for (entkey, entval) in &self.def.custentfos_settings {
            print!("{}", "****");
            if entval.hardware_id == self.local_settings.default_hardware_id {
                println!("{}", " DEFAULT HARDWARE");
            } else {
                println!();
            }
            println!("{}", entkey);
            println!("{:?}", entval);
        }
        //println!("{:?}", self.def.custentfos_settings);
    }
    fn sync_info(&mut self) -> Result<(), CustomErr> {
        match self.local_settings.sync_method.as_str() {
            "" => {
                return Ok(());
            }
            "git" => {
                self.sync_via_git()?;
                return Ok(());
            }
            _ => {
                warn!("{}", "sync_method not recognized. Ignoring it.");
                return Ok(());
            }
        }
    }

    fn git_chk_email_n_name(&self) -> Result<(), CustomSimpleErr> {
        println!("{}", "****** GIT CHK ******");
        let out = Command::new("git")
            .current_dir(&self.sync_dir)
            .arg("config")
            .arg("user.email")
            .output()?;
        println!("STDOUT\n{}", String::from_utf8_lossy(&out.stdout));
        println!("STDERR\n{}", String::from_utf8_lossy(&out.stderr));
        if out.stdout.is_empty() {
            error!("{}", "git output is empty",);
            return Err(CustomSimpleErr {});
        }
        if !out.status.success() {
            error!("{}", "git exit status is not successful",);
            return Err(CustomSimpleErr {});
        }
        let out = Command::new("git")
            .current_dir(&self.sync_dir)
            .arg("config")
            .arg("user.name")
            .output()?;
        println!("STDOUT\n{}", String::from_utf8_lossy(&out.stdout));
        println!("STDERR\n{}", String::from_utf8_lossy(&out.stderr));
        if out.stdout.is_empty() {
            error!("{}", "git output is empty",);
            return Err(CustomSimpleErr {});
        }
        if !out.status.success() {
            error!("{}", "git exit status is not successful",);
            return Err(CustomSimpleErr {});
        }
        println!("{}", "****** GIT CHK DONE ******");
        return Ok(());
    }
    fn sync_via_git(&mut self) -> Result<(), CustomSimpleErr> {
        'gitcmd: {
            println!("{}", "****** GIT PULL ******");
            let out = Command::new("git")
                .current_dir(&self.sync_dir)
                .arg("pull")
                .output()?;
            println!("STDOUT\n{}", String::from_utf8_lossy(&out.stdout));
            println!("STDERR\n{}", String::from_utf8_lossy(&out.stderr));
            if !out.status.success() {
                return Err("git exit status is not successful".into());
            }
            println!("{}", "****** GIT ADD ******");
            let out = Command::new("git")
                .current_dir(&self.sync_dir)
                .arg("add")
                .arg("-A")
                .output()?;
            println!("STDOUT\n{}", String::from_utf8_lossy(&out.stdout));
            println!("STDERR\n{}", String::from_utf8_lossy(&out.stderr));
            if !out.status.success() {
                return Err("git exit status is not successful".into());
            }
            println!("{}", "****** GIT DIFF ******");
            let out = Command::new("git")
                .current_dir(&self.sync_dir)
                .arg("diff")
                .arg("--cached")
                //.arg("--name-only")
                .output()?;
            println!("STDOUT\n{}", String::from_utf8_lossy(&out.stdout));
            println!("STDERR\n{}", String::from_utf8_lossy(&out.stderr));
            if !out.status.success() {
                return Err("git exit status is not successful".into());
            }
            if out.stdout.is_empty() {
                break 'gitcmd;
            }
            if !self.already_checked_git_email_n_name {
                self.already_checked_git_email_n_name = true;
                self.git_chk_email_n_name()?;
            }
            println!("{}", "****** GIT COMMIT ******");
            let out = Command::new("git")
                .current_dir(&self.sync_dir)
                .arg("commit")
                .arg("--allow-empty-message")
                .arg("-m")
                .arg("")
                .output()?;
            println!("STDOUT\n{}", String::from_utf8_lossy(&out.stdout));
            println!("STDERR\n{}", String::from_utf8_lossy(&out.stderr));
            if !out.status.success() {
                return Err("git exit status is not successful".into());
            }
            println!("{}", "****** GIT PUSH ******");
            let out = Command::new("git")
                .current_dir(&self.sync_dir)
                .arg("push")
                .arg("-u")
                .arg("origin")
                .arg("HEAD") //https://stackoverflow.com/questions/23241052/what-does-git-push-origin-head-mean
                .output()?;
            println!("STDOUT\n{}", String::from_utf8_lossy(&out.stdout));
            println!("STDERR\n{}", String::from_utf8_lossy(&out.stderr));
            if !out.status.success() {
                return Err("git exit status is not successful".into());
            }
        }
        const SYNC_DONE_STR: &str = "****** SYNC DONE ******";
        println!("{}", SYNC_DONE_STR);
        Ok(())
    }
}

fn main() -> ExitCode {
    env::set_var("RUST_BACKTRACE", "1"); //? not 100% sure this has 0 impact on performance? Maybe setting via command line instead of hardcoding is better?
                                         //env::set_var("RUST_LIB_BACKTRACE", "1");//? this line is useless?
                                         ////
    env::set_var("RUST_LOG", "debug"); //note this line must be above logger init.
                                       //some libraries might print huge amount of data on "trace", e.g. reqwest, thus here is using "debug" for default
    env_logger::init();
    ////

    //todo first arg as file to open, if not specified, list recent files for selection
    let args: Vec<String> = env::args().collect(); //Note that std::env::args will panic if any argument contains invalid Unicode.
    defer! {
    if std::thread::panicking() {
            println!("{}", "PANICKING");
    }
        println!("{}", "ALL DONE");
    }

    if main_inner(args).is_err() {
        return ExitCode::from(1);
    }
    ExitCode::from(0)
}
fn main_inner(args: Vec<String>) -> Result<(), CustomErr> {
    use rusqlite::Connection;
    let conn = Connection::open_in_memory()?;
    let mut ctx = Ctx::new(args, conn);
    ctx.hnd_main()?;
    Ok(())
}
fn calc_hash(hasher: &mut sha2::Sha256, pat: &std::path::Path) -> Result<[u8; 32], CustomErr> {
    use sha2::Digest;
    use std::fs::*;
    //use sha2::{Sha256, Digest};
    //let mut hasher = Sha256::new();
    let mut file = File::open(pat)?;
    let _bytes_written = io::copy(&mut file, hasher)?;
    //let hash_bytes = hasher.finalize();
    let hash_bytes = hasher.finalize_reset();
    //let hash_bytes = hasher.finalize_boxed_reset();
    //use base64::{engine::general_purpose, Engine as _};
    //return Ok(general_purpose::STANDARD_NO_PAD.encode(hash_bytes));
    let retval: [u8; 32] = hash_bytes.as_slice().try_into()?;
    Ok(retval)
}
