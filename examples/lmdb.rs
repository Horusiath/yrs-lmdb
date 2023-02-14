use lib0::decoding::{Cursor, Read};
use lmdb_rs::core::DbCreate;
use lmdb_rs::Environment;
use std::sync::Arc;
use std::time::Instant;
use yrs::{Doc, Text, Transact};
use yrs_lmdb::DocStore;

fn main() {
    let env = Environment::new()
        .autocreate_dir(true)
        .max_dbs(4)
        .open("example-lmdb", 0o777)
        .unwrap();
    let env = Arc::new(env);
    let handle = Arc::new(env.create_db("test", DbCreate).unwrap());
    let doc_name = "sample-doc";

    let doc = Doc::new();
    let text = doc.get_or_insert_text("text");

    // store subsequent updates automatically
    let stat = env.stat().unwrap();
    let _sub = {
        let env = env.clone();
        let handle = handle.clone();
        doc.observe_update_v1(move |_, e| {
            let txn = env.new_transaction().unwrap();
            let db = txn.bind(&handle);
            let i = db.push_update(doc_name, &e.update).unwrap();
            if i % 128 == 0 {
                // compact updates into document
                db.flush_doc(doc_name).unwrap();
            }
            txn.commit().unwrap();
        })
        .unwrap()
    };

    {
        // load document using readonly transaction
        let mut txn = doc.transact_mut();
        let db_txn = env.get_reader().unwrap();
        let db = db_txn.bind(&handle);
        db.load_doc(&doc_name, &mut txn).unwrap();
    }

    // execute editing trace
    let ops = read_input("./examples/editing-trace.bin");
    let now = Instant::now();
    let ops_count = ops.len();
    for op in ops.iter() {
        let mut txn = doc.transact_mut();
        match op {
            TextOp::Insert(idx, txt) => text.insert(&mut txn, *idx, txt),
            TextOp::Delete(idx, len) => text.remove_range(&mut txn, *idx, *len),
        }
    }
    let elapsed = Instant::now().duration_since(now);
    println!(
        "executed {} operations in {}ms",
        ops_count,
        elapsed.as_millis()
    );
}

enum TextOp {
    Insert(u32, String),
    Delete(u32, u32),
}

fn read_input(fpath: &str) -> Vec<TextOp> {
    use std::fs::File;
    use yrs::updates::decoder::DecoderV1;

    let mut f = File::open(fpath).unwrap();
    let mut buf = Vec::new();
    std::io::Read::read_to_end(&mut f, &mut buf).unwrap();
    let mut decoder = DecoderV1::new(Cursor::new(buf.as_slice()));
    let len: usize = decoder.read_var().unwrap();
    let mut result = Vec::with_capacity(len);
    for _ in 0..len {
        let op = {
            match decoder.read_var().unwrap() {
                1u32 => {
                    let idx = decoder.read_var().unwrap();
                    let chunk = decoder.read_string().unwrap();
                    TextOp::Insert(idx, chunk.to_string())
                }
                2u32 => {
                    let idx = decoder.read_var().unwrap();
                    let len = decoder.read_var().unwrap();
                    TextOp::Delete(idx, len)
                }
                other => panic!("unrecognized TextOp tag type: {}", other),
            }
        };
        result.push(op);
    }
    result
}
