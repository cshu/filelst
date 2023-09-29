//use crate::common::*;
//use crabrs::*;
//use crabdxrs::*;
//use serde::{Deserialize, Serialize};
//use std::path::PathBuf;
use super::*;

pub fn upsert_a_row_for_dx(
    con: &Ctx,
    email: &str,
    path_display: &str,
    md: dropbox_sdk::files::FileMetadata,
) -> CustRes<()> {
    assert!(
        path_display == &md.path_display.as_ref().unwrap()[1..]
            && md.path_display.as_ref().unwrap().starts_with('/')
    );
    let r_empty = {
        let mut cached_stmt = con
            .db
            .prepare_cached("select 0 from dx where email=?1 and path_display=?2")?;
        result_rows_empty(cached_stmt.query((email, path_display)))?
    };
    if r_empty {
        let mut cached_stmt = con
            .db
            .prepare_cached("insert into dx values(?1,?2,?3,?4,?5,?6)")?;
        dx_exec_insertion(email, md, &mut cached_stmt)?;
    } else {
        let mut cached_stmt = con
        .db
        .prepare_cached("update dx set id=?1,client_modified=?2,server_modified=?3,size=?4 where email=?5 and path_display=?6")?;
        let id: String = md.id;
        let client_modified: String = md.client_modified;
        let server_modified: String = md.server_modified;
        let size: i64 = md.size as i64;
        cached_stmt.execute((
            id,
            client_modified,
            server_modified,
            size,
            email,
            path_display,
        ))?;
    }
    Ok(())
}
pub fn dx_exec_insertion(
    email: &str,
    md: dropbox_sdk::files::FileMetadata,
    cached_stmt: &mut rusqlite::CachedStatement,
) -> CustRes<()> {
    //let dropbox_sdk::files::FileMetadata{id, client_modified, server_modified, size, path_display, ..} = md;
    let path_display: Option<String> = md.path_display;
    let mut path_display: String = match path_display {
        None => {
            return Ok(());
        }
        Some(inner) => inner,
    };
    if !path_display.starts_with('/') {
        warn!(
            "{}{}",
            "IGNORING PATH NOT STARTING WITH SLASH: ", path_display
        );
        return Ok(()); //should be impossible to reach here if dx acts consistently?
    }
    path_display.remove(0);
    let id: String = md.id;
    let client_modified: String = md.client_modified;
    let server_modified: String = md.server_modified;
    let size: i64 = md.size as i64;
    cached_stmt.execute((
        email,
        id,
        client_modified,
        server_modified,
        size,
        path_display,
    ))?;
    Ok(())
}
pub fn del_a_row_from_dx(con: &Ctx, email: &str, path_display: &str) -> CustRes<()> {
    let mut cached_stmt = con
        .db
        .prepare_cached("delete from dx where email=?1 and path_display=?2")?;
    cached_stmt.execute((email, path_display))?;
    Ok(())
}
pub fn list_dx(con: &mut Ctx) -> CustRes<()> {
    //note this scans superficially (does not calc hash of file)
    let entries = crabdxrs::list_folder_regular(&con.dx_cli)?;
    coutln!(entries.len(), " files found.");
    //todo save the result to info file(s), so that it can be viewed offline
    con.db.execute("delete from dx", ())?;
    {
        let mut cached_stmt = con
            .db
            .prepare_cached("insert into dx values(?1,?2,?3,?4,?5,?6)")?;
        for ent in entries {
            let md = match ent {
                dropbox_sdk::files::Metadata::File(md) => md,
                _ => {
                    continue;
                }
            };
            dx_exec_insertion(&con.def.dx_email, md, &mut cached_stmt)?;
        }
    }
    con.def
        .listed_at_least_once
        .insert(con.def.dx_email.clone() + "@dx");
    Ok(())
}

pub fn chk_listed_at_least_once(con: &Ctx, pair: &(dirdiff::InfoDir, dirdiff::InfoDir)) -> bool {
    macro_rules! match_them {
        ($arg: expr) => {
            match &$arg {
                InfoDir::Entfo(_) => true,
                InfoDir::M(inner) => {
                    let kstr: String = inner.0.to_owned() + "@m";
                    con.listed_at_least_once.contains(&kstr)
                }
                InfoDir::D(inner) => {
                    let kstr: String = inner.0.to_owned() + "@dx";
                    con.listed_at_least_once.contains(&kstr)
                }
            }
        };
    }
    let ok0 = match_them!(pair.0);
    let ok1 = match_them!(pair.1);
    ok0 && ok1
}

//#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
//pub struct InfoJsonRemote {
//    #[serde(skip)]
//    tmpid: i64,
//    #[serde(skip_serializing_if = "Vec::is_empty")]
//    #[serde(default)]
//    pf: Vec<Vec<serde_json::Value>>, //note elem is `Entfo folder ID`+`rel path relative to the Entfo`+`actual modtime on cloud`
//    #[serde(skip_serializing_if = "Vec::is_empty")]
//    #[serde(default)]
//    lbls: Vec<String>,
//    #[serde(skip_serializing_if = "String::is_empty")]
//    #[serde(default)]
//    desc: String,
//    #[serde(default)]
//    size: i64,
//    #[serde(default)]
//    mtime: i64, //note this is the earliest time you are sure since when it has never been modifed, not physical mtime
//    #[serde(skip_serializing_if = "String::is_empty")]
//    #[serde(default)]
//    hash: String,
//    #[serde(skip_serializing_if = "String::is_empty")]
//    #[serde(default)]
//    old_filename: String,
//    #[serde(skip_serializing_if = "String::is_empty")]
//    #[serde(default)]
//    download_url: String,
//    #[serde(skip_serializing_if = "String::is_empty")]
//    #[serde(default)]
//    archive_filename: String,
//    #[serde(skip_serializing_if = "String::is_empty")]
//    #[serde(default)]
//    copyright: String,
//}
//
//#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
//pub struct FolderPair {
//    #[serde(skip)]
//    pub lpb: PathBuf,
//    #[serde(skip)]
//    pub rpb: PathBuf,
//    pub local: String,
//    pub remote: String,
//}
