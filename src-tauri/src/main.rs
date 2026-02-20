#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

#[derive(Serialize, Deserialize, Default, Clone)]
struct AppConfig {
  site: Option<String>,
  device_id: Option<String>,
  device_token: Option<String>,
  paired: bool,
}

fn config_path() -> PathBuf {
let mut dir = tauri::path::home_dir().unwrap_or(std::env::temp_dir());
  dir.push(".gentlesite");
  let _ = fs::create_dir_all(&dir);
  dir.push("gentle-phone-transfer.json");
  dir
}

fn load_config() -> AppConfig {
  let p = config_path();
  if let Ok(s) = fs::read_to_string(&p) {
    if let Ok(cfg) = serde_json::from_str::<AppConfig>(&s) {
      return cfg;
    }
  }
  AppConfig::default()
}

fn save_config(cfg: &AppConfig) -> Result<(), String> {
  let p = config_path();
  let s = serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?;
  fs::write(&p, s).map_err(|e| e.to_string())?;
  Ok(())
}

fn rest_base(site: &str) -> String {
  format!("{}/wp-json/gsdt/v1/phone-transfer", site.trim_end_matches('/'))
}

fn device_id() -> String {
  let host = whoami::hostname();
  let user = whoami::username();
  let raw = format!("{}|{}", host, user);
  format!("{:x}", md5::compute(raw))
}

#[tauri::command]
async fn pair(site: String, pairingCode: String) -> Result<serde_json::Value, String> {
  let site = site.trim().to_string();
  if !site.starts_with("http") { return Err("Site must start with http(s)://".into()); }
  let code = pairingCode.trim().to_string();
  if code.is_empty() { return Err("Pairing code is empty".into()); }

  let base = rest_base(&site);
  let url = format!("{}/pairing/claim", base);
  let did = device_id();

  let body = serde_json::json!({
    "pairing_code": code,
    "device_id": did,
    "device_name": whoami::hostname(),
  });

  let client = reqwest::Client::new();
  let resp = client.post(url).json(&body).send().await.map_err(|e| e.to_string())?;
  let status = resp.status();
  let txt = resp.text().await.map_err(|e| e.to_string())?;
  if !status.is_success() {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&txt) {
      if let Some(e) = v.get("error").and_then(|x| x.as_str()) {
        return Err(e.to_string());
      }
    }
    return Err(format!("Pair failed ({}): {}", status, txt));
  }

  let v: serde_json::Value = serde_json::from_str(&txt).map_err(|e| e.to_string())?;
  let token = v.get("device_token").and_then(|x| x.as_str()).unwrap_or("").to_string();
  let device_id = v.get("device_id").and_then(|x| x.as_str()).unwrap_or("").to_string();
  if token.is_empty() || device_id.is_empty() {
    return Err("Unexpected pairing response".into());
  }

  let mut cfg = load_config();
  cfg.site = Some(site);
  cfg.device_id = Some(device_id.clone());
  cfg.device_token = Some(token);
  cfg.paired = true;
  save_config(&cfg)?;

  Ok(serde_json::json!({ "device_id": device_id }))
}

#[tauri::command]
async fn heartbeat() -> Result<bool, String> {
  let cfg = load_config();
  if !cfg.paired { return Ok(false); }
  let site = cfg.site.clone().unwrap_or_default();
  let token = cfg.device_token.clone().unwrap_or_default();
  if site.is_empty() || token.is_empty() { return Ok(false); }

  let base = rest_base(&site);
  let url = format!("{}/device/heartbeat", base);

  let body = serde_json::json!({
    "device_id": cfg.device_id.clone().unwrap_or_default(),
    "utility_version": "0.1.0",
    "status": "ready"
  });

  let client = reqwest::Client::new();
  let resp = client.post(url).bearer_auth(token).json(&body).send().await.map_err(|e| e.to_string())?;
  Ok(resp.status().is_success())
}

#[tauri::command]
fn get_config() -> Result<serde_json::Value, String> {
  let cfg = load_config();
  Ok(serde_json::json!({ "site": cfg.site, "paired": cfg.paired }))
}

fn main() {
  tauri::Builder::default()
    .invoke_handler(tauri::generate_handler![pair, heartbeat, get_config])
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
