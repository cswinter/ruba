use std::collections::{HashMap, VecDeque};
use std::ops::DerefMut;
use std::str;
use std::sync::{Arc, Mutex, RwLock, Condvar};
use std::thread;

use disk_store::db::*;
use engine::query::QueryResult;
use ingest::input_column::InputColumn;
use ingest::raw_val::RawVal;
use mem_store::batch::Batch;
use mem_store::table::*;
use nom;
use num_cpus;
use parser::parser;
use scheduler::*;
use time;


pub struct InnerRuba {
    tables: RwLock<HashMap<String, Table>>,
    idle_queue: (Mutex<bool>, Condvar),
    task_queue: RwLock<VecDeque<Arc<Task>>>,
    storage: Box<DB>,
}

impl InnerRuba {
    pub fn new(storage: Box<DB>, load_tabledata: bool) -> InnerRuba {
        let existing_tables =
            if load_tabledata {
                Table::restore_from_db(20_000, storage.as_ref())
            } else {
                Table::load_table_metadata(20_000, storage.as_ref())
            };

        let ruba = InnerRuba {
            tables: RwLock::new(existing_tables),
            idle_queue: (Mutex::new(false), Condvar::new()),
            task_queue: RwLock::new(VecDeque::new()),
            storage: storage,
        };

        return ruba;
    }

    pub fn start_worker_threads(ruba: Arc<InnerRuba>) {
        for _ in 0..num_cpus::get() {
            let cloned = ruba.clone();
            thread::spawn(move || InnerRuba::worker_loop(cloned));
        }
    }

    // TODO(clemens): make a synchronous and move to Ruba
    pub fn run_query(&self, query: &str) -> Result<QueryResult, String> {
        match parser::parse_query(query.as_bytes()) {
            nom::IResult::Done(_remaining, query) => {
                let tables = self.tables.read().unwrap();
                // TODO(clemens): extend query language with from clause
                match tables.get(&query.table) {
                    Some(table) => table.run_query(query),
                    None => Err(format!("Table `{}` not found!", query.table).to_string()),
                }
            }
            err => Err(format!("Failed to parse query! {:?}", err).to_string()),
        }
    }

    fn worker_loop(ruba: Arc<InnerRuba>) {
        loop {
            if let Some(task) = ruba.await_task() {
                task.execute();
            }
        }
    }

    fn await_task(&self) -> Option<Arc<Task>> {
        let &(ref lock, ref cvar) = &self.idle_queue;
        let mut task_available = lock.lock().unwrap();
        while !*task_available {
            task_available = cvar.wait(task_available).unwrap();
        }
        let mut task_queue_guard = self.task_queue.write().unwrap();
        let task_queue = task_queue_guard.deref_mut();
        while let Some(task) = task_queue.pop_front() {
            if task.completed() { continue; }
            if task.multithreaded() {
                task_queue.push_front(task.clone());
            }
            *task_available = task_queue.len() > 0;
            if *task_available {
                cvar.notify_one();
            }
            return Some(task);
        }
        None
    }

    pub fn schedule(&self, task: Arc<Task>) {
        // This function may be entered by event loop thread so it's important it always returns quickly.
        // Since the task queue/idle queue locks are never held for long, we should be fine.
        let &(ref lock, ref cvar) = &self.idle_queue;
        let mut task_available = lock.lock().unwrap();
        let mut task_queue_guard = self.task_queue.write().unwrap();
        let task_queue = task_queue_guard.deref_mut();
        task_queue.push_back(task);
        *task_available = true;
        cvar.notify_one();
    }

    pub fn load_table_data(&self) {
        let tables = self.tables.read().unwrap();
        for (_, table) in tables.iter() {
            table.load_table_data(self.storage.as_ref());
            println!("Finished loading {}", &table.name());
        }
        println!("Finished loading all table data!");
    }

    pub fn load_batches(&self, table: &str, batches: Vec<Batch>) {
        self.create_if_empty(table);
        let tables = self.tables.read().unwrap();
        let table = tables.get(table).unwrap();
        for batch in batches.into_iter() {
            table.load_batch(batch);
        }
    }

    pub fn ingest(&self, table: &str, row: Vec<(String, RawVal)>) {
        self.create_if_empty(table);
        let tables = self.tables.read().unwrap();
        tables.get(table).unwrap().ingest(row)
    }


    pub fn ingest_homogeneous(&self, table: &str, columns: HashMap<String, InputColumn>) {
        self.create_if_empty(table);
        let tables = self.tables.read().unwrap();
        tables.get(table).unwrap().ingest_homogeneous(columns)
    }

    pub fn ingest_heterogeneous(&self, table: &str, columns: HashMap<String, Vec<RawVal>>) {
        self.create_if_empty(table);
        let tables = self.tables.read().unwrap();
        tables.get(table).unwrap().ingest_heterogeneous(columns)
    }

    pub fn stats(&self) -> Stats {
        let tables = self.tables.read().unwrap();
        Stats {
            tables: tables.values().map(|table| table.stats()).collect()
        }
    }

    fn create_if_empty(&self, table: &str) {
        let exists = {
            let tables = self.tables.read().unwrap();
            tables.contains_key(table)
        };
        if !exists {
            {
                let mut tables = self.tables.write().unwrap();
                tables.insert(table.to_string(), Table::new(10_000, table, Metadata { batch_count: 0, name: table.to_string() }));
            }
            self.ingest("_meta_tables", vec![
                ("timestamp".to_string(), RawVal::Int(time::now().to_timespec().sec)),
                ("name".to_string(), RawVal::Str(table.to_string())),
            ]);
        }
    }
}

