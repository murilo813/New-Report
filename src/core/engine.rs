use encoding_rs::WINDOWS_1252;
use memmap2::Mmap;
use regex::Regex;
use rusqlite::Connection as SqliteConn;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs::File;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use dotenvy::dotenv;
use std::env;
use std::io::Write;

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
    pub sqlite: SqliteConn,
    pub schema: BTreeMap<String, TableConfig>,
    pub base_path: String,
}

enum WorkerMsg {
    Chunk {
        table_name: String,
        rows: Vec<Vec<rusqlite::types::Value>>,
        processed_count: usize,
    },
    Error(String),
}

impl DataEngine {
    pub fn new_empty() -> Self {
        Self {
            sqlite: SqliteConn::open_in_memory().unwrap_or_else(|_| SqliteConn::open_in_memory().unwrap()),
            schema: BTreeMap::new(),
            base_path: String::new(),
        }
    }

    pub fn new() -> Self {
        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(dir) = exe_path.parent() { dotenvy::from_path(dir.join(".env")).ok(); }
        }
        dotenv().ok(); 

        let conn = SqliteConn::open_in_memory().unwrap_or_else(|_| SqliteConn::open_in_memory().unwrap());
        let base_path = env::var("DB_PATH").map(|v| v.replace('"', "")).unwrap_or_else(|_| r"C:\BmSoft\Bases\zecao".to_string());
        
        let mut schema = BTreeMap::new();
        if let Ok(toml_content) = std::fs::read_to_string("schema.toml") {
            if let Ok(parsed) = toml::from_str(&toml_content) { schema = parsed; }
        } else if let Ok(exe_path) = std::env::current_exe() {
            if let Some(dir) = exe_path.parent() {
                if let Ok(toml_content) = std::fs::read_to_string(dir.join("schema.toml")) {
                    if let Ok(parsed) = toml::from_str(&toml_content) { schema = parsed; }
                }
            }
        }

