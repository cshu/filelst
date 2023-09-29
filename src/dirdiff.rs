//use crabrs::*;
//use log::*;
use serde::{Deserialize, Serialize};
//use std::io::Write;
use std::path::PathBuf;
use std::*;

use super::*;
use remote::*;

pub fn dir_diff(con: &mut Ctx) -> CustRes<bool> {
    if !con.def.info_pair_file.try_exists()? {
        println!(
            "{}{}",
            "No pair defined yet. Pairs can be manually written into: ",
            con.def.info_pair_file.display()
        );
        coutln!(
            r#"An example of the content is: [[{"Entfo":["opt_filelst","a/relative/path/"]}, {"D":["your@account.com","a/relative/path/"]}]] (Note that path cannot start with slash. A leading slash will cause no rows to match. Because your sqlite db stores relative paths without leading slash. And the path prefix must be EMPTY or end with SLASH or BACKSLASH)"#
        );
        return Ok(true);
    }
    //fs::read_to_string(&con.def.info_pair_file)?;
    let pairs = read_info_pair_file(&con.def.info_pair_file)?;
    if pairs.is_empty() {
        return Ok(true);
    }
    for (idx, pair) in pairs.iter().enumerate() {
        println!(
            "{}{} {:?}",
            if Some(pair) == con.def.info_pair.as_ref() {
                "+"
            } else {
                " "
            },
            idx,
            pair
        );
    }
    let choice = cout_n_flush_input_none_ret_false!(
        "Choose one (or enter empty input if no change to choice): ",
        con.stdin_w.lines
    );
    if choice.is_empty() {
        match &con.def.info_pair {
            None => {
                coutln!("No choice made yet.");
                return Ok(true);
            }
            Some(_) => {}
        }
    } else {
        let idx = match choice.parse::<usize>() {
            Err(_) => {
                coutln!("Invalid index");
                return Ok(true);
            }
            Ok(inner) => inner,
        };
        let chosen_pair = match pairs.into_iter().nth(idx) {
            None => {
                coutln!("Invalid index");
                return Ok(true);
            }
            Some(inner) => inner,
        };
        if !chk_listed_at_least_once(con, &chosen_pair) {
            coutln!("Remote files must be at least be listed (fetch filenames) for once.");
            return Ok(true);
        }
        con.def.info_pair = Some(chosen_pair);
    }
    //fixme your comparison logic is only comparing records in sqlite, when sqlite does not contain a local file path it does not guarantee there is no such file in real physical filesystem. (It might be ignored during scan, due to your customized rules.) This could cause silent overwriting of files?
    //fixme another flaw of your comparison logic is that you forgot the existence of folder occupying the filename exists in opposite storage. So when you sync from that storage to this storage, it can fail due to filename already occupied by a folder
    let mut mpair = mem::take(&mut con.def.info_pair).unwrap();
    let flist = get_recs_for_diff(con, &mut mpair)?;
    con.def.info_pair = Some(mpair);
    if flist.is_empty() {
        coutln!("Nothing can be done, because there are 0 records found for the pair.");
        return Ok(true);
    }
    let mut f_iter = flist.into_iter().peekable();
    let mut difflst = Vec::<DRow>::new();
    macro_rules! pushdiff {
        ($arg: expr) => {
            if 0 == $arg.col {
                difflst.push(mk_drow($arg, ComRec::default()));
            } else {
                difflst.push(mk_drow(ComRec::default(), $arg));
            }
        };
        ($arg0: expr, $arg1: expr) => {
            difflst.push(mk_drow($arg0, $arg1));
        };
    }
    loop {
        let single = match f_iter.next() {
            None => break,
            Some(inner) => inner,
        };
        let next_f = match f_iter.peek() {
            Some(inner) => inner,
            None => {
                pushdiff!(single);
                break;
            }
        };
        if single.frel == next_f.frel {
            pushdiff!(single, f_iter.next().unwrap());
        } else {
            pushdiff!(single);
        }
    }
    loop {
        print_diff(&difflst);
        let instr = cout_n_flush_input_none_ret_false!(
            r##"Input an index to toggle selection (`all` for all, `?-?` for range), or input use0/use1 to sync selected rows with data from 0/1 (To leave, enter empty input): "##,
            con.stdin_w.lines
        );
        fn setlst(lst: &mut [DRow]) {
            let mut all_selected = true;
            for drow in lst.iter_mut() {
                if !drow.selected {
                    all_selected = false;
                    drow.selected = true;
                }
            }
            if all_selected {
                for drow in lst.iter_mut() {
                    drow.selected = false;
                }
            }
        }
        match instr.as_str() {
            "" => break,
            "all" => {
                setlst(&mut difflst);
            }
            "use0" => {
                if drow_sync_use0(con, &mut difflst)? {
                    remove_empty_rows(&mut difflst);
                }
            }
            "use1" => {
                if drow_sync_use1(con, &mut difflst)? {
                    remove_empty_rows(&mut difflst);
                }
            }
            _ => {
                if let Some(hidx) = instr.find('-') {
                    let bidx = match instr[0..hidx].parse::<usize>() {
                        Err(_) => {
                            println!("{}", "Invalid index");
                            continue;
                        }
                        Ok(l_idx) => l_idx,
                    };
                    let eidx = match instr[hidx + 1..].parse::<usize>() {
                        Err(_) => {
                            println!("{}", "Invalid index");
                            continue;
                        }
                        Ok(l_idx) => l_idx,
                    };
                    if bidx > eidx || eidx >= difflst.len() {
                        println!("{}", "Invalid index");
                        continue;
                    }
                    setlst(&mut difflst[bidx..=eidx]);
                    continue;
                }
                let selidx = match instr.parse::<usize>() {
                    Err(_) => {
                        println!("{}", "Invalid index");
                        continue;
                    }
                    Ok(l_idx) => l_idx,
                };
                setlst(match difflst.get_mut(selidx..=selidx) {
                    None => {
                        println!("{}", "Invalid index");
                        continue;
                    }
                    Some(sli) => sli,
                });
            }
        }
    }
    Ok(true)
}
//pub fn difflst_sync_use0(difflst: &mut [DRow]) {
//    for drow in difflst {
//        if drow.selected {
//            comrec_sync_use0(drow);
//        }
//    }
//}
//pub fn comrec_sync_use0(drow: &mut DRow) {
//    drow.column1.fsize = drow.column0.fsize;
//}
pub fn remove_empty_rows(difflst: &mut Vec<DRow>) {
    difflst.retain(|drow| !drow.column0.is_empty() || !drow.column1.is_empty());
}
pub fn drow_sync_use1(con: &mut Ctx, difflst: &mut [DRow]) -> CustRes<bool> {
    fn src_col(drow: &DRow) -> &ComRec {
        &drow.column1
    }
    fn dst_col(drow: &DRow) -> &ComRec {
        &drow.column0
    }
    fn src_dst(drow: &mut DRow) -> (&ComRec, &mut ComRec) {
        (&drow.column1, &mut drow.column0)
    }
    let mpair: &(InfoDir, InfoDir) = &con.def.info_pair.clone().unwrap();
    let src = &mpair.1;
    let dst = &mpair.0;
    println!("{}{:?}{}{:?}", "FROM ", src, " TO ", dst);
    match mpair {
        (InfoDir::Entfo(idst), InfoDir::D(isrc)) => {
            sync_dnload_fr_dx(con, isrc, idst, difflst, dst_col, src_dst)
        }
        (InfoDir::D(idst), InfoDir::Entfo(isrc)) => {
            sync_upload_to_dx(con, isrc, idst, difflst, src_col, src_dst)
        }
        _ => {
            coutln!("Unsupported operation.");
            Ok(false)
        }
    }
}
pub fn drow_sync_use0(con: &mut Ctx, difflst: &mut [DRow]) -> CustRes<bool> {
    fn src_col(drow: &DRow) -> &ComRec {
        &drow.column0
    }
    fn dst_col(drow: &DRow) -> &ComRec {
        &drow.column1
    }
    fn src_dst(drow: &mut DRow) -> (&ComRec, &mut ComRec) {
        (&drow.column0, &mut drow.column1)
    }
    //let mpair = con.def.info_pair.as_ref().unwrap();
    let mpair: &(InfoDir, InfoDir) = &con.def.info_pair.clone().unwrap();
    let src = &mpair.0;
    let dst = &mpair.1;
    println!("{}{:?}{}{:?}", "FROM ", src, " TO ", dst);
    //if !matches!(src, InfoDir::D(_)) || !matches!(dst, InfoDir::Entfo(_)) {
    //    coutln!("Unsupported operation.");
    //    return Ok(false);
    //}
    match mpair {
        (InfoDir::D(isrc), InfoDir::Entfo(idst)) => {
            sync_dnload_fr_dx(con, isrc, idst, difflst, dst_col, src_dst)
        }
        (InfoDir::Entfo(isrc), InfoDir::D(idst)) => {
            sync_upload_to_dx(con, isrc, idst, difflst, src_col, src_dst)
        }
        _ => {
            coutln!("Unsupported operation.");
            Ok(false)
        }
    }
}
pub fn sync_dnload_fr_dx(
    con: &mut Ctx,
    isrc: &(String, String),
    idst: &(String, String),
    difflst: &mut [DRow],
    dst_col: fn(&DRow) -> &ComRec,
    src_dst: fn(&mut DRow) -> (&ComRec, &mut ComRec),
) -> CustRes<bool> {
    let entfostr = &idst.0;
    let prefix = &idst.1;
    //fixme mentfos_settings should also be checked?
    let fsentfo = match con.def.custentfos_settings.get(entfostr) {
        None => {
            coutln!("Entfo not found.");
            return Ok(false);
        }
        Some(inner) => inner,
    };
    if !con.is_entfo_mounted(entfostr) {
        coutln!("Entfo not mounted.");
        return Ok(false);
    }
    let prefix = fsentfo.efolder.pb.join(prefix);
    coutln!("START EXTRA CHECKING FOR ", prefix.display());
    for drow in &*difflst {
        macro_rules! mk_f_path {
            () => {
                prefix.join(drow.fs_rel().as_ref())
            };
        }
        let f_path = mk_f_path!();
        let cdst = dst_col(drow);
        if cdst.is_empty() {
            if !possible_to_create_new_file(&f_path)? {
                coutln!(
                    "Cannot proceed due to a path (or its ancestor) unexpectedly occupied: ",
                    f_path.display()
                );
                return Ok(false);
            }
        } else {
            if !real_reg_file_without_symlink(&f_path) {
                coutln!(
                    "Cannot proceed due to a path is not regular file: ",
                    f_path.display()
                );
                return Ok(false);
            }
        }
    }
    coutln!("START MODIFYING ", prefix.display());
    for drow in difflst {
        if !drow.selected {
            continue;
        }
        macro_rules! mk_f_path {
            () => {
                prefix.join(drow.fs_rel().as_ref())
            };
        }
        let f_path = mk_f_path!();
        let (csrc, cdst) = src_dst(drow);
        if csrc.is_empty() {
            coutln!("DELETING ", f_path.display());
            fs::remove_file(f_path)?;
            cdst.clear_f();
        } else if csrc.fsize != cdst.fsize || cdst.is_empty() {
            coutln!("OVERWRITING ", f_path.display());
            fs::create_dir_all(
                f_path
                    .parent()
                    .ok_or("Unexpected parent NONE. Should never happen")?,
            )?;
            let mut dstf = fs::File::create(f_path)?;
            crabdxrs::download_ignore_json_header(
                &con.def.dx_cli,
                &("/".to_owned() + &isrc.1 + &csrc.frel),
                &mut dstf,
            )?;
            cdst.becomes_eq(csrc);
        } else {
            coutln!("SKIPPING ", f_path.display());
        }
    }
    coutln!("Now re-scanning the local folder.");
    //*****
    let l_custentfos_settings = mem::take(&mut con.def.custentfos_settings);
    let res = con.scan_single_entfo(&l_custentfos_settings, entfostr);
    con.def.custentfos_settings = l_custentfos_settings;
    res?;
    //*****
    Ok(true)
}
pub fn sync_upload_to_dx(
    con: &Ctx,
    isrc: &(String, String),
    idst: &(String, String),
    difflst: &mut [DRow],
    src_col: fn(&DRow) -> &ComRec,
    src_dst: fn(&mut DRow) -> (&ComRec, &mut ComRec),
) -> CustRes<bool> {
    let entfostr = &isrc.0;
    let prefix = &isrc.1;
    //fixme mentfos_settings should also be checked?
    let fsentfo = match con.def.custentfos_settings.get(entfostr) {
        None => {
            coutln!("Entfo not found.");
            return Ok(false);
        }
        Some(inner) => inner,
    };
    if !con.is_entfo_mounted(entfostr) {
        coutln!("Entfo not mounted.");
        return Ok(false);
    }
    let prefix = fsentfo.efolder.pb.join(prefix);
    coutln!("START EXTRA CHECKING FOR ", idst.1);
    for drow in &*difflst {
        macro_rules! mk_f_path {
            () => {
                prefix.join(drow.fs_rel().as_ref())
            };
        }
        let f_path = mk_f_path!();
        let csrc = src_col(drow);
        if csrc.is_empty() {
            if exists_without_following_sym(&f_path)? {
                coutln!(
                    "Cannot proceed due to a path unexpectedly occupied: ",
                    f_path.display()
                );
                return Ok(false);
            }
        } else {
            if !real_reg_file_without_symlink(&f_path) {
                coutln!(
                    "Cannot proceed due to a path is not regular file: ",
                    f_path.display()
                );
                return Ok(false);
            }
            if f_path.symlink_metadata()?.len() > crabdxrs::MAX_FILE_SIZE {
                coutln!("Cannot proceed due to huge file: ", f_path.display());
                return Ok(false);
            }
        }
    }
    coutln!("START MODIFYING ", idst.1);
    for drow in difflst {
        if !drow.selected {
            continue;
        }
        let dst_path = "/".to_owned() + &idst.1 + drow.frel();
        let (csrc, cdst) = src_dst(drow);
        if csrc.is_empty() {
            coutln!("DELETING ", dst_path);
            crabdxrs::delete(&con.def.dx_cli, &dst_path)?;
            del_a_row_from_dx(con, &idst.0, &dst_path[1..])?;
            cdst.clear_f();
        } else if csrc.fsize != cdst.fsize || cdst.is_empty() {
            coutln!("OVERWRITING ", dst_path);
            //no need to create parent dir before upload, dropbox will do it for you if needed
            let new_md = crabdxrs::upload_regular(
                &con.def.dx_cli,
                &prefix.join(csrc.fs_rel().as_ref()),
                &dst_path,
            )?;
            upsert_a_row_for_dx(con, &idst.0, &dst_path[1..], new_md)?;
            cdst.becomes_eq(csrc);
        } else {
            coutln!("SKIPPING ", dst_path);
        }
    }
    Ok(true)
}
pub fn mk_drow(column0: ComRec, column1: ComRec) -> DRow {
    DRow {
        selected: false,
        column0,
        column1,
    }
}
pub struct DRow {
    selected: bool,
    column0: ComRec,
    column1: ComRec,
}
impl DRow {
    pub fn sel_char(&self) -> char {
        if self.selected {
            '+'
        } else {
            ' '
        }
    }
    pub fn fs_rel(&self) -> borrow::Cow<str> {
        if self.column0.is_empty() {
            debug_assert!(!self.column1.is_empty());
            return self.column1.fs_rel();
        } else {
            return self.column0.fs_rel();
        }
    }
    pub fn frel(&self) -> &str {
        if self.column0.is_empty() {
            debug_assert!(!self.column1.is_empty());
            return &self.column1.frel;
        } else {
            return &self.column0.frel;
        }
    }
}

