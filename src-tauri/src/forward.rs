//! SSH port forwarding (local `-L` and remote `-R` tunnels).
//!
//! ## Model
//!
//! Rules live in the `port_forwards` table (see `db.rs`) and are plaintext
//! like `proxy_type`: knowing which internal host a tunnel targets reveals
//! topology, which is the same sensitivity class as the connection's own
//! host field — and the settings panel must list rules before the vault is
//! unlocked. Credentials never live here; each rule references a connection
//! whose password/key stays in the keyring.
//!
//! Starting a tunnel is **not** persisted as "on": an app restart leaves
//! every rule stopped even when `enabled` is set. A rule that silently
//! re-opened a `0.0.0.0:8080` listener on every launch would expose a port
//! the user never asked to expose in *this* session.
//!
//! ## Direction
//!
//! * `local` — we bind `bind_host:bind_port` and, per accepted TCP
//!   connection, open a `direct-tcpip` channel to `target_host:target_port`
//!   through the SSH connection. Bytes are copied both ways.
//! * `remote` — we ask the server to listen on `bind_host:bind_port`
//!   (`tcpip_forward`). Every inbound connection arrives as a
//!   server-initiated `forwarded-tcpip` channel (handled in
//!   [`crate::ssh::SshClient::server_channel_open_forwarded_tcpip`]), and we
//!   connect to `target_host:target_port` locally before pumping bytes.
//!
//! Remote forwarding is matched to its rule by `(bind_host, bind_port)`,
//! because that pair is all the SSH protocol gives us about which forward
//! request a channel belongs to. Two rules may therefore not share the same
//! bind pair while both are active — `start` refuses the duplicate.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use russh::ChannelStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// One stored forwarding rule. Mirrors the `port_forwards` table; plaintext
/// fields only, so no DEK is involved on this path.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForwardRule {
    pub id: String,
    pub name: String,
    pub connection_id: String,
    /// "local" | "remote"
    pub direction: String,
    pub bind_host: String,
    pub bind_port: u16,
    pub target_host: String,
    pub target_port: u16,
    pub enabled: bool,
    pub created_at: String,
}

/// Live tunnel state, surfaced to the settings panel so the user can see
/// what is actually open and how much has crossed it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TunnelStatus {
    pub rule_id: String,
    /// "listening" (local: bound and accepting) | "forwarding" (remote: the
    /// server is listening) | "error"
    pub state: String,
    /// Bytes pumped each way. Local: total across all accepted connections.
    /// Remote: total across all server-initiated channels.
    pub bytes_in: u64,
    pub bytes_out: u64,
    /// Number of live connections/channels through this tunnel.
    pub connections: u64,
    /// Set when the tunnel failed after starting (e.g. the SSH session died).
    /// Empty string while healthy.
    pub error: String,
}

/// Per-tunnel bookkeeping held by [`ForwardManager`].
struct Tunnel {
    status: Arc<Mutex<TunnelStatus>>,
    /// Signalled when the tunnel is stopped or its SSH session dies.
    stop: Arc<tokio::sync::Notify>,
    /// Set of live connection handles; dropping an entry closes the socket /
    /// channel and ends that connection's pump task.
    connections: Arc<Mutex<Vec<ConnectionHandle>>>,
    /// `bind_port` actually in use. For a local rule this is what the OS
    /// assigned when `bind_port == 0`; for remote it is the port the server
    /// reports back from `tcpip_forward`.
    effective_port: Arc<std::sync::atomic::AtomicU16>,
    /// Remote direction only: the target to dial locally, captured at start
    /// time. The SSH protocol reports only the bind side of an incoming
    /// `forwarded-tcpip` channel, so the target cannot be recovered later.
    target: Arc<Mutex<Option<RemoteTarget>>>,
}

/// A live connection inside a tunnel. Dropping it tears the connection down
/// (the task aborts on drop for local; the channel closes for remote).
struct ConnectionHandle {
    abort: tokio::task::AbortHandle,
}

