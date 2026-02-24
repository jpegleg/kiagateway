#![forbid(unsafe_code)]
use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt, copy_bidirectional};
use chrono::{SecondsFormat, Utc};
use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
struct Config {
    backends: HashMap<String, String>,
}

/// Try to find the `Host:` header in the HTTP request headers.
/// The function expects the whole HTTP headers (up to the \r\n\r\n).
fn extract_host(headers: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(headers).ok()?;
    for line in text.split("\r\n") {
        if line.to_lowercase().starts_with("host:") {
            let val = line[5..].trim();
            if let Some(pos) = val.rfind(':') {
                let maybe_port = &val[pos + 1..];
                if maybe_port.parse::<u16>().is_ok() {
                    return Some(val[..pos].to_lowercase());
                }
            }
            return Some(val.to_lowercase());
        }
    }
    None
}

/// Handle HTTP connections by inspecting the Host header
/// and passing the TCP stream to the configured backends.
async fn handle_http(mut client: TcpStream, config: Arc<Config>) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut header_buf = Vec::new();
    let mut tmp = [0u8; 1024];
    loop {
        let n = client.read(&mut tmp).await?;
        if n == 0 {
            return Ok(());
        }
        header_buf.extend_from_slice(&tmp[..n]);
        if header_buf.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
    }

    let host = match extract_host(&header_buf) {
        Some(h) => h,
        None => {
            client.write_all(b"HTTP/1.1 400 Bad Request\r\n\r\n").await?;
            return Ok(());
        }
    };

    let backend_addr = match config.backends.get(&host) {
        Some(addr) => addr.clone(),
        None => {
            client.write_all(b"HTTP/1.1 502 Bad Gateway\r\n\r\n").await?;
            return Ok(());
        }
    };

    let mut backend = TcpStream::connect(&backend_addr).await?;
    backend.write_all(&header_buf).await?;
    copy_bidirectional(&mut client, &mut backend).await?;
    Ok(())
}

/// Very small TLS ClientHello parser that extracts the
/// ServerName (SNI) extension (type 0).  It works for TLS 1.0 through 1.3.
/// The TCP stream is passed to the configured backend matching the SNI (tls host).
/// The stream is not decrypted or termianted at kiagateway, just the SNI read from it
/// and a bidirectional stream established if there is a configured backend for that
/// domain name.
fn extract_sni(data: &[u8]) -> Option<String> {
    if data.len() < 5 || data[0] != 0x16 {
        return None;
    }
    
    let record_len = ((data[3] as usize) << 8) | (data[4] as usize);
    
    if data.len() < 5 + record_len {
        return None;
    }

    let handshake = &data[5..5 + record_len];

    if handshake.is_empty() || handshake[0] != 0x01 {
        return None
    }

    let handshake_len = ((handshake[1] as usize) << 16)
        | ((handshake[2] as usize) << 8)
        | (handshake[3] as usize);

    if handshake.len() < 4 + handshake_len {
        return None;
    }

    let mut idx = 4;
    idx += 2 + 32;
    let session_id_len = handshake[idx] as usize;
    idx += 1 + session_id_len;
    let cipher_suites_len = ((handshake[idx] as usize) << 8) | (handshake[idx + 1] as usize);
    idx += 2 + cipher_suites_len;
    let compression_len = handshake[idx] as usize;
    idx += 1 + compression_len;
    let extensions_len = ((handshake[idx] as usize) << 8) | (handshake[idx + 1] as usize);
    idx += 2;
    let extensions_end = idx + extensions_len;

    while idx + 4 <= extensions_end {
        let ext_type = ((handshake[idx] as usize) << 8) | (handshake[idx + 1] as usize);
        let ext_len = ((handshake[idx + 2] as usize) << 8) | (handshake[idx + 3] as usize);
        idx += 4;

        if ext_type == 0 {
            if idx + 2 > handshake.len() {
                return None;
            }
            let _name_list_len = ((handshake[idx] as usize) << 8) | (handshake[idx + 1] as usize);
            idx += 2;
            if idx + 3 > handshake.len() {
                return None;
            }
            let name_type = handshake[idx];
            idx += 1;
            let name_len = ((handshake[idx] as usize) << 8) | (handshake[idx + 1] as usize);
            idx += 2;
            if name_type == 0 && idx + name_len <= handshake.len() {
                let sni = String::from_utf8_lossy(&handshake[idx..idx + name_len]).to_string();
                return Some(sni.to_lowercase());
            }
            return None;
        }
        idx += ext_len;
    }
    None
}

