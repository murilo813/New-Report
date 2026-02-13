use encoding_rs::WINDOWS_1252;
use memmap2::Mmap;
use regex::Regex;
use rusqlite::{Connection as SqliteConn, params_from_iter};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs::File;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use dotenvy::dotenv;
use std::env;

#[derive(Deserialize, Debug, Clone)]
pub struct Column {
    pub name: String,
    pub field_type: String,
    pub offset: u32,
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

impl DataEngine {
    pub fn new() -> Self {
        dotenv().ok();

        let db_path = "zecao.db";
        let conn = SqliteConn::open(db_path).expect("Falha ao abrir arquivo de banco");
        
        let base_path = env::var("DATABASE_PATH")
            .unwrap_or_else(|_| r"C:\BmSoft\Bases\zecao".to_string());

        let toml_content = std::fs::read_to_string("schema.toml")
            .expect("Arquivo schema.toml não encontrado na raiz!");
            
        let schema: BTreeMap<String, TableConfig> = toml::from_str(&toml_content)
            .expect("Erro ao processar o TOML de configuração!");

        Self { 
            sqlite: conn, 
            schema,
            base_path
        }
    }

    fn decode_db_string(bytes: &[u8]) -> String {
        let (decoded, _, _) = WINDOWS_1252.decode(bytes);
        decoded.trim_matches(|c: char| c == '\0' || c.is_whitespace()).to_string()
    }