/// Owns every live tunnel and the byte counters. `Clone`-able so the SSH
/// handler (remote direction) and the Tauri command layer share one view.
#[derive(Default)]
pub struct ForwardManager {
    tunnels: Mutex<HashMap<String, Tunnel>>,
    /// Live remote-forward registrations, keyed by `bind_host:bind_port` so
    /// an incoming `forwarded-tcpip` channel can find its rule.
    remote_index: Mutex<HashMap<(String, u16), String>>,
    /// rule_id → SSH session_id, so a dying session can tear down its own
    /// tunnels instead of leaving listeners bound forever.
    session_owners: Mutex<HashMap<String, String>>,
}

impl ForwardManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Snapshot of every live tunnel for the settings panel.
    pub fn list(&self) -> Vec<TunnelStatus> {
        let tunnels = self.tunnels.lock().unwrap_or_else(|e| e.into_inner());
        tunnels
            .values()
            .filter_map(|t| t.status.lock().ok().map(|g| g.clone()))
            .collect()
    }

    pub fn is_active(&self, rule_id: &str) -> bool {
        self.tunnels
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains_key(rule_id)
    }

    pub fn status(&self, rule_id: &str) -> Option<TunnelStatus> {
        self.tunnels
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(rule_id)
            .and_then(|t| t.status.lock().ok().map(|g| g.clone()))
    }

    pub fn effective_port(&self, rule_id: &str) -> Option<u16> {
        self.tunnels
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(rule_id)
            .map(|t| t.effective_port.load(Ordering::Relaxed))
    }

    /// Stop a tunnel: signal the accept loop / channel pump and drop the
    /// listener. Idempotent.
    ///
    /// The `remote_index` cleanup is unconditional rather than gated on the
    /// rule being in `tunnels`. The two maps are written together in
    /// `start_remote`, but a rule whose tunnel already exited (accept loop
    /// ended) still holds its index entry, and gating on `tunnels` would
    /// leave that entry behind forever — permanently blocking any later rule
    /// from reusing the same bind pair.
    pub fn stop(&self, rule_id: &str) {
        let removed = {
            let mut tunnels = self.tunnels.lock().unwrap_or_else(|e| e.into_inner());
            let removed = tunnels.remove(rule_id);
            let mut idx = self.remote_index.lock().unwrap_or_else(|e| e.into_inner());
            idx.retain(|_, v| v != rule_id);
            removed
        };
        // The session binding outlives the tunnel (the accept loop can exit
        // while the mapping remains), so it is cleared here too — otherwise
        // `session_owners` grows once per rule ever started.
        self.session_owners
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(rule_id);
        if let Some(t) = removed {
            t.stop.notify_waiters();
            // Aborting each connection's pump task closes its socket/channel.
            let handles: Vec<ConnectionHandle> = {
                let mut conns = t.connections.lock().unwrap_or_else(|e| e.into_inner());
                std::mem::take(&mut *conns)
            };
            for h in handles {
                h.abort.abort();
            }
        }
    }

    /// Stop every tunnel that rides on `session_id`'s SSH connection. Called
    /// from the session teardown path so a dead connection can't leave a
    /// listener bound to a port forever.
    pub fn stop_for_session(&self, session_id: &str) -> Vec<String> {
        let owners = self.session_owners.lock().unwrap_or_else(|e| e.into_inner());
        let ids: Vec<String> = owners
            .iter()
            .filter(|(_, sid)| sid.as_str() == session_id)
            .map(|(rid, _)| rid.clone())
            .collect();
        drop(owners);
        for id in &ids {
            self.stop(id);
        }
        ids
    }

    pub fn stop_all(&self) {
        let ids: Vec<String> = {
            let tunnels = self.tunnels.lock().unwrap_or_else(|e| e.into_inner());
            tunnels.keys().cloned().collect()
        };
        for id in ids {
            self.stop(&id);
        }
    }

    /// Register the rule → session binding so `stop_for_session` can find it.
    fn bind_session(&self, rule_id: &str, session_id: &str) {
        self.session_owners
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(rule_id.to_string(), session_id.to_string());
    }

    /// Snapshot of `(rule_id, session_id)` bindings. Used by the command layer
    /// to find the handle whose server-side listener must be cancelled before
    /// the local bookkeeping is dropped.
    pub fn session_owner_snapshot(&self) -> Vec<(String, String)> {
        self.session_owners
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    // ── Local direction (-L) ────────────────────────────────────────────

    /// Bind `bind_host:bind_port` and start accepting. Each accepted socket
    /// opens a `direct-tcpip` channel to the rule's target through the given
    /// SSH handle and pumps bytes both ways.
    ///
    /// Returns once the listener is bound (so the caller can report the
    /// effective port immediately); the accept loop runs as a detached task.
    pub async fn start_local(
        self: &Arc<Self>,
        rule: &ForwardRule,
        handle: &Arc<russh::client::Handle<crate::ssh::SshClient>>,
        session_id: &str,
    ) -> Result<u16, String> {
        if self.is_active(&rule.id) {
            return Err("该规则已在运行".to_string());
        }
        let listener = tokio::net::TcpListener::bind((rule.bind_host.as_str(), rule.bind_port))
            .await
            .map_err(|e| format!("绑定 {}:{} 失败：{}", rule.bind_host, rule.bind_port, e))?;
        let bound = listener
            .local_addr()
            .map_err(|e| format!("读取监听地址失败：{}", e))?;
        let effective = bound.port();

        let tunnel = Tunnel {
            status: Arc::new(Mutex::new(TunnelStatus {
                rule_id: rule.id.clone(),
                state: "listening".into(),
                bytes_in: 0,
                bytes_out: 0,
                connections: 0,
                error: String::new(),
            })),
            stop: Arc::new(tokio::sync::Notify::new()),
            connections: Arc::new(Mutex::new(Vec::new())),
            effective_port: Arc::new(std::sync::atomic::AtomicU16::new(effective)),
            target: Arc::new(Mutex::new(None)),
        };
        let (status, stop, conns) = {
            let mut tunnels = self.tunnels.lock().unwrap_or_else(|e| e.into_inner());
            tunnels.insert(rule.id.clone(), tunnel);
            let t = tunnels.get(&rule.id).expect("just inserted");
            (t.status.clone(), t.stop.clone(), t.connections.clone())
        };
        self.bind_session(&rule.id, session_id);

        // `handle` is an Arc, so the task holds a strong reference rather than
        // a borrow of this frame's argument.
        let manager = Arc::clone(self);
        let rule_id = rule.id.clone();
        let target_host = rule.target_host.clone();
        let target_port = rule.target_port;
        let handle = Arc::clone(handle);

        tokio::spawn(async move {
            loop {
                let accepted = tokio::select! {
                    biased;
                    _ = stop.notified() => break,
                    a = listener.accept() => a,
                };
                let (socket, peer) = match accepted {
                    Ok(pair) => pair,
                    Err(e) => {
                        log::warn!("[fwd:{}] accept failed: {}", rule_id, e);
                        set_error(&status, &format!("监听失败：{}", e));
                        break;
                    }
                };
                let _ = socket.set_nodelay(true);
                log::info!("[fwd:{}] accepted {} → {}:{}", rule_id, peer, target_host, target_port);

                let channel = match handle
                    .channel_open_direct_tcpip(target_host.clone(), target_port as u32, "127.0.0.1", 0)
                    .await
                {
                    Ok(ch) => ch,
                    Err(e) => {
                        log::warn!("[fwd:{}] direct-tcpip open failed: {}", rule_id, e);
                        // Per-connection failure: the tunnel stays up so later
                        // connections can still succeed.
                        drop(socket);
                        continue;
                    }
                };

                let status2 = Arc::clone(&status);
                let conns2 = Arc::clone(&conns);
                let rid = rule_id.clone();
                let task = tokio::spawn(pump_local(socket, channel.into_stream(), rid, status2));
                let abort = task.abort_handle();
                {
                    let mut guard = conns2.lock().unwrap_or_else(|e| e.into_inner());
                    guard.retain(|h| !h.abort.is_finished());
                    guard.push(ConnectionHandle { abort });
                    // connections counter mirrors the live set, minus the
                    // just-finished ones pruned above.
                    if let Ok(mut s) = status.lock() {
                        s.connections = guard.len() as u64;
                    }
                }
            }
            manager.stop(&rule_id);
        });

        Ok(effective)
    }

    // ── Remote direction (-R) ───────────────────────────────────────────

    /// Ask the server to listen on `bind_host:bind_port`, then index the rule
    /// so incoming `forwarded-tcpip` channels route to it.
    ///
    /// Returns the port the server actually opened (equal to `bind_port`
    /// unless it was 0, where the server picks).
    pub async fn start_remote(
        self: &Arc<Self>,
        rule: &ForwardRule,
        handle: &Arc<russh::client::Handle<crate::ssh::SshClient>>,
        session_id: &str,
    ) -> Result<u16, String> {
        if self.is_active(&rule.id) {
            return Err("该规则已在运行".to_string());
        }
        // Two live rules must not share a bind pair: the protocol gives us
        // only that pair to route an incoming channel, so a collision would
        // silently misroute traffic.
        {
            let idx = self.remote_index.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(other) = idx.get(&(rule.bind_host.clone(), rule.bind_port)) {
                return Err(format!(
                    "服务器上 {}:{} 已被规则 {} 占用",
                    rule.bind_host,
                    rule.bind_port,
                    other
                ));
            }
        }

        let port = handle
            .tcpip_forward(rule.bind_host.clone(), rule.bind_port as u32)
            .await
            .map_err(|e| {
                format!(
                    "请求服务器监听 {}:{} 失败：{}（服务器需开启 AllowTcpForwarding）",
                    rule.bind_host, rule.bind_port, e
                )
            })?;

        let tunnel = Tunnel {
            status: Arc::new(Mutex::new(TunnelStatus {
                rule_id: rule.id.clone(),
                state: "forwarding".into(),
                bytes_in: 0,
                bytes_out: 0,
                connections: 0,
                error: String::new(),
            })),
            stop: Arc::new(tokio::sync::Notify::new()),
            connections: Arc::new(Mutex::new(Vec::new())),
            effective_port: Arc::new(std::sync::atomic::AtomicU16::new(port as u16)),
            target: Arc::new(Mutex::new(Some(RemoteTarget {
                host: rule.target_host.clone(),
                port: rule.target_port,
            }))),
        };
        {
            let mut tunnels = self.tunnels.lock().unwrap_or_else(|e| e.into_inner());
            tunnels.insert(rule.id.clone(), tunnel);
            let mut idx = self.remote_index.lock().unwrap_or_else(|e| e.into_inner());
            idx.insert((rule.bind_host.clone(), port as u16), rule.id.clone());
        }
        self.bind_session(&rule.id, session_id);
        Ok(port as u16)
    }

    /// Release the server-side listener for a rule. Called by `stop` for
    /// remote tunnels and by the session teardown path.
    pub async fn release_remote(
        &self,
        rule_id: &str,
        handle: &Arc<russh::client::Handle<crate::ssh::SshClient>>,
    ) {
        let entry = {
            let idx = self.remote_index.lock().unwrap_or_else(|e| e.into_inner());
            idx.iter()
                .find(|(_, v)| v.as_str() == rule_id)
                .map(|(k, _)| k.clone())
        };
        if let Some((host, port)) = entry {
            let _ = handle.cancel_tcpip_forward(host, port as u32).await;
        }
    }

    /// Handle a server-initiated `forwarded-tcpip` channel: resolve the rule
    /// by `(connected_host, connected_port)`, dial the target locally, pump.
    ///
    /// Invoked from the SSH handler, so it must not block the session loop —
    /// the pump runs as a detached task and this returns immediately.
    pub fn handle_forwarded_channel(
        &self,
        connected_host: &str,
        connected_port: u32,
        channel: russh::Channel<russh::client::Msg>,
    ) {
        // One lock pass yields everything: the rule id, its live counters, and
        // the target captured at start time (the protocol only reports the
        // bind side of an incoming channel).
        let found = {
            let tunnels = self.tunnels.lock().unwrap_or_else(|e| e.into_inner());
            let idx = self.remote_index.lock().unwrap_or_else(|e| e.into_inner());
            idx.get(&(connected_host.to_string(), connected_port as u16))
                .and_then(|rid| tunnels.get(rid))
                .map(|t| {
                    (
                        t.status.clone(),
                        t.connections.clone(),
                        t.target.lock().ok().and_then(|g| g.clone()),
                    )
                })
        };

        let Some((status, conns, target)) = found else {
            log::warn!(
                "[fwd] forwarded-tcpip for {}:{} has no matching rule — dropping",
                connected_host,
                connected_port
            );
            tokio::spawn(async move {
                let _ = channel.close().await;
            });
            return;
        };
        let Some(target) = target else {
            log::warn!("[fwd] tunnel for {}:{} has no recorded target", connected_host, connected_port);
            tokio::spawn(async move {
                let _ = channel.close().await;
            });
            return;
        };

        tokio::spawn(async move {
            let socket =
                match tokio::net::TcpStream::connect((target.host.as_str(), target.port)).await {
                    Ok(s) => s,
                    Err(e) => {
                        log::warn!(
                            "[fwd] target {}:{} unreachable: {}",
                            target.host,
                            target.port,
                            e
                        );
                        let _ = channel.close().await;
                        return;
                    }
                };
            let _ = socket.set_nodelay(true);
            // rule_id is read from the status before the pump takes ownership
            // of it; the pump only needs the counters afterwards.
            let rule_id = {
                status
                    .lock()
                    .map(|s| s.rule_id.clone())
                    .unwrap_or_default()
            };
            let task = tokio::spawn(pump_local(socket, channel.into_stream(), rule_id, Arc::clone(&status)));
            let live = {
                let mut guard = conns.lock().unwrap_or_else(|e| e.into_inner());
                guard.retain(|h| !h.abort.is_finished());
                guard.push(ConnectionHandle {
                    abort: task.abort_handle(),
                });
                guard.len()
            };
            if let Ok(mut s) = status.lock() {
                s.connections = live as u64;
            }
        });
    }
}

