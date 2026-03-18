use datafusion::arrow::array::{
    ArrayRef, BooleanBuilder, Date32Builder, Float64Builder, Int64Builder, StringBuilder,
};
use datafusion::arrow::datatypes::{DataType, Field, Schema as ArrowSchema};
use datafusion::arrow::record_batch::RecordBatch;
use datafusion::datasource::MemTable;
use datafusion::prelude::*;
use dotenvy::dotenv;
use encoding_rs::WINDOWS_1252;
use memmap2::Mmap;
use rayon::prelude::*;
use regex::Regex;
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
use std::env;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

const DBISAM_HEADER_MIN_LEN: usize = 512;
const DBISAM_OFFSET_TOTAL_ROWS: std::ops::Range<usize> = 0x29..0x2D;
const DBISAM_OFFSET_TOTAL_FIELDS: std::ops::Range<usize> = 0x2F..0x31;
const DBISAM_BASE_HEADER_SIZE: usize = 0x200; // 512 bytes
const DBISAM_FIELD_DEF_SIZE: usize = 768;
const CHUNK_SIZE: usize = 100_000;

#[derive(Deserialize, Debug, Clone)]
pub struct Column {
    pub name: String,
    pub field_type: String,
    pub offset: u32,
    pub length: u32,
}

#[derive(Deserialize, Debug, Clone)]
pub struct TableConfig {
    pub record_size: u32,
    pub columns: Vec<Column>,
}

pub struct DataEngine {
    pub ctx: SessionContext,
    pub schema: BTreeMap<String, TableConfig>,
    pub base_path: String,
    pub cached_results: Arc<Mutex<Vec<RecordBatch>>>,
}

enum WorkerMsg {
    Batch {
        table_name: String,
        batch: RecordBatch,
        processed_count: usize,
    },
    Error(String),
}

enum ColBuilder {
    Int(Int64Builder),
    Float(Float64Builder),
    Date(Date32Builder),
    Text(StringBuilder),
    Bool(BooleanBuilder),
}