        Self { sqlite: conn, schema, base_path }
    }

    fn decode_db_string(bytes: &[u8]) -> String {
        let (decoded, _, _) = WINDOWS_1252.decode(bytes);
        decoded.trim_matches(|c: char| c == '\0' || c.is_whitespace()).to_string()
    }

    fn parse_sync_header(&self, sql: &str) -> Vec<(String, Vec<String>)> {
        let mut tasks = Vec::new();
        if let Ok(re_header) = Regex::new(r"(?i)\[SYNC:\s*(?s)(.*?)\]") {
            if let Some(content) = re_header.captures(sql).and_then(|caps| caps.get(1)) {
                if let Ok(re_table) = Regex::new(r"([a-zA-Z0-9_]+)\s*\((.*?)\)") {
                    for cap in re_table.captures_iter(content.as_str()) {
                        let table_name = cap[1].to_string();
                        let col_str = cap[2].trim();
                        let cols = if col_str == "*" { vec!["*".to_string()] } else { col_str.split(',').map(|s| s.trim().to_string()).collect() };
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
        mut on_progress: F,
    ) -> Result<(), String>
    where
        F: FnMut(f32) + Send + 'static,
    {
        let sync_tasks = self.parse_sync_header(user_sql);
        if sync_tasks.is_empty() { return Err("Tag [SYNC: ...] não encontrada ou formato inválido!".to_string()); }
        if self.schema.is_empty() { return Err("schema.toml não encontrado ou vazio! O motor não sabe os tipos das colunas.".to_string()); }

        let _ = self.sqlite.execute("PRAGMA synchronous = OFF", []);
        let _ = self.sqlite.execute("PRAGMA journal_mode = MEMORY", []);

        let mut total_rows_overall = 0;
        for (table_name, _) in &sync_tasks {
            let dat_path = format!(r"{}/{}.dat", self.base_path, table_name);
            if let Ok(file) = File::open(&dat_path) {
                if let Ok(mmap) = unsafe { Mmap::map(&file) } {
                    if mmap.len() >= 49 { 
                        total_rows_overall += mmap.get(0x29..0x2D).and_then(|s| s.try_into().ok()).map(u32::from_le_bytes).unwrap_or(0) as usize; 
                    }
                }
            }
        }
        if total_rows_overall == 0 { total_rows_overall = 1; }
        let total_rows_f32 = total_rows_overall as f32;

        let mut insert_sqls = std::collections::HashMap::new();

        for (table_name, requested_cols) in &sync_tasks {
            let config = self.schema.iter()
                .find(|(k, _)| k.to_lowercase() == table_name.to_lowercase())
                .map(|(_, v)| v.clone())
                .ok_or_else(|| format!("Tabela {} não mapeada no schema!", table_name))?;

            let target_columns: Vec<Column> = if requested_cols.len() == 1 && requested_cols[0] == "*" {
                config.columns.clone()
            } else {
                config.columns.iter().filter(|c| requested_cols.iter().any(|rc| rc.to_lowercase() == c.name.to_lowercase())).cloned().collect()
            };

            let mut col_defs = Vec::new();
            for col in &target_columns {
                let dtype = match col.field_type.as_str() { "I" => "INTEGER", "F" => "REAL", _ => "TEXT" };
                col_defs.push(format!("\"{}\" {}", col.name, dtype));
            }
            
            let _ = self.sqlite.execute(&format!("DROP TABLE IF EXISTS {}", table_name), []);
            let create_sql = format!("CREATE TABLE {} ({})", table_name, col_defs.join(", "));
            self.sqlite.execute(&create_sql, []).map_err(|e| e.to_string())?;

            let placeholders = vec!["?"; target_columns.len()].join(", ");
            let insert_sql = format!("INSERT INTO {} VALUES ({})", table_name, placeholders);
            insert_sqls.insert(table_name.clone(), (insert_sql, config, target_columns));
        }

        let (tx, rx) = std::sync::mpsc::channel();
        let mut handles = Vec::new();

        for (table_name, (_ignorado, config, target_columns)) in insert_sqls.clone() {
            let tx_clone = tx.clone();
            let cancel = cancel_flag.clone();
            let base_path_clone = self.base_path.clone();

            handles.push(std::thread::spawn(move || {
                let res = (|| -> Result<(), String> {
                    let dat_path = format!(r"{}/{}.dat", base_path_clone, table_name);
                    let file = File::open(&dat_path).map_err(|e| format!("Erro ao abrir {}: {}", dat_path, e))?;
                    let mmap = unsafe { Mmap::map(&file).map_err(|e| e.to_string())? };

                    if mmap.len() < 512 { return Ok(()); }

                    let total_fields = mmap.get(0x2F..0x31).and_then(|s| s.try_into().ok()).map(u16::from_le_bytes).unwrap_or(0) as usize;
                    let data_offset = 0x200 + (total_fields * 768);
                    let total_rows_expected = mmap.get(0x29..0x2D).and_then(|s| s.try_into().ok()).map(u32::from_le_bytes).unwrap_or(0);

                    let mut chunk = Vec::with_capacity(10_000);
                    let mut count = 0;
                    let mut i = data_offset;

                    while i + config.record_size as usize <= mmap.len() && count < total_rows_expected {
                        if cancel.load(Ordering::SeqCst) { return Err("Operação cancelada pelo usuário".to_string()); }

                        if let Some(row_data) = mmap.get(i..i + config.record_size as usize) {
                            if row_data[0] == 0 { 
                                let mut row_vals = Vec::with_capacity(target_columns.len());
                                for col in &target_columns {
                                    let start = col.offset as usize + 1;
                                    let end = start + col.length as usize;

                                    let val = match col.field_type.as_str() {
                                        "I" => {
                                            let v = i32::from_le_bytes(row_data.get(start..start+4).and_then(|s| s.try_into().ok()).unwrap_or([0;4]));
                                            rusqlite::types::Value::Integer(v as i64)
                                        },
                                        "F" => {
                                            let v = f64::from_le_bytes(row_data.get(start..start+8).and_then(|s| s.try_into().ok()).unwrap_or([0;8]));
                                            rusqlite::types::Value::Real(v)
                                        },
                                        "D" => {
                                            let days = i32::from_le_bytes(row_data.get(start..start+4).and_then(|s| s.try_into().ok()).unwrap_or([0;4]));
                                            if days > 0 { rusqlite::types::Value::Text(convert_dbisam_to_iso(days)) } 
                                            else { rusqlite::types::Value::Null }
                                        },
                                        _ => {
                                            if let Some(slice) = row_data.get(start..end) {
                                                rusqlite::types::Value::Text(Self::decode_db_string(slice))
                                            } else {
                                                rusqlite::types::Value::Null
                                            }
                                        }
                                    };
                                    row_vals.push(val);
                                }
                                chunk.push(row_vals);
                                count += 1;
                            }
                        }
                        i += config.record_size as usize;

                        if chunk.len() >= 10_000 {
                            let mut to_send = Vec::with_capacity(10_000);
                            std::mem::swap(&mut chunk, &mut to_send); 
                            if tx_clone.send(WorkerMsg::Chunk {
                                table_name: table_name.clone(),
                                rows: to_send,
                                processed_count: 10_000,
                            }).is_err() {
                                return Err("Main thread fechou o canal".to_string());
                            }
                        }
                    }

                    if !chunk.is_empty() {
                        let len = chunk.len();
                        let _ = tx_clone.send(WorkerMsg::Chunk { table_name: table_name.clone(), rows: chunk, processed_count: len });
                    }

                    Ok(())
                })();

                if let Err(e) = res {
                    let _ = tx_clone.send(WorkerMsg::Error(e));
                }
            }));
        }

        drop(tx);

        let mut db_tx = self.sqlite.transaction().map_err(|e| e.to_string())?;
        let mut total_processed = 0;
        let mut final_error = None;

        for msg in rx {
            if cancel_flag.load(Ordering::SeqCst) { break; }

            match msg {
                WorkerMsg::Chunk { table_name, rows, processed_count } => {
                    let (sql, _, _) = insert_sqls.get(&table_name).ok_or("Erro interno no cache SQL".to_string())?;
                    let mut stmt = db_tx.prepare_cached(sql).map_err(|e| e.to_string())?;
                    for row in rows {
                        stmt.execute(rusqlite::params_from_iter(row)).map_err(|e| e.to_string())?;
                    }
                    
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

        db_tx.commit().map_err(|e| e.to_string())?;

        for h in handles { let _ = h.join(); }

        Ok(())
    }

    pub fn execute_user_sql(&self, sql: &str) -> Result<rusqlite::Statement<'_>, String> {
        let re_sync = Regex::new(r"(?i)\[SYNC:\s*(?s)(.*?)\]").map_err(|e| e.to_string())?;
        let clean_sql = re_sync.replace_all(sql, "").to_string();

        let commands: Vec<&str> = clean_sql.split(';').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
        if commands.is_empty() { return Err("SQL vazio".to_string()); }

        for i in 0..(commands.len() - 1) {
            let cmd = commands[i];
            self.sqlite.execute(cmd, []).map_err(|e| format!("Erro no comando {}: {}", cmd, e))?;
            let _ = self.sqlite.execute("PRAGMA schema_version", []); 
        }

        let last_query = commands.last().unwrap_or(&"");
        self.sqlite.prepare(last_query).map_err(|e| format!("Erro no SELECT final: {}", e))
    }
}

fn convert_dbisam_to_iso(days: i32) -> String {
    let epoch_days = days - 719163;
    let seconds = (epoch_days as i64) * 86400;
    if let Some(t) = std::time::SystemTime::UNIX_EPOCH.checked_add(std::time::Duration::from_secs(seconds.max(0) as u64)) {
        let datetime: chrono::DateTime<chrono::Utc> = t.into();
        return datetime.format("%Y-%m-%d").to_string();
    }
    "0001-01-01".to_string()
}

pub fn append_log(report_name: &str, stage: &str, duration_ms: u128) {
    let now = chrono::Local::now().format("%d/%m/%Y %H:%M:%S");
    let log_line = format!("(RELATÓRIO: {} | {}) -> {} levou {} ms\n", report_name, now, stage, duration_ms);
    
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open("logs_desempenho.txt") {
        let _ = file.write_all(log_line.as_bytes());
    }
}