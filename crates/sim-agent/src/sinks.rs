//! Veriyolu çıkışları (host tarafı I/O): JSONL dosyası ve canlı TCP yayını.

use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use sim_core::{Msg, Sink};

/// Her mesaj bir satır. Tick ve bitişte diske itilir: çökse bile kayıt tick sınırında tutarlı.
pub struct FileSink(BufWriter<File>);

impl FileSink {
    pub fn create(path: &Path) -> io::Result<Self> {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)?;
        }
        Ok(Self(BufWriter::new(File::create(path)?)))
    }
}

impl Sink for FileSink {
    fn publish(&mut self, msg: &Msg) {
        if serde_json::to_writer(&mut self.0, msg).is_ok() {
            self.0.write_all(b"\n").ok();
        }
        if matches!(msg, Msg::Tick { .. } | Msg::End { .. }) {
            self.0.flush().ok();
        }
    }
}

/// Bağlanan her istemciye her mesajı yazar. Yavaş ya da kopan istemci düşürülür;
/// simülasyon izleyiciyi beklemez (yazma zaman aşımı 200 ms).
pub struct TcpSink {
    clients: Arc<Mutex<Vec<TcpStream>>>,
}

impl TcpSink {
    pub fn listen(addr: &str) -> io::Result<(Self, String)> {
        let listener = TcpListener::bind(addr)?;
        let local = listener.local_addr()?.to_string();
        let clients: Arc<Mutex<Vec<TcpStream>>> = Arc::default();
        let accepted = clients.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                stream.set_write_timeout(Some(Duration::from_millis(200))).ok();
                stream.set_nodelay(true).ok();
                if let Ok(mut c) = accepted.lock() {
                    c.push(stream);
                }
            }
        });
        Ok((Self { clients }, local))
    }
}

impl Sink for TcpSink {
    fn publish(&mut self, msg: &Msg) {
        let Ok(mut line) = serde_json::to_vec(msg) else { return };
        line.push(b'\n');
        if let Ok(mut clients) = self.clients.lock() {
            clients.retain_mut(|c| c.write_all(&line).is_ok());
        }
    }
}