impl DataEngine {
    pub fn new_empty() -> Self {
        Self {
            ctx: SessionContext::new(),
            schema: BTreeMap::new(),
            base_path: String::new(),
            cached_results: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn new() -> Self {
        if let Ok(exe_path) = env::current_exe() {
            if let Some(dir) = exe_path.parent() {
                dotenvy::from_path(dir.join(".env")).ok();
            }
        }
        dotenv().ok();

        let base_path = env::var("DB_PATH")
            .map(|v| v.replace('"', ""))
            .unwrap_or_else(|_| r"C:\BmSoft\Bases\zecao".to_string());

        let mut schema = BTreeMap::new();

        if let Ok(toml_content) = std::fs::read_to_string("schema.toml") {
            if let Ok(parsed) = toml::from_str(&toml_content) {
                schema = parsed;
            }
        } else if let Ok(exe_path) = env::current_exe() {
            if let Some(dir) = exe_path.parent() {
                if let Ok(toml_content) = std::fs::read_to_string(dir.join("schema.toml")) {
                    if let Ok(parsed) = toml::from_str(&toml_content) {
                        schema = parsed;
                    }
                }
            }
        }

        Self {
            ctx: SessionContext::new(),
            schema,
            base_path,
            cached_results: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn decode_db_string(bytes: &[u8]) -> String {
        let (decoded, _, _) = WINDOWS_1252.decode(bytes);
        decoded
            .trim_matches(|c: char| c == '\0' || c.is_whitespace())
            .to_string()
    }

    fn parse_sync_header(&self, sql: &str) -> Vec<(String, Vec<String>)> {
        let mut tasks = Vec::new();
        if let Ok(re_header) = Regex::new(r"(?i)\[SYNC:\s*(?s)(.*?)\]") {
            if let Some(content) = re_header.captures(sql).and_then(|caps| caps.get(1)) {
                if let Ok(re_table) = Regex::new(r"([a-zA-Z0-9_]+)\s*\((.*?)\)") {
                    for cap in re_table.captures_iter(content.as_str()) {
                        let table_name = cap[1].to_string();
                        let col_str = cap[2].trim();
                        let cols = if col_str == "*" {
                            vec!["*".to_string()]
                        } else {
                            col_str.split(',').map(|s| s.trim().to_string()).collect()
                        };
                        tasks.push((table_name, cols));
                    }
                }
            }
        }
        tasks
    }

    pub fn process_report_with_progress<F>(
        &mut self,
        user_sql: &str,
        cancel_flag: Arc<AtomicBool>,
        report_name: &str,
        mut on_progress: F,
    ) -> Result<(), String>
    where
        F: FnMut(f32) + Send + 'static,
    {
        self.ctx = SessionContext::new();

        let start_carga = std::time::Instant::now();
        let mut tempo_registro = 0;

        let sync_tasks = self.parse_sync_header(user_sql);
        if sync_tasks.is_empty() {
            return Err("Tag [SYNC: ...] não encontrada ou formato inválido!".to_string());
        }
        if self.schema.is_empty() {
            return Err("schema.toml não encontrado ou vazio!".to_string());
        }

        let mut total_rows_overall = 0;
        for (table_name, _) in &sync_tasks {
            let dat_path = format!(r"{}/{}.dat", self.base_path, table_name);
            if let Ok(file) = File::open(&dat_path) {
                if let Ok(mmap) = unsafe { Mmap::map(&file) } {
                    if mmap.len() >= 49 {
                        total_rows_overall += mmap
                            .get(DBISAM_OFFSET_TOTAL_ROWS)
                            .and_then(|s| s.try_into().ok())
                            .map(u32::from_le_bytes)
                            .unwrap_or(0) as usize;
                    }
                }
            }
        }
        if total_rows_overall == 0 {
            total_rows_overall = 1;
        }
        let total_rows_f32 = total_rows_overall as f32;

        let mut extract_jobs = HashMap::new();

        for (table_name, requested_cols) in &sync_tasks {
            let config = self
                .schema
                .iter()
                .find(|(k, _)| k.to_lowercase() == table_name.to_lowercase())
                .map(|(_, v)| v.clone())
                .ok_or_else(|| format!("Tabela {} não mapeada no schema!", table_name))?;

            let target_columns: Vec<Column> =
                if requested_cols.len() == 1 && requested_cols[0] == "*" {
                    config.columns.clone()
                } else {
                    config
                        .columns
                        .iter()
                        .filter(|c| {
                            requested_cols
                                .iter()
                                .any(|rc| rc.to_lowercase() == c.name.to_lowercase())
                        })
                        .cloned()
                        .collect()
                };

            extract_jobs.insert(table_name.clone(), (config, target_columns));
        }

        let (tx, rx) = mpsc::channel();
        let mut handles = Vec::new();

        for (table_name, (config, target_columns)) in extract_jobs.clone() {
            let tx_clone = tx.clone();
            let cancel = cancel_flag.clone();
            let base_path_clone = self.base_path.clone();

            handles.push(std::thread::spawn(move || {
                if let Err(e) = parse_dbisam_table(
                    base_path_clone,
                    table_name,
                    config,
                    target_columns,
                    tx_clone.clone(),
                    cancel,
                ) {
                    let _ = tx_clone.send(WorkerMsg::Error(e));
                }
            }));
        }
        drop(tx);

        let mut total_processed = 0;
        let mut final_error = None;
        let mut table_batches: HashMap<String, Vec<RecordBatch>> = HashMap::new();

        for msg in rx {
            if cancel_flag.load(Ordering::SeqCst) {
                break;
            }

            match msg {
                WorkerMsg::Batch {
                    table_name,
                    batch,
                    processed_count,
                } => {
                    let start_registro = std::time::Instant::now();

                    table_batches
                        .entry(table_name)
                        .or_insert_with(Vec::new)
                        .push(batch);

                    tempo_registro += start_registro.elapsed().as_millis();

                    total_processed += processed_count;
                    let progress_percent = (total_processed as f32 / total_rows_f32) * 100.0;
                    on_progress(progress_percent.min(100.0));
                }
                WorkerMsg::Error(e) => {
                    final_error = Some(e);
                    cancel_flag.store(true, Ordering::SeqCst);
                    break;
                }
            }
        }

        if let Some(err) = final_error {
            return Err(err);
        }

        for h in handles {
            let _ = h.join();
        }

        for (table_name, batches) in table_batches {
            if !batches.is_empty() {
                let schema = batches[0].schema();
                let mem_table = MemTable::try_new(schema, vec![batches])
                    .map_err(|e| format!("Erro ao mapear MemTable: {}", e))?;

                self.ctx
                    .register_table(table_name.to_lowercase().as_str(), Arc::new(mem_table))
                    .map_err(|e| format!("Erro ao registrar tabela {}: {}", table_name, e))?;
            }
        }

        let tempo_total = start_carga.elapsed().as_millis();
        let tempo_extracao = tempo_total.saturating_sub(tempo_registro);

        println!("⏱️ [EXTRAÇÃO DBISAM -> ARROW] {} ms", tempo_extracao);
        println!("⏱️ [REGISTRO DATAFUSION] {} ms", tempo_registro);

        append_log(
            report_name,
            "1. Extração DBISAM (.dat -> Arrow)",
            tempo_extracao,
        );
        append_log(
            report_name,
            "2. Registro DataFusion (Memória Colunar)",
            tempo_registro,
        );

        Ok(())
    }

    pub fn execute_user_sql(
        &self,
        sql: &str,
        report_name: &str,
    ) -> Result<(Vec<String>, usize), String> {
        let start_sql = std::time::Instant::now();

        let re_sync = Regex::new(r"(?i)\[SYNC:\s*(?s)(.*?)\]").map_err(|e| e.to_string())?;
        let clean_sql = re_sync.replace_all(sql, "").to_string();

        let commands: Vec<String> = clean_sql
            .split(';')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        if commands.is_empty() {
            return Err("SQL vazio".to_string());
        }

        let ctx = self.ctx.clone();
        let cache_ptr = self.cached_results.clone();

        let result = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();

            rt.block_on(async move {
                for i in 0..(commands.len() - 1) {
                    ctx.sql(&commands[i]).await.map_err(|e| e.to_string())?;
                }

                let last_query = commands.last().unwrap();
                let df = ctx.sql(last_query).await.map_err(|e| e.to_string())?;

                let batches = df.collect().await.map_err(|e| e.to_string())?;

                let mut total_rows = 0;
                let mut cols = Vec::new();

                if !batches.is_empty() {
                    let schema = batches[0].schema();
                    cols = schema.fields().iter().map(|f| f.name().clone()).collect();
                    total_rows = batches.iter().map(|b| b.num_rows()).sum();
                }

                let mut cache = cache_ptr.lock().unwrap();
                *cache = batches;

                Ok((cols, total_rows))
            })
        }).join().unwrap_or(Err("Erro crítico na thread do SQL".into()));

        let tempo_sql = start_sql.elapsed().as_millis();
        append_log(report_name, "3. Execução SQL (DataFusion)", tempo_sql);

        result
    }

    pub fn get_rows_slice(&self, offset: usize, limit: usize) -> Vec<Vec<String>> {
        let cache = self.cached_results.lock().unwrap();
        let mut rows_vec = Vec::new();
        let mut current_idx = 0;

        for batch in cache.iter() {
            let num_rows = batch.num_rows();
            
            if current_idx + num_rows <= offset {
                current_idx += num_rows;
                continue;
            }

            let num_cols = batch.num_columns();
            for row_idx in 0..num_rows {
                if current_idx >= offset && rows_vec.len() < limit {
                    let mut row_data = Vec::with_capacity(num_cols);
                    for col_idx in 0..num_cols {
                        let array = batch.column(col_idx);
                        let val_str = datafusion::arrow::util::display::array_value_to_string(
                            array, row_idx,
                        ).unwrap_or_default();
                        row_data.push(val_str);
                    }
                    rows_vec.push(row_data);
                }
                current_idx += 1;
                if rows_vec.len() >= limit { break; }
            }
            if rows_vec.len() >= limit { break; }
        }
        rows_vec
    }
}

// FUNÇÕES AUXILIARES E WORKERS
fn parse_dbisam_table(
    base_path: String,
    table_name: String,
    config: TableConfig,
    target_columns: Vec<Column>,
    tx: mpsc::Sender<WorkerMsg>,
    cancel: Arc<AtomicBool>,
) -> Result<(), String> {
    let dat_path = format!(r"{}/{}.dat", base_path, table_name);
    let file = File::open(&dat_path).map_err(|e| format!("Erro ao abrir {}: {}", dat_path, e))?;

    let mmap = unsafe { Mmap::map(&file).map_err(|e| e.to_string())? };

    if mmap.len() < DBISAM_HEADER_MIN_LEN {
        return Ok(());
    }

    let total_fields = mmap
        .get(DBISAM_OFFSET_TOTAL_FIELDS)
        .and_then(|s| s.try_into().ok())
        .map(u16::from_le_bytes)
        .unwrap_or(0) as usize;
    let data_offset = DBISAM_BASE_HEADER_SIZE + (total_fields * DBISAM_FIELD_DEF_SIZE);
    let total_rows_expected = mmap
        .get(DBISAM_OFFSET_TOTAL_ROWS)
        .and_then(|s| s.try_into().ok())
        .map(u32::from_le_bytes)
        .unwrap_or(0);

    let mut arrow_fields = Vec::new();
    let mut builders: Vec<ColBuilder> = Vec::new();

    for col in &target_columns {
        let normalized_name = col.name.to_lowercase();

        match col.field_type.as_str() {
            "I" => {
                if col.length == 1 {
                    arrow_fields.push(Field::new(&normalized_name, DataType::Boolean, true));
                    builders.push(ColBuilder::Bool(BooleanBuilder::new()));
                } else {
                    arrow_fields.push(Field::new(&normalized_name, DataType::Int64, true));
                    builders.push(ColBuilder::Int(Int64Builder::new()));
                }
            }
            "F" => {
                arrow_fields.push(Field::new(&normalized_name, DataType::Float64, true));
                builders.push(ColBuilder::Float(Float64Builder::new()));
            }
            "D" => {
                arrow_fields.push(Field::new(&normalized_name, DataType::Date32, true));
                builders.push(ColBuilder::Date(Date32Builder::new()));
            }
            _ => {
                arrow_fields.push(Field::new(&normalized_name, DataType::Utf8, true));
                builders.push(ColBuilder::Text(StringBuilder::new()));
            }
        }
    }

    let arrow_schema = Arc::new(ArrowSchema::new(arrow_fields));

    let row_indexes: Vec<u32> = (0..total_rows_expected).collect();

    row_indexes.par_chunks(CHUNK_SIZE).try_for_each(|chunk| {
        if cancel.load(Ordering::SeqCst) {
            return Err("Operação cancelada pelo usuário".to_string());
        }

        let mut local_builders = create_builders_from_cols(&target_columns);
        let mut local_count = 0;

        for &row_idx in chunk {
            let offset_da_linha = data_offset + (row_idx as usize * config.record_size as usize);

            if let Some(row_data) =
                mmap.get(offset_da_linha..offset_da_linha + config.record_size as usize)
            {
                if row_data[0] == 0 {
                    for (col_idx, col) in target_columns.iter().enumerate() {
                        let start = col.offset as usize + 1;
                        let end = start + col.length as usize;

                        match &mut local_builders[col_idx] {
                            ColBuilder::Int(b) => {
                                let val = match col.length {
                                    1 => row_data[start] as i64,
                                    2 => i16::from_le_bytes(
                                        row_data[start..start + 2].try_into().unwrap_or([0; 2]),
                                    ) as i64,
                                    _ => i32::from_le_bytes(
                                        row_data[start..start + 4].try_into().unwrap_or([0; 4]),
                                    ) as i64,
                                };
                                b.append_value(val);
                            }
                            ColBuilder::Float(b) => {
                                let v = f64::from_le_bytes(
                                    row_data[start..start + 8].try_into().unwrap_or([0; 8]),
                                );
                                b.append_value(v);
                            }
                            ColBuilder::Date(b) => {
                                let days = i32::from_le_bytes(
                                    row_data[start..start + 4].try_into().unwrap_or([0; 4]),
                                );
                                if days > 0 {
                                    b.append_value(days - 719163);
                                } else {
                                    b.append_null();
                                }
                            }
                            ColBuilder::Text(b) => {
                                if let Some(slice) = row_data.get(start..end) {
                                    b.append_value(DataEngine::decode_db_string(slice));
                                } else {
                                    b.append_null();
                                }
                            }
                            ColBuilder::Bool(b) => {
                                b.append_value(row_data[start] != 0);
                            }
                        }
                    }
                    local_count += 1;
                }
            }
        }

        if local_count > 0 {
            send_batch(
                &table_name,
                &arrow_schema,
                &mut local_builders,
                local_count,
                &tx,
            )?;
        }

        Ok(())
    })?;

    Ok(())
}

fn create_builders_from_cols(target_columns: &[Column]) -> Vec<ColBuilder> {
    target_columns
        .iter()
        .map(|col| match col.field_type.as_str() {
            "I" => {
                if col.length == 1 {
                    ColBuilder::Bool(BooleanBuilder::new())
                } else {
                    ColBuilder::Int(Int64Builder::new())
                }
            }
            "F" => ColBuilder::Float(Float64Builder::new()),
            "D" => ColBuilder::Date(Date32Builder::new()),
            _ => ColBuilder::Text(StringBuilder::new()),
        })
        .collect()
}

fn send_batch(
    table_name: &str,
    schema: &Arc<ArrowSchema>,
    builders: &mut Vec<ColBuilder>,
    count: usize,
    tx: &mpsc::Sender<WorkerMsg>,
) -> Result<(), String> {
    let arrays: Vec<ArrayRef> = builders
        .iter_mut()
        .map(|b| match b {
            ColBuilder::Int(b) => Arc::new(b.finish()) as ArrayRef,
            ColBuilder::Float(b) => Arc::new(b.finish()) as ArrayRef,
            ColBuilder::Date(b) => Arc::new(b.finish()) as ArrayRef,
            ColBuilder::Text(b) => Arc::new(b.finish()) as ArrayRef,
            ColBuilder::Bool(b) => Arc::new(b.finish()) as ArrayRef,
        })
        .collect();

    let batch = RecordBatch::try_new(schema.clone(), arrays).map_err(|e| e.to_string())?;

    tx.send(WorkerMsg::Batch {
        table_name: table_name.to_string(),
        batch,
        processed_count: count,
    })
    .map_err(|_| "Falha ao enviar o batch Arrow".to_string())?;

    Ok(())
}

pub fn append_log(report_name: &str, stage: &str, duration_ms: u128) {
    let now = chrono::Local::now().format("%d/%m/%Y %H:%M:%S");
    let log_line = format!(
        "(RELATÓRIO: {} | {}) -> {} levou {} ms\n",
        report_name, now, stage, duration_ms
    );

    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open("logs_desempenho.txt")
    {
        let _ = file.write_all(log_line.as_bytes());
    }
}
