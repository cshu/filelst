//use crate::common::*;
//use crate::sqlite_util::*;

use crabrs::*;
use crabsqliters::*;
use log::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::io::Write;
use std::path::PathBuf;
use std::*;

const MAX_NUM_OF_ELEMS_IN_ONE_INFO_T: usize = 32;
const _: () = assert!(MAX_NUM_OF_ELEMS_IN_ONE_INFO_T >= 2, "Constraint on const");
//note if you have a newer version compiled with a MAX_NUM_OF_ELEMS_IN_ONE_INFO_T smaller than an old version, then previously generated info json files are likely to contain elements exceeding MAX_NUM_OF_ELEMS_IN_ONE_INFO_T

//#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
//pub struct InfoJson {
//    elems: Vec<InfoJsonElem>,
//}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct InfoJsonElem {
    #[serde(skip)]
    tmpid: i64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[serde(default)]
    old: Vec<String>, //note containing previous hash of previous content occupying the same path
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[serde(default)]
    pf: Vec<Vec<serde_json::Value>>, //note elem is `Entfo folder ID`+`rel path relative to the Entfo`+`actual modtime of file on disk`
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[serde(default)]
    lbls: Vec<String>,
    #[serde(skip_serializing_if = "String::is_empty")]
    #[serde(default)]
    desc: String,
    #[serde(default)]
    size: i64,
    #[serde(default)]
    mtime: i64, //note this is the earliest time you are sure since when it has never been modifed, not physical mtime
    #[serde(skip_serializing_if = "String::is_empty")]
    #[serde(default)]
    hash: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    #[serde(default)]
    old_filename: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    #[serde(default)]
    download_url: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    #[serde(default)]
    archive_filename: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    #[serde(default)]
    copyright: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TIdx {
    pub t_idx: u32,
    pub spacious_t: BTreeSet<u32>,
}
impl TIdx {
    //pub fn get_curr_n_increment(&mut self)-> u32{
    //	let retval = self.t_idx;
    //	self.t_idx+=1;
    //	retval
    //}
    pub fn new_space(&mut self) -> u32 {
        let retval: u32 = self.t_idx;
        self.spacious_t.insert(retval);
        self.t_idx += 1;
        retval
    }
    pub fn mk_treefile_path(&self, pathbuild: &mut PathBuf) {
        mk_treefile_name(self.t_idx, pathbuild);
    }
    pub fn t_idx_i64(&self) -> i64 {
        self.t_idx as i64
    }
}

pub fn read_treefile(pathbuild: &PathBuf) -> CustRes<Vec<InfoJsonElem>> {
    use std::fs::*;
    let treefile = File::open(pathbuild)?;
    let reader = io::BufReader::new(treefile);
    let retval: Vec<InfoJsonElem> = serde_json::from_reader(reader)?;
    Ok(retval)
}

pub fn base64_to_hash<T: AsRef<[u8]>>(input: T) -> CustRes<Vec<u8>> {
    use base64::{engine::general_purpose, Engine as _};
    let retval = general_purpose::STANDARD_NO_PAD.decode(input)?;
    Ok(retval)
}

pub fn hash_to_base64<T: AsRef<[u8]>>(input: T) -> String {
    use base64::{engine::general_purpose, Engine as _};
    general_purpose::STANDARD_NO_PAD.encode(input)
}