pub fn print_diff(difflst: &[DRow]) {
    for (idx, drow) in difflst.iter().enumerate() {
        if drow.column0.is_empty() {
            print!("{} {} {}", drow.sel_char(), idx, ">");
            print_single_row(&drow.column1);
            continue;
        }
        if drow.column1.is_empty() {
            print!("{} {} {}", drow.sel_char(), idx, "<");
            print_single_row(&drow.column0);
            continue;
        }
        print!("{} {} ", drow.sel_char(), idx);
        print_row_cmp(&drow.column0, &drow.column1);
    }
}

pub fn print_row_cmp(cr0: &ComRec, cr1: &ComRec) {
    if cr0.fsize == cr1.fsize {
        print!("{}", "=");
        coutln!(cr0.frel, " [SIZE] ", cr0.fsize);
    } else {
        print!("{}", "!");
        coutln!(cr0.frel, " [SIZE] ", cr0.fsize, " != ");
    }
}
pub fn print_single_row(comrec: &ComRec) {
    //if 0 == comrec.col {
    //    print!("{}", "<");
    //} else {
    //    print!("{}", ">");
    //}
    coutln!(comrec.frel, " [SIZE] ", comrec.fsize);
}

pub fn get_recs_for_diff(con: &mut Ctx, mpair: &mut (InfoDir, InfoDir)) -> CustRes<Vec<ComRec>> {
    let mut flist = Vec::<ComRec>::new();
    let mut mdir: &mut InfoDir;
    macro_rules! match_them {
        ($arg: expr) => {
            match mdir {
                InfoDir::Entfo((entfoid, rel)) => {
                    get_recs_for_diff_entfo(con, entfoid, rel, $arg, &mut flist)?;
                }
                InfoDir::M(_) => return dummy_err("M not supported yet."),
                InfoDir::D((email, rel)) => {
                    get_recs_for_diff_dx(con, email, rel, $arg, &mut flist)?;
                }
            }
        };
    }
    mdir = &mut mpair.0;
    match_them!(0);
    mdir = &mut mpair.1;
    match_them!(1);
    flist.sort_unstable();
    Ok(flist)
}
pub fn get_recs_for_diff_dx(
    con: &Ctx,
    email: &str,
    rel: &str,
    col: u8,
    flist: &mut Vec<ComRec>,
) -> CustRes<()> {
    let mut cached_stmt = con.db.prepare_cached(
        "select path_display,size from dx where email=?1 and 1=instr(path_display,?2)",
    )?;
    let mut rows = cached_stmt.query((email, rel))?;
    while let Some(row) = rows.next()? {
        let mut frel: String = row.get(0)?;
        let fsize: i64 = row.get(1)?;
        frel.replace_range(..rel.len(), "");
        flist.push(ComRec { col, frel, fsize });
    }
    Ok(())
}
pub fn get_recs_for_diff_entfo(
    con: &Ctx,
    entfoid: &str,
    rel: &str,
    col: u8,
    flist: &mut Vec<ComRec>,
) -> CustRes<()> {
    let mut cached_stmt = con
            .db
            .prepare_cached("select rel,size from fsfiles join files on fsfiles.tmpid=files.tmpid where entfo=?1 and 1=instr(rel,?2)")?;
    let mut rows = cached_stmt.query((entfoid, rel))?;
    while let Some(row) = rows.next()? {
        let mut frel: String = row.get(0)?;
        let fsize: i64 = row.get(1)?;
        frel.replace_range(..rel.len(), "");
        //if cfg!(target_os = "windows") {
        //you have to use slash for dirdiff because such that you can strcmp paths from Windows+Linux
        frel = frel.replace('\\', "/");
        //}
        flist.push(ComRec { col, frel, fsize });
    }
    Ok(())
}

