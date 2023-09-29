//use crate::common::*;
use crabrs::*;
use crabsqliters::*;
//use rusqlite::*;

impl crate::Ctx {
    pub fn exec_with_i64_sli(&self, sqlstr: &str, paramsli: &[i64]) -> CustRes<()> {
        exec_with_slice_i64(&self.db, sqlstr, paramsli)
    }
}