/// Handle a TLS encrypted connection.
async fn handle_https(mut client: TcpStream, config: Arc<Config>) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut initial = vec![0u8; 8192];
    let n = client.read(&mut initial).await?;
    if n == 0 {
        return Ok(());
    }
    initial.truncate(n);

    let sni = match extract_sni(&initial) {
        Some(s) => s,
        None => {
            return Ok(());
        }
    };

    let backend_addr = match config.backends.get(&sni) {
        Some(addr) => addr.clone(),
        None => {
            return Ok(());
        }
    };

    let mut backend = TcpStream::connect(&backend_addr).await?;
    backend.write_all(&initial).await?;
    copy_bidirectional(&mut client, &mut backend).await?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {

    let config_path = std::env::args().nth(1).unwrap_or_else(|| "servers.toml".to_string());
    let srversstr = std::fs::read_to_string(&config_path)?;
    let config: Config = toml::from_str(&srversstr)?;
    let config = Arc::new(config);
    let printcfg = srversstr.replace("\n", "");
    let ts = chrono::DateTime::<Utc>::from(SystemTime::now()).to_rfc3339_opts(SecondsFormat::Millis, true);

    println!("{ts} <-> kiagateway >>> service starting: HTTP (host header inspection) on port 80, HTTPS (passthrough inspection) on port 443");
    println!("{ts} <-> kiagateway >>> service config loaded: {}", printcfg);

    let http = TcpListener::bind("0.0.0.0:80").await?;
    let config_http = config.clone();

    tokio::spawn(async move {
        loop {
            match http.accept().await {
                Ok((socket, addr)) => {
                    let cfg = config_http.clone();
                    let txid = Uuid::new_v4().to_string();
                    tokio::spawn(async move {
                        if let Err(e) = handle_http(socket, cfg).await {
                            let ts = chrono::DateTime::<Utc>::from(SystemTime::now()).to_rfc3339_opts(SecondsFormat::Millis, true);
                            println!("{ts} - {txid} - kiagateway >>> HTTP ERROR {}: {}", addr, e);
                        }
                    });
                }
                Err(e) => {
                    let txid = Uuid::new_v4().to_string();
                    let ts = chrono::DateTime::<Utc>::from(SystemTime::now()).to_rfc3339_opts(SecondsFormat::Millis, true);
                    println!("{ts} - {txid} - kiagateway >>> HTTP accept ERROR: {}", e);
                }
            }
        }
    });

    let https = TcpListener::bind("0.0.0.0:443").await?;
    let config_https = config.clone();

    loop {
        match https.accept().await {
            Ok((socket, addr)) => {
                let cfg = config_https.clone();
                let txid = Uuid::new_v4().to_string();
                tokio::spawn(async move {
                    if let Err(e) = handle_https(socket, cfg).await {
                        let ts = chrono::DateTime::<Utc>::from(SystemTime::now()).to_rfc3339_opts(SecondsFormat::Millis, true);
                        println!("{ts} - {txid} - kiagateway >>> HTTPS ERROR {}: {}", addr, e);
                    }
                });
            }
            Err(e) => {
                let txid = Uuid::new_v4().to_string();
                let ts = chrono::DateTime::<Utc>::from(SystemTime::now()).to_rfc3339_opts(SecondsFormat::Millis, true);
                println!("{ts} - {txid} - kiagateway >>> HTTPS accept ERROR: {}", e);
            }
        }
    }
}