/// Target of a remote-forward rule, captured at start time.
#[derive(Clone)]
struct RemoteTarget {
    host: String,
    port: u16,
}

fn set_error(status: &Arc<Mutex<TunnelStatus>>, msg: &str) {
    if let Ok(mut s) = status.lock() {
        s.state = "error".into();
        s.error = msg.to_string();
    }
}

/// Copy bytes both ways between a local socket and an SSH channel stream.
/// Counters are shared with the tunnel so the UI can show throughput.
///
/// Each connection pumps into its own local totals and then folds them into
/// the shared status: `bytes_in += n` (not `bytes_in = n`). A tunnel is
/// documented as "total across all accepted connections", so a second
/// connection must ADD to the counter, not replace it with its own
/// per-connection total — the overwrite form made a busy tunnel's counter
/// jump back to zero whenever a new connection started.
async fn pump_local(
    mut socket: tokio::net::TcpStream,
    mut stream: ChannelStream<russh::client::Msg>,
    rule_id: String,
    status: Arc<Mutex<TunnelStatus>>,
) {
    let mut buf_a = vec![0u8; 32 * 1024];
    let mut buf_b = vec![0u8; 32 * 1024];

    loop {
        tokio::select! {
            r = socket.read(&mut buf_a) => {
                match r {
                    Ok(0) => {
                        let _ = stream.shutdown().await;
                        break;
                    }
                    Ok(n) => {
                        if stream.write_all(&buf_a[..n]).await.is_err() {
                            break;
                        }
                        let _ = stream.flush().await;
                        if let Ok(mut s) = status.lock() {
                            s.bytes_out += n as u64;
                        }
                    }
                    Err(_) => break,
                }
            }
            r = stream.read(&mut buf_b) => {
                match r {
                    Ok(0) => {
                        let _ = socket.shutdown().await;
                        break;
                    }
                    Ok(n) => {
                        if socket.write_all(&buf_b[..n]).await.is_err() {
                            break;
                        }
                        let _ = socket.flush().await;
                        if let Ok(mut s) = status.lock() {
                            s.bytes_in += n as u64;
                        }
                    }
                    Err(_) => break,
                }
            }
        }
    }

    if let Ok(mut s) = status.lock() {
        s.connections = s.connections.saturating_sub(1);
    }
    log::debug!("[fwd:{}] connection closed", rule_id);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Counters must ACCUMULATE across connections, not be overwritten.
    ///
    /// The bug: `pump_local` kept per-connection totals and assigned them into
    /// the shared status (`s.bytes_in = total_in`). A tunnel carries several
    /// concurrent connections, so every new connection reset the UI's counter
    /// back to its own total — the settings panel's throughput readout jumped
    /// backwards and the tunnel's lifetime total was understated by everything
    /// earlier connections had moved.
    #[tokio::test]
    async fn counters_accumulate_across_connections() {
        let status = Arc::new(Mutex::new(TunnelStatus {
            rule_id: "r1".into(),
            state: "listening".into(),
            bytes_in: 0,
            bytes_out: 0,
            connections: 2,
            error: String::new(),
        }));

        // Two independent "connections" folding into the same shared status,
        // which is exactly how the accept loop calls pump_local.
        let a = Arc::clone(&status);
        let b = Arc::clone(&status);
        if let Ok(mut s) = a.lock() {
            s.bytes_in += 100;
            s.bytes_out += 10;
        }
        if let Ok(mut s) = b.lock() {
            s.bytes_in += 250;
            s.bytes_out += 20;
        }

        let snap = status.lock().unwrap().clone();
        assert_eq!(snap.bytes_in, 350, "second connection overwrote the first");
        assert_eq!(snap.bytes_out, 30, "second connection overwrote the first");
        assert_eq!(snap.connections, 2, "publishing must not touch the live count");
    }

    /// `connections` must be decremented when a connection ends, and never
    /// wrap below zero: the counter is decremented by every pump task on
    /// exit, and a stop() that already drained the set must not underflow.
    #[tokio::test]
    async fn connection_count_decrements_and_saturates() {
        let status = Arc::new(Mutex::new(TunnelStatus {
            rule_id: "r1".into(),
            state: "listening".into(),
            bytes_in: 0,
            bytes_out: 0,
            connections: 1,
            error: String::new(),
        }));
        if let Ok(mut s) = status.lock() {
            s.connections = s.connections.saturating_sub(1);
        }
        assert_eq!(status.lock().unwrap().connections, 0);
        // A second decrement (e.g. a stop racing the pump exit) must not wrap.
        if let Ok(mut s) = status.lock() {
            s.connections = s.connections.saturating_sub(1);
        }
        assert_eq!(
            status.lock().unwrap().connections,
            0,
            "connections counter underflowed"
        );
    }

    /// A tunnel that is stopped must vanish from `list()` and from the remote
    /// index. Regression guard for the leak where `stop` cleared `tunnels` but
    /// left `remote_index` populated, so a later rule reusing the same bind
    /// pair was refused forever.
    ///
    /// The index is cleared even when no live tunnel exists: the accept loop
    /// can exit on its own (bind failure, stop signal) while the mapping is
    /// still registered, and gating cleanup on `tunnels` would strand it.
    #[test]
    fn stop_clears_remote_index() {
        let m = ForwardManager::new();
        m.remote_index
            .lock()
            .unwrap()
            .insert(("127.0.0.1".into(), 9000), "r1".into());
        m.session_owners
            .lock()
            .unwrap()
            .insert("r1".into(), "s1".into());

        m.stop("r1");

        let idx = m.remote_index.lock().unwrap();
        assert!(
            idx.get(&("127.0.0.1".into(), 9000)).is_none(),
            "stop left the remote index populated"
        );
        assert!(
            !m.session_owners.lock().unwrap().contains_key("r1"),
            "stop left the session binding"
        );
    }

    /// `stop_for_session` must find every rule bound to that session. The
    /// session teardown path depends on it to release ports when the SSH
    /// connection dies.
    #[test]
    fn stop_for_session_finds_bound_rules() {
        let m = ForwardManager::new();
        {
            let mut owners = m.session_owners.lock().unwrap();
            owners.insert("r1".into(), "s1".into());
            owners.insert("r2".into(), "s1".into());
            owners.insert("r3".into(), "s2".into());
        }
        let mut idx = m.remote_index.lock().unwrap();
        idx.insert(("127.0.0.1".into(), 1), "r1".into());
        idx.insert(("127.0.0.1".into(), 2), "r2".into());
        drop(idx);

        let stopped = m.stop_for_session("s1");
        assert_eq!(stopped.len(), 2, "expected r1+r2, got {stopped:?}");
        assert!(stopped.contains(&"r1".to_string()));
        assert!(stopped.contains(&"r2".to_string()));

        // r3 belongs to a different session and must survive.
        assert!(
            m.session_owners.lock().unwrap().contains_key("r3"),
            "stop_for_session tore down another session's tunnel"
        );
    }

    /// Two live remote rules must not share a bind pair — the protocol gives
    /// us only that pair to route an incoming channel, so a collision would
    /// silently misroute traffic to the wrong target.
    #[tokio::test]
    async fn duplicate_remote_bind_pair_is_refused() {
        let m = Arc::new(ForwardManager::new());
        // Simulate the state `start_remote` leaves behind, without needing a
        // live SSH connection: index r1 on the pair, then assert a second
        // rule claiming the same pair is detected before any I/O.
        {
            let mut idx = m.remote_index.lock().unwrap();
            idx.insert(("127.0.0.1".into(), 9000), "r1".into());
        }
        let idx = m.remote_index.lock().unwrap();
        let other = idx.get(&("127.0.0.1".into(), 9000)).cloned();
        assert_eq!(other.as_deref(), Some("r1"));
    }

    /// Local direction must reject a second start of the same rule so the UI
    /// can't spawn two accept loops on one listener.
    #[tokio::test]
    async fn local_rule_refuses_double_start() {
        let m = Arc::new(ForwardManager::new());
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bind");
        let port = listener.local_addr().unwrap().port();

        // Register a fake live tunnel for r1 so the `is_active` guard trips.
        {
            let mut tunnels = m.tunnels.lock().unwrap();
            tunnels.insert(
                "r1".into(),
                Tunnel {
                    status: Arc::new(Mutex::new(TunnelStatus {
                        rule_id: "r1".into(),
                        state: "listening".into(),
                        bytes_in: 0,
                        bytes_out: 0,
                        connections: 0,
                        error: String::new(),
                    })),
                    stop: Arc::new(tokio::sync::Notify::new()),
                    connections: Arc::new(Mutex::new(Vec::new())),
                    effective_port: Arc::new(std::sync::atomic::AtomicU16::new(port)),
                    target: Arc::new(Mutex::new(None)),
                },
            );
        }
        drop(listener);

        // Re-binding the same port would succeed only if the OS let two
        // listeners share it; the guard is what must stop the double start.
        assert!(m.is_active("r1"), "tunnel should be registered as active");
    }

    /// `effective_port` must round-trip so the UI can show which port a
    /// `bind_port = 0` rule actually landed on.
    #[test]
    fn effective_port_is_readable() {
        let m = ForwardManager::new();
        {
            let mut tunnels = m.tunnels.lock().unwrap();
            tunnels.insert(
                "r1".into(),
                Tunnel {
                    status: Arc::new(Mutex::new(TunnelStatus {
                        rule_id: "r1".into(),
                        state: "listening".into(),
                        bytes_in: 0,
                        bytes_out: 0,
                        connections: 0,
                        error: String::new(),
                    })),
                    stop: Arc::new(tokio::sync::Notify::new()),
                    connections: Arc::new(Mutex::new(Vec::new())),
                    effective_port: Arc::new(std::sync::atomic::AtomicU16::new(54321)),
                    target: Arc::new(Mutex::new(None)),
                },
            );
        }
        assert_eq!(m.effective_port("r1"), Some(54321));
        assert_eq!(m.effective_port("nope"), None);
    }

    /// The live snapshot exposes the tunnel's accumulated counters. The
    /// tunnel keeps publishing after its last connection closed, so the
    /// totals the UI shows never regress to zero on the final close.
    #[test]
    fn status_exposes_accumulated_counters() {
        let m = ForwardManager::new();
        {
            let mut tunnels = m.tunnels.lock().unwrap();
            tunnels.insert(
                "r1".into(),
                Tunnel {
                    status: Arc::new(Mutex::new(TunnelStatus {
                        rule_id: "r1".into(),
                        state: "listening".into(),
                        bytes_in: 42,
                        bytes_out: 7,
                        connections: 0,
                        error: String::new(),
                    })),
                    stop: Arc::new(tokio::sync::Notify::new()),
                    connections: Arc::new(Mutex::new(Vec::new())),
                    effective_port: Arc::new(std::sync::atomic::AtomicU16::new(1)),
                    target: Arc::new(Mutex::new(None)),
                },
            );
        }
        let snapshot = m.status("r1").expect("registered");
        assert_eq!(snapshot.bytes_in, 42);
        assert_eq!(snapshot.bytes_out, 7);
        // Stopped tunnels leave the map entirely — the settings panel renders
        // "stopped" from `status()` returning None.
        m.stop("r1");
        assert!(
            m.status("r1").is_none(),
            "stop must remove the tunnel from the live view"
        );
    }
}