#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct ComRec {
    frel: String,
    col: u8,
    fsize: i64,
}
impl ComRec {
    #[cfg(target_os = "windows")]
    pub fn fs_rel(&self) -> borrow::Cow<str> {
        borrow::Cow::Owned(self.frel.replace('/', "\\"))
    }

    #[cfg(not(target_os = "windows"))]
    pub fn fs_rel(&self) -> borrow::Cow<str> {
        borrow::Cow::Borrowed(&self.frel)
    }

    pub fn is_empty(&self) -> bool {
        self.frel.is_empty()
    }
    pub fn clear_f(&mut self) {
        self.frel.clear();
        self.fsize = 0;
    }
    pub fn becomes_eq(&mut self, other: &Self) {
        if self.frel != other.frel {
            self.frel.clone_from(&other.frel);
        }
        self.fsize = other.fsize;
    }
}

//fn dir_diff_inner(con: &mut Ctx, pair: &(InfoDir, InfoDir)) -> CustRes<()>{
//	Ok(())
//}

//type InfoDir = Vec<String>;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum InfoDir {
    //note with enum, the derived serialization result is something like {"D":["11","22"]}
    Entfo((String, String)),
    M((String, String)),
    D((String, String)),
}

pub fn read_info_pair_file(pathbuild: &PathBuf) -> CustRes<Vec<(InfoDir, InfoDir)>> {
    use std::fs::*;
    let jsonfile = File::open(pathbuild)?;
    let reader = io::BufReader::new(jsonfile);
    let mut retval: Vec<(InfoDir, InfoDir)> = serde_json::from_reader(reader)?;
    if retval.is_empty() {
        coutln!("No pairs found.");
        return Ok(retval);
    }
    for pair in &mut retval {
        fn chk_validity(inner: &(String, String)) -> bool {
            if inner.1.is_empty() || inner.1.ends_with(&['\\', '/']) {
                return true;
            }
            coutln!("Invalid path prefix found. Abort.");
            return false;
        }
        macro_rules! chk_valid {
            ($arg: expr) => {
                if !chk_validity($arg) {
                    return Ok(Vec::<(InfoDir, InfoDir)>::new());
                }
            };
        }
        macro_rules! chk_prefix {
            ($arg: expr) => {
                match &mut $arg {
                    InfoDir::Entfo(inner) => {
                        chk_valid!(inner);
                    }
                    InfoDir::M(inner) => {
                        chk_valid!(inner);
                    }
                    InfoDir::D(inner) => {
                        chk_valid!(inner);
                    }
                }
            };
        }
        chk_prefix!(pair.0);
        chk_prefix!(pair.1);
    }
    Ok(retval)
}