    fn parse_sync_header(&self, sql: &str) -> Vec<(String, Vec<String>)> {
        let mut tasks = Vec::new();
        let re_header = Regex::new(r"(?i)\[SYNC:\s*(?s)(.*?)\]").unwrap();
        
        if let Some(content) = re_header.captures(sql).and_then(|caps| caps.get(1)) {
            let re_table = Regex::new(r"([a-zA-Z0-9_]+)\s*\((.*?)\)").unwrap();
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
        let _ = self.sqlite.execute("PRAGMA synchronous = OFF", []);
        let _ = self.sqlite.execute("PRAGMA journal_mode = MEMORY", []);

        let sync_tasks = self.parse_sync_header(user_sql);
        let total_tasks = sync_tasks.len();

        if total_tasks == 0 {
            return Err("Tag [SYNC: ...] não encontrada!".to_string());
        }

        for (idx, (table_name, requested_cols)) in sync_tasks.iter().enumerate() {
            if cancel_flag.load(Ordering::SeqCst) { return Err("Cancelado".to_string()); }

            let config = self.schema.iter()
                .find(|(k, _)| k.to_lowercase() == table_name.to_lowercase())
                .map(|(_, v)| v.clone())
                .ok_or_else(|| format!("Tabela {} não mapeada!", table_name))?;

            let target_columns: Vec<Column> = if requested_cols.len() == 1 && requested_cols[0] == "*" {
                config.columns.clone()
            } else {
                config.columns.iter()
                    .filter(|c| requested_cols.iter().any(|rc| rc.to_lowercase() == c.name.to_lowercase()))
                    .cloned()
                    .collect()
            };

            let _ = self.sqlite.execute(&format!("DROP TABLE IF EXISTS {}", table_name), []);
            
            let mut col_defs = Vec::new();
            for col in &target_columns {
                let dtype = match col.field_type.as_str() {
                    "I" => "INTEGER",
                    "F" => "REAL",
                    "D" => "TEXT",
                    _ => "TEXT",
                };
                col_defs.push(format!("\"{}\" {}", col.name, dtype));
            }

            let create_sql = format!("CREATE TABLE {} ({})", table_name, col_defs.join(", "));
            self.sqlite.execute(&create_sql, []).map_err(|e| e.to_string())?;

            self.insert_bulk_manual(table_name, &config, &target_columns, &cancel_flag)?;

            let p = ((idx + 1) as f32 / total_tasks as f32) * 100.0;
            on_progress(p);
        }
        Ok(())
    }

    fn insert_bulk_manual(
        &mut self, 
        table_name: &str, 
        config: &TableConfig, 
        target_cols: &[Column], 
        cancel_flag: &AtomicBool
    ) -> Result<(), String> {
        let dat_path = format!(r"{}/{}.dat", self.base_path, table_name);
        let file = File::open(&dat_path).map_err(|e| format!("Erro ao abrir {}: {}", dat_path, e))?;
        let mmap = unsafe { Mmap::map(&file).map_err(|e| e.to_string())? };

        let total_fields = u16::from_le_bytes(mmap[0x2F..0x31].try_into().unwrap()) as usize;
        let data_offset = 0x200 + (total_fields * 768);
        let total_rows_expected = u32::from_le_bytes(mmap[0x29..0x2D].try_into().unwrap());


        let placeholders = vec!["?"; target_cols.len()].join(", ");
        let insert_sql = format!("INSERT INTO {} VALUES ({})", table_name, placeholders);

        let tx = self.sqlite.transaction().map_err(|e| e.to_string())?;
        let mut count = 0;
        {
            let mut stmt = tx.prepare(&insert_sql).map_err(|e| e.to_string())?;
            let mut i = data_offset;

            while i + config.record_size as usize <= mmap.len() {
                if cancel_flag.load(Ordering::SeqCst) { return Err("Cancelado".to_string()); }
                let row_data = &mmap[i..i + config.record_size as usize];

                if row_data[0] == 0 {
                    let mut params = Vec::with_capacity(target_cols.len());
                    for col in target_cols {
                        let start = col.offset as usize + 1;
                        match col.field_type.as_str() {
                            "I" => {
                                let val = i32::from_le_bytes(row_data[start..start+4].try_into().unwrap_or([0;4]));
                                
                                params.push(rusqlite::types::Value::Integer(val as i64));
                            },
                            "F" => {
                                let val = f64::from_le_bytes(row_data[start..start+8].try_into().unwrap_or([0;8]));
                                params.push(rusqlite::types::Value::Real(val));
                            },
                            "D" => {
                                let days = i32::from_le_bytes(row_data[start..start+4].try_into().unwrap_or([0;4]));
                                if days > 0 {
                                    params.push(rusqlite::types::Value::Text(convert_dbisam_to_iso(days)));
                                } else {
                                    params.push(rusqlite::types::Value::Null);
                                }
                            },
                            _ => {
                                let end = calculate_string_end(config, col, row_data.len());
                                let text = Self::decode_db_string(&row_data[start..end]);
                                
                                params.push(rusqlite::types::Value::Text(text));
                            }
                        }
                    }
                    stmt.execute(params_from_iter(params)).map_err(|e| e.to_string())?;
                    count += 1;
                }
                i += config.record_size as usize;
                if count >= total_rows_expected { break; }
            }
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn execute_user_sql(&self, sql: &str) -> Result<rusqlite::Statement, String> {
        let re_sync = Regex::new(r"(?i)\[SYNC:.*?\]").unwrap();
        let clean_sql = re_sync.replace_all(sql, "").to_string();

        let commands: Vec<&str> = clean_sql.split(';').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
        
        if commands.is_empty() { return Err("SQL vazio".to_string()); }

        for i in 0..(commands.len() - 1) {
            let cmd = commands[i];
            
            self.sqlite.execute(cmd, []).map_err(|e| format!("Erro no comando {}: {}", cmd, e))?;
            
            let _ = self.sqlite.execute("PRAGMA schema_version", []); 
        }

        let last_query = commands.last().unwrap();
        
        self.sqlite.prepare(last_query).map_err(|e| format!("Erro no SELECT final: {}", e))
    }
}

fn calculate_string_end(config: &TableConfig, current_col: &Column, row_len: usize) -> usize {
    let mut next_offset = row_len as u32;
    for c in &config.columns {
        if c.offset > current_col.offset && c.offset < next_offset {
            next_offset = c.offset;
        }
    }
    next_offset as usize
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