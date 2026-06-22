use crate::{error_string, random_id, string_arg, ActiveTransfer, AppState};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};
use tauri::Emitter;

pub(super) struct TransferReporter {
    window: tauri::Window,
    cancellations: Arc<Mutex<HashSet<String>>>,
    active_transfers: Arc<Mutex<HashMap<String, ActiveTransfer>>>,
    state: Arc<Mutex<TransferReporterState>>,
}

struct TransferReporterState {
    connection_id: String,
    queue_id: String,
    client_id: Option<String>,
    transfer_type: String,
    file_name: String,
    transferred: u64,
    total: u64,
    current_file_transferred: u64,
    current_file_total: u64,
    completed_files: u64,
    total_files: u64,
    completed_items: u64,
    total_items: u64,
    started: bool,
    ended: bool,
    registered: bool,
}

impl TransferReporter {
    pub(super) fn new(
        app_state: &AppState,
        window: &tauri::Window,
        connection_id: &str,
        transfer_type: &str,
        options: Option<&Value>,
        file_name: String,
    ) -> Self {
        let client_id = options
            .and_then(|value| {
                value
                    .get("transferClientId")
                    .or_else(|| value.get("clientId"))
            })
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string);
        let queue_id = options
            .and_then(|value| value.get("queueId"))
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
            .unwrap_or_else(|| random_id("transfer"));

        Self {
            window: window.clone(),
            cancellations: app_state.transfer_cancellations.clone(),
            active_transfers: app_state.active_transfers.clone(),
            state: Arc::new(Mutex::new(TransferReporterState {
                connection_id: connection_id.to_string(),
                queue_id,
                client_id,
                transfer_type: transfer_type.to_string(),
                file_name,
                transferred: 0,
                total: 0,
                current_file_transferred: 0,
                current_file_total: 0,
                completed_files: 0,
                total_files: 0,
                completed_items: 0,
                total_items: 0,
                started: false,
                ended: false,
                registered: false,
            })),
        }
    }

    pub(super) fn set_totals(&self, total: u64, total_files: u64, total_items: u64) {
        self.register_active();
        if let Ok(mut state) = self.state.lock() {
            state.total = total;
            state.total_files = total_files;
            state.total_items = total_items;
            state.started = true;
        }
        self.emit_progress();
    }

    pub(super) fn start_file(&self, file_name: &str, current_file_total: u64) {
        self.register_active();
        if let Ok(mut state) = self.state.lock() {
            state.file_name = file_name.to_string();
            state.current_file_transferred = 0;
            state.current_file_total = current_file_total;
            if state.total == 0 && current_file_total > 0 {
                state.total = current_file_total;
            }
            state.started = true;
        }
        self.emit_progress();
    }

    pub(super) fn add_bytes(&self, bytes: u64) {
        self.register_active();
        if let Ok(mut state) = self.state.lock() {
            state.transferred = state.transferred.saturating_add(bytes);
            state.current_file_transferred = state.current_file_transferred.saturating_add(bytes);
            if state.total < state.transferred {
                state.total = state.transferred;
            }
            if state.current_file_total < state.current_file_transferred {
                state.current_file_total = state.current_file_transferred;
            }
            state.started = true;
        }
        self.emit_progress();
    }

    pub(super) fn complete_file(&self) {
        self.register_active();
        if let Ok(mut state) = self.state.lock() {
            state.completed_files = state.completed_files.saturating_add(1);
            state.completed_items = state.completed_items.saturating_add(1);
            if state.current_file_total > 0 {
                state.current_file_transferred = state.current_file_total;
            }
            state.started = true;
        }
        self.emit_progress();
    }

    pub(super) fn check_canceled(&self) -> Result<(), String> {
        self.register_active();
        let (queue_id, client_id) = self.ids();
        let canceled = self
            .cancellations
            .lock()
            .map_err(error_string)?
            .iter()
            .any(|id| {
                id == &queue_id || client_id.as_ref().is_some_and(|client_id| id == client_id)
            });
        if canceled {
            let message = "传输已取消。".to_string();
            self.finish(false, Some(&message));
            return Err(message);
        }
        Ok(())
    }

    pub(super) fn finish(&self, success: bool, error: Option<&str>) {
        let mut payload = None;
        let mut ids = None;
        if let Ok(mut state) = self.state.lock() {
            if state.ended {
                return;
            }
            state.ended = true;
            ids = Some((state.queue_id.clone(), state.client_id.clone()));
            payload = Some(state.payload(success, error));
        }
        if let Some((queue_id, client_id)) = ids {
            self.unregister(&queue_id, client_id.as_deref());
        }
        if let Some(payload) = payload {
            let _ = self.window.emit("transfer:end", payload);
        }
    }

    fn register_active(&self) {
        let mut registration = None;
        if let Ok(mut state) = self.state.lock() {
            if state.ended || state.registered {
                return;
            }
            state.registered = true;
            registration = Some((
                state.queue_id.clone(),
                ActiveTransfer {
                    connection_id: state.connection_id.clone(),
                    client_id: state.client_id.clone(),
                },
            ));
        }
        if let Some((queue_id, transfer)) = registration {
            if let Ok(mut active_transfers) = self.active_transfers.lock() {
                active_transfers.insert(queue_id, transfer);
            }
        }
    }

    fn unregister(&self, queue_id: &str, client_id: Option<&str>) {
        if let Ok(mut cancellations) = self.cancellations.lock() {
            cancellations.remove(queue_id);
            if let Some(client_id) = client_id {
                cancellations.remove(client_id);
            }
        }
        if let Ok(mut active_transfers) = self.active_transfers.lock() {
            active_transfers.remove(queue_id);
        }
    }

    fn emit_progress(&self) {
        if let Ok(state) = self.state.lock() {
            if state.ended {
                return;
            }
            let _ = self
                .window
                .emit("transfer:progress", state.payload(false, None));
        }
    }

    fn ids(&self) -> (String, Option<String>) {
        self.state
            .lock()
            .map(|state| (state.queue_id.clone(), state.client_id.clone()))
            .unwrap_or_else(|_| (String::new(), None))
    }
}

