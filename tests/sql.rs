#[cfg(test)]
mod tests {

    #[test]
    fn sql_param() {
        use rusqlite::Connection;
        let conn = Connection::open_in_memory().unwrap();
        let mut cached_stmt = conn.prepare_cached("select ?1,?2,?1,?2").unwrap();
        let mut rows = cached_stmt.query(("foo", "bar")).unwrap();
        if let Some(row) = rows.next().unwrap() {
            let c1: String = row.get(0).unwrap();
            let c2: String = row.get(1).unwrap();
            let c3: String = row.get(2).unwrap();
            let c4: String = row.get(3).unwrap();
            dbg!(c1);
            dbg!(c2);
            dbg!(c3);
            dbg!(c4);
        } else {
            dbg!("No row");
        }
    }
}
