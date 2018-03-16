extern crate ruba;

extern crate rustyline;
extern crate heapsize;
extern crate time;
extern crate nom;

mod print_results;
mod fmt_table;

use std::collections::HashMap;
use std::env;
use std::panic;

use heapsize::HeapSizeOf;
use ruba::mem_store::batch::Batch;
use ruba::parser::parser;
use time::precise_time_s;

const LOAD_CHUNK_SIZE: usize = 200_000;

fn main() {
    let args: Vec<String> = env::args().collect();
    let filename = &args.get(1).expect("Specify data file as argument.");
    let columnarization_start_time = precise_time_s();
    let batches = ruba::ingest::csv_loader::ingest_file(filename, LOAD_CHUNK_SIZE);
    print_ingestion_stats(&batches, columnarization_start_time);

    repl(&batches);
}

fn print_ingestion_stats(batches: &Vec<Batch>, starttime: f64) {
    let bytes_in_ram: usize = batches.iter().map(|batch| batch.cols.heap_size_of_children()).sum();
    println!("Loaded data into {:.2} MiB in RAM in {} chunk(s) in {:.1} seconds.",
             bytes_in_ram as f64 / 1024f64 / 1024f64,
             batches.len(),
             precise_time_s() - starttime);

    println!("\n# Breakdown by column #");
    let mut column_sizes = HashMap::new();
    for batch in batches {
        for col in &batch.cols {
            let heapsize = col.heap_size_of_children();
            if let Some(size) = column_sizes.get_mut(col.name()) {
                *size += heapsize;
            }
            if !column_sizes.contains_key(col.name()) {
                column_sizes.insert(col.name().to_string(), heapsize);
            }
        }
    }
    for (columname, heapsize) in column_sizes {
        println!("{}: {:.2}MiB", columname, heapsize as f64 / 1024. / 1024.);
    }
}

fn repl(datasource: &Vec<Batch>) {
    let mut rl = rustyline::Editor::<()>::new();
    rl.load_history(".ruba_history").ok();
    while let Ok(mut s) = rl.readline("ruba> ") {
        if let Some('\n') = s.chars().next_back() {
            s.pop();
        }
        if let Some('\r') = s.chars().next_back() {
            s.pop();
        }
        if s == "exit" {
            break;
        }
        if s.chars().next_back() != Some(';') {
            s.push(';');
        }
        rl.add_history_entry(&s);
        match parser::parse_query(s.as_bytes()) {
            nom::IResult::Done(_remaining, query) => {
                panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let mut compiled_query = query.compile(datasource);
                    let result = compiled_query.run();
                    print_results::print_query_result(&result);
                })).expect("fatal error");
            }
            err => {
                println!("Failed to parse query! {:?}", err);
                println!("Example for supported query:");
                println!("select url, count(1), app_name, sum(events) where and( >(timestamp, \
                          1000), =(version, \"1.5.3\") )\n");
            }
        }
    }
    rl.save_history(".ruba_history").ok();
}