impl Drop for TransferReporter {
    fn drop(&mut self) {
        let mut ids = None;
        let should_finish = self
            .state
            .lock()
            .map(|state| {
                ids = Some((state.queue_id.clone(), state.client_id.clone()));
                state.started && !state.ended
            })
            .unwrap_or(false);
        if should_finish {
            self.finish(false, Some("传输中断。"));
        } else if let Some((queue_id, client_id)) = ids {
            self.unregister(&queue_id, client_id.as_deref());
        }
    }
}

impl TransferReporterState {
    fn payload(&self, success: bool, error: Option<&str>) -> Value {
        let mut payload = json!({
            "connectionId": self.connection_id,
            "queueId": self.queue_id,
            "type": self.transfer_type,
            "fileName": self.file_name,
            "transferred": self.transferred,
            "total": self.total,
            "currentFileTransferred": self.current_file_transferred,
            "currentFileTotal": self.current_file_total,
            "completedFiles": self.completed_files,
            "totalFiles": self.total_files,
            "completedItems": self.completed_items,
            "totalItems": self.total_items,
            "success": success,
        });
        if let Some(client_id) = &self.client_id {
            payload["clientId"] = json!(client_id);
        }
        if let Some(error) = error {
            payload["error"] = json!(error);
        }
        payload
    }
}

pub(crate) fn cancel_transfer(state: &AppState, args: Vec<Value>) -> Result<Value, String> {
    let connection_id = string_arg(&args, 0)?;
    let requested_id = args.get(1).and_then(Value::as_str).unwrap_or("").trim();
    if requested_id.is_empty() {
        return cancel_transfers_for_connection(state, &connection_id)
            .map(|canceled| json!(canceled));
    }
    let mut cancel_ids = Vec::new();
    {
        let active_transfers = state.active_transfers.lock().map_err(error_string)?;
        if let Some((queue_id, transfer)) = active_transfers.iter().find(|(queue_id, transfer)| {
            transfer.connection_id == connection_id
                && (*queue_id == requested_id
                    || transfer
                        .client_id
                        .as_deref()
                        .is_some_and(|client_id| client_id == requested_id))
        }) {
            cancel_ids.push(queue_id.clone());
            if let Some(client_id) = &transfer.client_id {
                cancel_ids.push(client_id.clone());
            }
        }
    }
    if cancel_ids.is_empty() {
        return Ok(json!(false));
    }
    let mut cancellations = state.transfer_cancellations.lock().map_err(error_string)?;
    for id in cancel_ids {
        cancellations.insert(id);
    }
    Ok(json!(true))
}

pub(crate) fn cancel_transfers_for_connection(
    state: &AppState,
    connection_id: &str,
) -> Result<bool, String> {
    let mut cancel_ids = Vec::new();
    {
        let active_transfers = state.active_transfers.lock().map_err(error_string)?;
        for (queue_id, transfer) in active_transfers.iter() {
            if transfer.connection_id == connection_id {
                cancel_ids.push(queue_id.clone());
                if let Some(client_id) = &transfer.client_id {
                    cancel_ids.push(client_id.clone());
                }
            }
        }
    }
    if cancel_ids.is_empty() {
        return Ok(false);
    }
    let mut cancellations = state.transfer_cancellations.lock().map_err(error_string)?;
    for id in cancel_ids {
        cancellations.insert(id);
    }
    Ok(true)
}