pub fn read_all_infojson(db: &mut rusqlite::Connection, folder: &PathBuf) -> CustRes<TIdx> {
    let mut tmpid: i64 = 0;
    let mut retval = TIdx::default();
    let mut pathbuild = PathBuf::default();
    let tx = db.transaction()?; //note when this var drops, it calls roolback by default. (Unless consumed via `commit`)
    let mut oldh: Vec<(i64, Vec<String>)> = vec![];
    loop {
        retval.t_idx += 1;
        //let t_idx = retval.get_curr_n_increment();
        pathbuild.clone_from(folder);
        retval.mk_treefile_path(&mut pathbuild);
        if !real_reg_file_without_symlink(&pathbuild) {
            break;
        }
        let tjson = read_treefile(&pathbuild)?;
        if tjson.len() < MAX_NUM_OF_ELEMS_IN_ONE_INFO_T {
            retval.spacious_t.insert(retval.t_idx);
        }
        for jobj in tjson {
            tmpid += 1;
            {
                tx.prepare_cached(
                    "insert into files values(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                )?
                .execute((
                    tmpid,
                    jobj.size,
                    jobj.mtime,
                    base64_to_hash(jobj.hash)?,
                    jobj.desc,
                    jobj.old_filename,
                    jobj.download_url,
                    jobj.archive_filename,
                    jobj.copyright,
                    retval.t_idx_i64(),
                ))?;
            }
            if !jobj.old.is_empty() {
                oldh.push((tmpid, jobj.old));
            }
            {
                let mut stmt = tx.prepare_cached("insert into lbls values(?1, ?2)")?;
                for lbl in jobj.lbls {
                    stmt.execute((tmpid, lbl))?;
                }
            }
            let mut stmt = tx.prepare_cached("insert into fsfiles values(?1, ?2, ?3, ?4)")?;
            //use serde_json::*;
            for l_fsfile in jobj.pf {
                let rid = l_fsfile.get(0).ok_or("Invalid pf")?;
                let ridstr = match rid {
                    serde_json::Value::String(rstr) => rstr,
                    _ => {
                        return Err("Invalid pf".into());
                    }
                };
                let rel = l_fsfile.get(1).ok_or("Invalid pf")?;
                let relstr = match rel {
                    serde_json::Value::String(rstr) => rstr,
                    _ => {
                        return Err("Invalid pf".into());
                    }
                };
                let pftime = l_fsfile.get(2);
                let mtime: i64;
                if let Some(modtime) = pftime {
                    mtime = match modtime {
                        serde_json::Value::Number(timenum) => {
                            timenum.as_i64().ok_or("Invalid pf")?
                        }
                        _ => {
                            return Err("Invalid pf".into());
                        }
                    }
                } else {
                    mtime = jobj.mtime;
                }
                stmt.execute((tmpid, ridstr, relstr, mtime))?;
            }
            //for (fsfilekey, fsfileval, modtime) in jobj.pf {
            //    let l_fsfilekey: String = fsfilekey;
            //    let l_fsfileval: String = fsfileval;
            //    let l_modtime: i64 = modtime;
            //    stmt.execute((tmpid, l_fsfilekey, l_fsfileval, l_modtime))?;
            //}
        }
    }
    {
        let mut stmt =
            tx.prepare_cached("insert into oldh select ?1,tmpid from files where hash=?2")?;
        for old_hash in oldh {
            for one_hash in old_hash.1 {
                if 1 != stmt.execute((old_hash.0, base64_to_hash(one_hash)?))? {
                    return Err("Old hash dangling".into());
                }
            }
        }
    }
    tx.commit()?;
    Ok(retval)
}

pub fn check_fsfile_existence(db: &rusqlite::Connection, rel: &str, entfo: &str) -> CustRes<bool> {
    let mut cached_stmt = db.prepare_cached("select 0 from fsfiles where rel=?1 and entfo=?2")?;
    let mut rows = cached_stmt.query((rel, entfo))?;
    Ok(rows.next()?.is_some())
}

pub fn ins_fsf(
    db: &rusqlite::Connection,
    tmpid: i64,
    entfo: &str,
    rel: &str,
    mtime: i64,
) -> CustRes<()> {
    let mut cached_stmt = db.prepare_cached("insert into fsfiles values(?1,?2,?3,?4)")?;
    cached_stmt.execute((tmpid, entfo, rel, mtime))?;
    Ok(())
}

pub fn ins_file(db: &rusqlite::Connection, flen: i64, mtime: i64, chash: [u8; 32]) -> CustRes<i64> {
    let tmpid: i64 = super::get_avail_tmpid(db)?;
    let mut cached_stmt =
        db.prepare_cached("insert into files values(?1,?2,?3,?4,'','','','','',0)")?;
    cached_stmt.execute((tmpid, flen, mtime, chash))?;
    Ok(tmpid)
}

pub fn millis2display(ms: i64) -> String {
    use chrono::prelude::*;
    let ndt = NaiveDateTime::from_timestamp_millis(ms);
    let naive = match ndt {
        None => {
            return "FAILED TO CONV MS TO STR".to_owned();
        }
        Some(ndt_v) => ndt_v,
    };
    let datetime: DateTime<Utc> = Utc.from_utc_datetime(&naive);
    datetime.to_string()
}

pub fn tidx_touch(con: &mut crate::Ctx, tids: &[i64]) -> CustRes<()> {
    let mut cached_stmt = con
        .db
        .prepare_cached("select tidx from files where tmpid=?1")?;
    for tid in tids {
        let l_tid: i64 = *tid;
        let mut rows = cached_stmt.query((l_tid,))?;
        let tidx: i64 = rows
            .next()?
            .ok_or("Failed to find tmpid in files for del")?
            .get(0)?;
        //note not inserting into spacious_t. Because if you have a newer version compiled with a MAX_NUM_OF_ELEMS_IN_ONE_INFO_T smaller than an old version, then previously generated info json files are likely to contain elements exceeding MAX_NUM_OF_ELEMS_IN_ONE_INFO_T
        con.def.tidx_modified.insert(tidx as u32); //note inserting 0 is fine
    }
    Ok(())
}

impl crate::Ctx {
    pub fn write_all_to_info(&mut self) -> CustRes<()> {
        println!("{}", "**** Begin writing to info ****");
        let mut new_tmpid = Vec::<i64>::new();
        {
            let mut cached_stmt = self
                .db
                .prepare_cached("select tmpid from files where tidx=0")?;
            let mut rows = cached_stmt.query([])?;
            while let Some(row) = rows.next()? {
                let tid: i64 = row.get(0)?;
                new_tmpid.push(tid);
            }
        }
        let mut space_left = Vec::<u32>::with_capacity(self.tidx_st.spacious_t.len());
        {
            let mut cached_stmt = self
                .db
                .prepare_cached("select count(*) from files where tidx=?1")?;
            for space in &self.tidx_st.spacious_t {
                let space_t: i64 = *space as i64;
                let elem_count: i64 = cached_stmt.query((space_t,))?.next()?.unwrap().get(0)?;
                space_left.push((MAX_NUM_OF_ELEMS_IN_ONE_INFO_T as u32) - (elem_count as u32));
            }
        }
        let tmpdb = &self.db;
        let tmpdef = &mut self.def;
        let tmptidx = &mut tmpdef.tidx_st;
        let tmptmod = &mut tmpdef.tidx_modified;
        tmptmod.remove(&0); //note when you are adding, maybe you did not check 0
        {
            let mut cached_stmt =
                tmpdb.prepare_cached("update files set tidx=?1 where tmpid=?2")?;
            for new_tid in new_tmpid {
                let tidx_to_use: i64;
                if space_left.is_empty() {
                    space_left.push(MAX_NUM_OF_ELEMS_IN_ONE_INFO_T as u32 - 1);
                    tidx_to_use = tmptidx.new_space() as i64;
                } else {
                    tidx_to_use = *tmptidx.spacious_t.last().unwrap() as i64;
                    let t_space_left = space_left.last_mut().unwrap();
                    if *t_space_left == 1 {
                        space_left.pop();
                        tmptidx.spacious_t.pop_last();
                    } else {
                        *t_space_left -= 1;
                    }
                }
                cached_stmt.execute((tidx_to_use, new_tid))?;
                tmptmod.insert(tidx_to_use as u32);
            }
        }
        let mut elems = Vec::<InfoJsonElem>::with_capacity(MAX_NUM_OF_ELEMS_IN_ONE_INFO_T);
        let mut pathbuild = PathBuf::default();
        for tmod in &self.def.tidx_modified {
            elems.clear();
            {
                let mut cached_stmt = self.db.prepare_cached(
                    "select * from files where tidx=?1 order by size, mtime, hash",
                )?;
                let tmodified: i64 = *tmod as i64;
                let mut rows = cached_stmt.query((tmodified,))?;
                while let Some(row) = rows.next()? {
                    let mut tje = InfoJsonElem {
                        tmpid: row.get(0)?,
                        size: row.get(1)?,
                        mtime: row.get(2)?,
                        ..Default::default()
                    };
                    let hash: Vec<u8> = row.get(3)?;
                    tje.hash = hash_to_base64(hash);
                    tje.desc = row.get(4)?;
                    tje.old_filename = row.get(5)?;
                    tje.download_url = row.get(6)?;
                    tje.archive_filename = row.get(7)?;
                    tje.copyright = row.get(8)?;
                    elems.push(tje);
                }
            }
            {
                let mut cached_stmt = self.db.prepare_cached(
                    "select hash from files where tmpid in(select oldi from oldh where newi=?1)",
                )?;
                for elem in &mut elems {
                    let tmpid: i64 = elem.tmpid;
                    let mut rows = cached_stmt.query((tmpid,))?;
                    while let Some(row) = rows.next()? {
                        let hash: Vec<u8> = row.get(0)?;
                        elem.old.push(hash_to_base64(hash));
                    }
                    elem.old.sort_unstable();
                }
            }
            {
                let mut cached_stmt = self.db.prepare_cached(
                    "select entfo,rel,mtime from fsfiles where tmpid=?1 order by entfo,rel",
                )?;
                for elem in &mut elems {
                    let tmpid: i64 = elem.tmpid;
                    let mut rows = cached_stmt.query((tmpid,))?;
                    while let Some(row) = rows.next()? {
                        let entfoid: String = row.get(0)?;
                        let rel: String = row.get(1)?;
                        let ms: i64 = row.get(2)?;
                        use serde_json::json;
                        let entfo_v = json!(entfoid);
                        let rel_v = json!(rel);
                        if ms == elem.mtime {
                            elem.pf.push(vec![entfo_v, rel_v]);
                        } else {
                            elem.pf.push(vec![entfo_v, rel_v, json!(ms)]);
                        }
                    }
                }
            }
            {
                let mut cached_stmt = self
                    .db
                    .prepare_cached("select lbl from lbls where tmpid=?1 order by lbl")?;
                for elem in &mut elems {
                    let tmpid: i64 = elem.tmpid;
                    let mut rows = cached_stmt.query((tmpid,))?;
                    while let Some(row) = rows.next()? {
                        let lbl: String = row.get(0)?;
                        elem.lbls.push(lbl);
                    }
                }
            }
            pathbuild.clone_from(&self.def.info_dir);
            mk_treefile_name(*tmod, &mut pathbuild);
            write_treefile(&pathbuild, &elems)?;
        }
        self.def.tidx_modified.clear();
        println!("{}", "**** End writing to info ****");
        Ok(())
    }
    pub fn set_desc(&mut self) -> CustRes<()> {
        let iline = mem::take(&mut self.iline);
        let newval: &str = &iline[self.iline_argidx..];
        if self.exec_with_str_selection("update files set desc=?1 where tmpid=?2", newval)? {
            println!("{}{}", "New value set: ", newval);
        }
        Ok(())
    }
    pub fn set_oldfilename(&mut self) -> CustRes<()> {
        let iline = mem::take(&mut self.iline);
        let newval: &str = &iline[self.iline_argidx..];
        if self
            .exec_with_str_selection("update files set old_filename=?1 where tmpid=?2", newval)?
        {
            println!("{}{}", "New value set: ", newval);
        }
        Ok(())
    }
    pub fn set_downloadurl(&mut self) -> CustRes<()> {
        let iline = mem::take(&mut self.iline);
        let newval: &str = &iline[self.iline_argidx..];
        if self
            .exec_with_str_selection("update files set download_url=?1 where tmpid=?2", newval)?
        {
            println!("{}{}", "New value set: ", newval);
        }
        Ok(())
    }
    pub fn set_archivefilename(&mut self) -> CustRes<()> {
        let iline = mem::take(&mut self.iline);
        let newval: &str = &iline[self.iline_argidx..];
        if self.exec_with_str_selection(
            "update files set archive_filename=?1 where tmpid=?2",
            newval,
        )? {
            println!("{}{}", "New value set: ", newval);
        }
        Ok(())
    }
    pub fn set_copyright(&mut self) -> CustRes<()> {
        let iline = mem::take(&mut self.iline);
        let newval: &str = &iline[self.iline_argidx..];
        if self.exec_with_str_selection("update files set copyright=?1 where tmpid=?2", newval)? {
            println!("{}{}", "New value set: ", newval);
        }
        Ok(())
    }
    pub fn exec_with_str_selection(&mut self, sqlstr: &str, strval: &str) -> CustRes<bool> {
        let tids = self.mk_vec_of_selection();
        if tids.is_empty() {
            return Ok(false);
        }
        tidx_touch(self, &tids)?;
        let mut cached_stmt = self.db.prepare_cached(sqlstr)?;
        for tid in tids {
            let l_tid: i64 = tid;
            cached_stmt.execute((strval, l_tid))?;
        }
        Ok(true)
    }
    pub fn lbl_input(&mut self) {
        let pat: &str = &self.iline[self.iline_argidx..];
        if pat.is_empty() {
            println!("{}", "Cannot be empty.");
            return;
        }
        self.def.chosen_lbl = pat.to_owned();
        println!("{}{}", "Chosen: ", self.def.chosen_lbl);
    }
    pub fn lbls(&self) -> CustRes<()> {
        let mut lbl_distinct: Vec<String> = vec![];
        let mut cached_stmt = self.db.prepare_cached("select distinct lbl from lbls")?;
        let mut rows = cached_stmt.query([])?;
        while let Some(row) = rows.next()? {
            let lbl: String = row.get(0)?;
            lbl_distinct.push(lbl);
        }
        lbl_distinct.sort_unstable();
        println!("{:?}", lbl_distinct);
        Ok(())
    }
    pub fn lbls_search(&mut self) -> CustRes<bool> {
        let pat: &str = &self.iline[self.iline_argidx..];
        let mut lbl_distinct: Vec<String> = vec![];
        {
            let mut cached_stmt = self
                .db
                .prepare_cached("select distinct lbl from lbls where instr(lbl,?1)")?;
            let mut rows = cached_stmt.query((pat,))?;
            while let Some(row) = rows.next()? {
                let lbl: String = row.get(0)?;
                lbl_distinct.push(lbl);
            }
        }
        if lbl_distinct.is_empty() {
            println!("{}", "No match.");
            return Ok(true);
        }
        if lbl_distinct.len() == 1 {
            self.def.chosen_lbl = lbl_distinct.into_iter().next().unwrap();
            println!("{}{}", "Chosen: ", self.def.chosen_lbl);
            return Ok(true);
        }
        lbl_distinct.sort_unstable();
        for (idx, l_lbl) in lbl_distinct.iter().enumerate() {
            println!("{} {}", idx, l_lbl);
        }
        print!("{}", "Please choose: ");
        io::stdout().flush()?;
        let choice: String = match self.stdin_w.lines.next() {
            None => {
                warn!("{}", "Unexpected stdin EOF");
                return Ok(false);
            }
            Some(Err(err)) => {
                let l_err: io::Error = err;
                return Err(l_err.into());
            }
            Some(Ok(linestr)) => linestr,
        };
        let idx: usize = match choice.parse::<usize>() {
            Err(_) => {
                println!("{}", "Invalid index");
                return Ok(true);
            }
            Ok(l_idx) => l_idx,
        };
        if let Some(sel_lbl) = lbl_distinct.into_iter().nth(idx) {
            self.def.chosen_lbl = sel_lbl;
            println!("{}{}", "Chosen: ", self.def.chosen_lbl);
        } else {
            println!("{}", "Invalid index");
        }
        Ok(true)
    }
    pub fn get_single_lbl_match(&self) -> CustRes<String> {
        let pat: &str = &self.iline[1..];
        let mut lbl_distinct: Vec<String> = vec![];
        {
            let mut cached_stmt = self
                .db
                .prepare_cached("select distinct lbl from lbls where instr(lbl,?1)")?;
            let mut rows = cached_stmt.query((pat,))?;
            while let Some(row) = rows.next()? {
                let lbl: String = row.get(0)?;
                lbl_distinct.push(lbl);
            }
        }
        if lbl_distinct.len() != 1 {
            println!("{}{:?}", "Must match exactly 1 label. ", lbl_distinct);
            return Ok("".to_owned());
        }
        Ok(lbl_distinct.into_iter().next().unwrap())
    }
    pub fn plus(&mut self) -> CustRes<()> {
        let lbl_distinct = self.get_single_lbl_match()?;
        if lbl_distinct.is_empty() {
            return Ok(());
        }
        self.lbl_add(&lbl_distinct)
    }
    pub fn minus(&mut self) -> CustRes<()> {
        let lbl_distinct = self.get_single_lbl_match()?;
        if lbl_distinct.is_empty() {
            return Ok(());
        }
        self.lbl_remove(&lbl_distinct)
    }
    pub fn lbl_add(&mut self, lbl_str: &str) -> CustRes<()> {
        let tids = self.mk_vec_of_selection();
        if tids.is_empty() {
            return Ok(());
        }
        {
            let mut cached_stmt = self
            .db
            .prepare_cached("insert into lbls select ?1,?2 where not exists(select 1 from lbls where tmpid=?1 and lbl=?2)")?;
            for tid in &tids {
                let l_tid: i64 = *tid;
                cached_stmt.execute((l_tid, lbl_str))?;
            }
        }
        tidx_touch(self, &tids)?;
        println!(
            "{}{}{}{}",
            lbl_str,
            ": insertion of label executed on ",
            tids.len(),
            " files."
        );
        Ok(())
    }
    pub fn lbl_remove(&mut self, lbl_str: &str) -> CustRes<()> {
        let tids = self.mk_vec_of_selection();
        if tids.is_empty() {
            return Ok(());
        }
        {
            let mut cached_stmt = self
                .db
                .prepare_cached("delete from lbls where tmpid=?1 and lbl=?2")?;
            for tid in &tids {
                let l_tid: i64 = *tid;
                cached_stmt.execute((l_tid, lbl_str))?;
            }
        }
        tidx_touch(self, &tids)?;
        println!(
            "{}{}{}{}",
            lbl_str,
            ": deletion of label executed on ",
            tids.len(),
            " files."
        );
        Ok(())
    }
    pub fn mk_vec_of_selection(&self) -> Vec<i64> {
        let mut tids: Vec<i64> = vec![];
        for selid in &self.def.filter_buf {
            if *selid < 0 {
                tids.push(-selid);
            }
        }
        if tids.is_empty() {
            println!("{}", "No selection");
        }
        tids
    }
    pub fn is_entfo_mounted(&self, rid: &str) -> bool {
        //fixme you need to check all mounted devices, not just default
        let entval = self.def.custentfos_settings.get(rid);
        let some_v = match entval {
            None => {
                return false;
            }
            Some(fsentfo) => fsentfo,
        };
        self.local_settings.default_hardware_id == some_v.hardware_id
    }
    pub fn force_entfo_path(&self, rid: &str) -> &path::Path {
        //fixme you need to check all mounted devices, not just default
        self.def
            .custentfos_settings
            .get(rid)
            .unwrap()
            .efolder
            .pb
            .as_path()
    }
    pub fn fsdel(&mut self) -> CustRes<()> {
        let tids = self.mk_vec_of_selection();
        if tids.is_empty() {
            return Ok(());
        }
        struct RidRel {
            rid: String,
            rel: String,
        }
        let mut ridrels = Vec::<RidRel>::new();
        {
            let mut cached_stmt = self
                .db
                .prepare_cached("select entfo,rel from fsfiles where tmpid=?1")?;
            for tid in &tids {
                let l_tid: i64 = *tid;
                let mut rows = cached_stmt.query((l_tid,))?;
                while let Some(row) = rows.next()? {
                    let ridrel = RidRel {
                        rid: row.get(0)?,
                        rel: row.get(1)?,
                    };
                    if !self.is_entfo_mounted(&ridrel.rid) {
                        println!("{}{}", "Entfo not mounted: ", ridrel.rid);
                        return Ok(());
                    }
                    ridrels.push(ridrel);
                }
            }
        }
        for ridrel in ridrels {
            fs::remove_file(self.force_entfo_path(&ridrel.rid).join(ridrel.rel))?;
        }
        self.exec_with_i64_sli("delete from fsfiles where tmpid=?1", &tids)?;
        tidx_touch(self, &tids)?;
        println!("{}", "DONE");
        Ok(())
    }
    pub fn fsdel_pattern(&mut self, eof: &mut bool) -> CustRes<()> {
        let pat: &str = &self.iline[self.iline_argidx..];
        let tids = self.mk_vec_of_selection();
        if tids.is_empty() {
            return Ok(());
        }
        struct RidRel {
            rid: String,
            rel: String,
            tmpid: i64,
        }
        let mut ridrels = Vec::<RidRel>::new();
        {
            let mut cached_stmt = self
                .db
                .prepare_cached("select entfo,rel from fsfiles where tmpid=?1")?;
            for tid in &tids {
                let l_tid: i64 = *tid;
                let mut rows = cached_stmt.query((l_tid,))?;
                while let Some(row) = rows.next()? {
                    let ridrel = RidRel {
                        rid: row.get(0)?,
                        rel: row.get(1)?,
                        tmpid: l_tid,
                    };
                    if !ridrel.rid.contains(pat) && !ridrel.rel.contains(pat) {
                        continue;
                    }
                    if !self.is_entfo_mounted(&ridrel.rid) {
                        println!("{}{}", "Skipping a entfo that is not mounted: ", ridrel.rid);
                        continue;
                    }
                    println!("{} {} {}", ridrels.len(), ridrel.rid, ridrel.rel);
                    ridrels.push(ridrel);
                }
            }
        }
        if ridrels.is_empty() {
            println!("{}", "No match.");
            return Ok(());
        }
        print!("{}", "Please choose (empty input for deleting all): ");
        io::stdout().flush()?;
        let choice = match self.def.stdin_w.lines.next() {
            None => {
                warn!("{}", "Unexpected stdin EOF");
                *eof = true;
                return Ok(());
            }
            Some(Err(err)) => {
                let l_err: io::Error = err;
                return Err(l_err.into());
            }
            Some(Ok(linestr)) => linestr,
        };
        if choice.is_empty() {
            for ridrel in &ridrels {
                tidx_touch(self, &[ridrel.tmpid])?;
                fs::remove_file(self.force_entfo_path(&ridrel.rid).join(&ridrel.rel))?;
            }
            {
                let mut cached_stmt = self
                    .db
                    .prepare_cached("delete from fsfiles where entfo=?1 and rel=?2")?;
                for ridrel in ridrels {
                    cached_stmt.execute((ridrel.rid, ridrel.rel))?;
                }
            }
            println!("{}", "ALL DELETED");
            return Ok(());
        }
        let idx: usize = match choice.parse::<usize>() {
            Err(_) => {
                println!("{}", "Invalid index");
                return Ok(());
            }
            Ok(l_idx) => l_idx,
        };
        let ridrel = match ridrels.into_iter().nth(idx) {
            None => {
                println!("{}", "Invalid index");
                return Ok(());
            }
            Some(l_ridrel) => l_ridrel,
        };
        fs::remove_file(self.force_entfo_path(&ridrel.rid).join(&ridrel.rel))?;
        {
            let mut cached_stmt = self
                .db
                .prepare_cached("delete from fsfiles where entfo=?1 and rel=?2")?;
            cached_stmt.execute((ridrel.rid, ridrel.rel))?;
        }
        tidx_touch(self, &[ridrel.tmpid])?;
        println!("{}", "ONE FILE DELETED");
        Ok(())
    }
    pub fn del(&mut self) -> CustRes<()> {
        let tids = self.mk_vec_of_selection();
        if tids.is_empty() {
            return Ok(());
        }
        {
            let mut cached_stmt = self
                .db
                .prepare_cached("select 0 from fsfiles where tmpid=?1 limit 1")?;
            for tid in &tids {
                let l_tid: i64 = *tid;
                if !result_rows_empty(cached_stmt.query((l_tid,)))? {
                    println!(
                        "{}",
                        "Cannot \"del\" a file that still has registered copies on devices"
                    );
                    return Ok(());
                }
            }
        }
        tidx_touch(self, &tids)?;
        self.exec_with_i64_sli("delete from files where tmpid=?1", &tids)?;
        self.exec_with_i64_sli("delete from oldh where newi=?1 or oldi=?1", &tids)?;
        self.exec_with_i64_sli("delete from lbls where tmpid=?1", &tids)?;
        self.exec_with_i64_sli("delete from filter_buf where tid=?1", &tids)?;
        self.def.filter_buf.retain(|elem| *elem >= 0);
        println!("{}", "DONE");
        Ok(())
    }
    //pub fn is_filter_buf_empty(&self) -> Result<bool, CustomErr> {
    //    let mut cached_stmt = self.db.prepare_cached("select 0 from filter_buf limit 1")?;
    //    result_rows_empty(cached_stmt.query([]))
    //}
    pub fn clear_filter_buf(&self) -> CustRes<()> {
        let mut cached_stmt = self.db.prepare_cached("delete from filter_buf")?;
        cached_stmt.execute([])?;
        Ok(())
    }
    pub fn show_recs_in_filter_buf(&self /*, tids: &[i64]*/) -> Result<(), CustomErr> {
        let mut cached_stmt = self.db.prepare_cached("with a(label)as(select group_concat(lbl) from lbls where tmpid=?1)select label,size,files.mtime,desc,old_filename,download_url,archive_filename,copyright,entfo,rel from files left join a on 1=1 left join fsfiles on files.tmpid=fsfiles.tmpid where files.tmpid=?1")?;
        let mut num_of_selected = 0;
        for (idx, tidraw) in self.def.filter_buf.iter().enumerate() {
            print!("{}{}", "**** ", idx);
            let tid: i64 = if *tidraw < 0 {
                num_of_selected += 1;
                println!("{}", " +");
                -tidraw
            } else {
                println!();
                *tidraw
            };
            let mut rows = cached_stmt.query((tid,))?;
            let mut printed_once = false;
            while let Some(row) = rows.next()? {
                fn show_field(pre: &str, fstr: String) {
                    if !fstr.is_empty() {
                        println!("{} {}", pre, fstr);
                    }
                }
                if !printed_once {
                    printed_once = true;
                    let lbls: Option<String> = row.get(0)?;
                    let flen: i64 = row.get(1)?;
                    let ms: i64 = row.get(2)?;
                    let desc: String = row.get(3)?;
                    let old_filename: String = row.get(4)?;
                    let download_url: String = row.get(5)?;
                    let archive_filename: String = row.get(6)?;
                    let copyright: String = row.get(7)?;
                    println!("{} {}", flen, millis2display(ms));
                    show_field("DESC", desc);
                    show_field("OLD FILENAME", old_filename);
                    show_field("DOWNLOAD URL", download_url);
                    show_field("ARCHIVE FILENAME", archive_filename);
                    show_field("COPYRIGHT", copyright);
                    show_field("LABEL", lbls.unwrap_or_default());
                }
                let entfo: Option<String> = row.get(8)?;
                let rel: Option<String> = row.get(9)?;
                show_field("ENTFO", entfo.unwrap_or_default());
                show_field("REL", rel.unwrap_or_default());
            }
        }
        println!("{}{}{}", "******* ", num_of_selected, " SELECTED");
        Ok(())
    }
    pub fn slash(&mut self) -> Result<(), CustomErr> {
        let pat: &str = &self.iline[1..];
        if pat.is_empty() {
            println!("{}", "Cannot be empty.");
            return Ok(());
        }
        macro_rules! selec {
            () => {
                "SELECT tmpid from files where "
            };
        }
        macro_rules! glob1 {
            () => ( "tmpid in(select tmpid from lbls where lbl glob ?1) or tmpid in(select tmpid from fsfiles where rel glob ?1) or old_filename glob ?1 or download_url glob ?1 or archive_filename glob ?1" )
        }
        //const : &'static str not working here with concat!. One way is to use macro_rules. Another way might be using const fn to do `+` (abandon concat!).
        let sqlstr = if self.filter_buf.is_empty() {
            concat!(selec!(), glob1!())
        } else {
            concat!(
                selec!(),
                "tmpid in(select tid from filter_buf) and (",
                glob1!(),
                ")"
            )
        };
        let tids = {
            let mut cached_stmt = self.db.prepare_cached(sqlstr)?;
            let glob_op: String = pat.to_owned() + "*";
            query_n_collect_into_vec_i64(cached_stmt.query((glob_op,)))?
        };
        self.iter_rows_to_update_filter_buf(tids)?;
        Ok(())
    }
    pub fn iter_rows_to_update_filter_buf(&mut self, tids: Vec<i64>) -> Result<(), CustomErr> {
        if tids.is_empty() {
            println!("{}", "Nothing found. Filtering is cancelled.");
        } else {
            self.clear_filter_buf()?;
            self.exec_with_i64_sli("insert into filter_buf values(?1)", &tids)?;
            self.def.filter_buf = tids;
            self.show_recs_in_filter_buf()?;
        }
        Ok(())
    }
}
fn write_treefile(pathbuild: &PathBuf, elems: &Vec<InfoJsonElem>) -> Result<(), CustomErr> {
    use std::fs::*;
    fs::create_dir_all(pathbuild.parent().unwrap())?;
    let mut file = File::create(pathbuild)?;
    serde_json::to_writer_pretty(&file, elems)?;
    use std::io::prelude::*;
    file.write_all(b"\n")?;
    Ok(())
}
pub fn chk_path_valid_as_entfo(t_path: &path::Path) -> bool {
    if t_path.is_file() || t_path.is_symlink() {
        warn!(
            "{}{:?}",
            "Specified entfo is regular file or symlink: ",
            t_path.as_os_str()
        );
        false
    } else {
        true
    }
}
