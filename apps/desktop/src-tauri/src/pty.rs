//! PTY session management — real terminal sessions streamed to the frontend.
//!
//! Each session spawns a real PTY process (e.g. `limactl shell agentbox` or
//! `docker exec -it <container> /bin/sh`) and streams I/O via Tauri events.
//!
//! Limits:
//! - Max concurrent sessions: `MAX_SESSIONS`
//! - Auto-close after inactivity: `SESSION_TTL`

use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::Arc;
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use tauri::{AppHandle, Emitter};
use tokio::sync::Mutex;

/// Maximum number of concurrent PTY sessions.
const MAX_SESSIONS: usize = 5;

/// Sessions are closed after this much inactivity.
const SESSION_TTL: Duration = Duration::from_secs(30 * 60); // 30 min

/// A single PTY session.
struct PtySession {
    /// Writer end of the PTY master (for sending input).
    writer: Box<dyn Write + Send>,
    /// Handle to the child process.
    _child: Box<dyn portable_pty::Child + Send + Sync>,
    /// Pair kept alive to prevent PTY from closing.
    _pair_master: Box<dyn portable_pty::MasterPty + Send>,
    /// Last activity timestamp (read or write).
    last_active: Instant,
    /// Agent ID this session belongs to.
    agent_id: String,
}

/// Manages multiple PTY sessions.
pub struct PtySessionManager {
    sessions: Arc<Mutex<HashMap<String, PtySession>>>,
}

impl PtySessionManager {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Spawn a new PTY session and start streaming output.
    /// Returns the session ID.
    pub async fn spawn(
        &self,
        session_id: String,
        agent_id: String,
        command: Vec<String>,
        rows: u16,
        cols: u16,
        app: AppHandle,
    ) -> Result<(), String> {
        let mut sessions = self.sessions.lock().await;

        // Enforce concurrent session limit
        if sessions.len() >= MAX_SESSIONS {
            return Err(format!("已达到最大会话数限制 ({MAX_SESSIONS})，请关闭其他终端后重试"));
        }

        // Don't allow duplicate session IDs
        if sessions.contains_key(&session_id) {
            return Err("会话已存在".into());
        }

        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| format!("无法创建终端: {e}"))?;

        let mut cmd = CommandBuilder::new(&command[0]);
        if command.len() > 1 {
            cmd.args(&command[1..]);
        }

        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| format!("无法启动终端进程: {e}"))?;

        // Get a reader from the master end for streaming output
        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| format!("无法读取终端输出: {e}"))?;

        // Get a writer for sending input
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| format!("无法写入终端: {e}"))?;

        let session = PtySession {
            writer,
            _child: child,
            _pair_master: pair.master,
            last_active: Instant::now(),
            agent_id,
        };

        sessions.insert(session_id.clone(), session);
        drop(sessions);

        // Spawn a background thread to read PTY output and emit events
        let sid = session_id.clone();
        let sessions_ref = self.sessions.clone();
        std::thread::spawn(move || {
            Self::read_loop(reader, sid, sessions_ref, app);
        });

        // Spawn TTL reaper for this session
        let sid = session_id.clone();
        let sessions_ref = self.sessions.clone();
        tokio::spawn(async move {
            Self::ttl_reaper(sid, sessions_ref).await;
        });

        Ok(())
    }

    /// Write data to a PTY session.
    pub async fn write(&self, session_id: &str, data: &[u8]) -> Result<(), String> {
        let mut sessions = self.sessions.lock().await;
        let session = sessions
            .get_mut(session_id)
            .ok_or_else(|| "会话不存在或已关闭".to_string())?;

        session.last_active = Instant::now();
        session
            .writer
            .write_all(data)
            .map_err(|e| format!("写入终端失败: {e}"))?;
        session
            .writer
            .flush()
            .map_err(|e| format!("刷新终端失败: {e}"))?;
        Ok(())
    }

    /// Resize a PTY session.
    pub async fn resize(&self, session_id: &str, rows: u16, cols: u16) -> Result<(), String> {
        let mut sessions = self.sessions.lock().await;
        let session = sessions
            .get_mut(session_id)
            .ok_or_else(|| "会话不存在或已关闭".to_string())?;

        session.last_active = Instant::now();
        session
            ._pair_master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| format!("调整终端大小失败: {e}"))?;
        Ok(())
    }

    /// Close a PTY session.
    pub async fn close(&self, session_id: &str) {
        let mut sessions = self.sessions.lock().await;
        // Dropping the session closes the PTY and kills the child process.
        sessions.remove(session_id);
    }

    /// Background read loop: reads from PTY and emits Tauri events.
    fn read_loop(
        mut reader: Box<dyn Read + Send>,
        session_id: String,
        sessions: Arc<Mutex<HashMap<String, PtySession>>>,
        app: AppHandle,
    ) {
        let mut buf = [0u8; 4096];
        let event_name = format!("pty-output-{session_id}");

        loop {
            match reader.read(&mut buf) {
                Ok(0) => {
                    // EOF — process exited
                    let _ = app.emit(&event_name, "\r\n[进程已退出]\r\n");
                    let _ = app.emit(
                        &format!("pty-exit-{session_id}"),
                        serde_json::json!({"session_id": session_id}),
                    );
                    break;
                }
                Ok(n) => {
                    // Convert to String, preserving all terminal escape sequences
                    let data = String::from_utf8_lossy(&buf[..n]).to_string();
                    let _ = app.emit(&event_name, &data);

                    // Update last_active (best-effort, don't block on lock)
                    let sessions = sessions.clone();
                    let sid = session_id.clone();
                    tokio::spawn(async move {
                        if let Some(session) = sessions.lock().await.get_mut(&sid) {
                            session.last_active = Instant::now();
                        }
                    });
                }
                Err(e) => {
                    tracing::debug!(session_id = %session_id, error = %e, "PTY read error");
                    let _ = app.emit(&event_name, format!("\r\n[终端连接已断开: {e}]\r\n"));
                    let _ = app.emit(
                        &format!("pty-exit-{session_id}"),
                        serde_json::json!({"session_id": session_id}),
                    );
                    break;
                }
            }
        }

        // Clean up session
        let sessions_clone = sessions.clone();
        let sid = session_id.clone();
        tokio::spawn(async move {
            sessions_clone.lock().await.remove(&sid);
        });
    }

    /// Periodically check if a session has exceeded the TTL and close it.
    async fn ttl_reaper(session_id: String, sessions: Arc<Mutex<HashMap<String, PtySession>>>) {
        loop {
            tokio::time::sleep(Duration::from_secs(60)).await;
            let mut guard = sessions.lock().await;
            if let Some(session) = guard.get(&session_id) {
                if session.last_active.elapsed() > SESSION_TTL {
                    tracing::info!(session_id = %session_id, agent_id = %session.agent_id,
                        "PTY session timed out, closing");
                    guard.remove(&session_id);
                    return;
                }
            } else {
                // Session already removed
                return;
            }
        }
    }
}
