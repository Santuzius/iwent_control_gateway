// src/main.rs
use std::sync::{Arc, RwLock};
use tokio::signal; // For graceful shutdown on Ctrl+C

mod can;
mod data;
mod error;
mod modbus_server;

use data::BmsData;
use error::AppError; // Import the AppError type

#[tokio::main]
async fn main() -> Result<(), AppError> {
    env_logger::init();

    log::info!("Application starting...");

    // Create shared data structures with thread-safe access
    // Initialized to None, as no data has been received yet
    let bms_data1: Arc<RwLock<Option<BmsData>>> = Arc::new(RwLock::new(Some(BmsData {
        min_cell_voltage: Some(0),
        max_cell_voltage: Some(0),
        min_temperature: Some(0),
        max_temperature: Some(0),
        info: Some(0xFF),
        soc: Some(0),
        current: Some(0),
        total_voltage: Some(0),
        warning1: Some(0),
        warning2: Some(0),
        error1: Some(0xFF),
        error2: Some(0xFF),
        on: Some(0),
        quit: Some(0),
    })));

    let bms_data2: Arc<RwLock<Option<BmsData>>> = Arc::new(RwLock::new(Some(BmsData {
        min_cell_voltage: Some(0),
        max_cell_voltage: Some(0),
        min_temperature: Some(0),
        max_temperature: Some(0),
        info: Some(0xFF),
        soc: Some(0),
        current: Some(0),
        total_voltage: Some(0),
        warning1: Some(0),
        warning2: Some(0),
        error1: Some(0xFF),
        error2: Some(0xFF),
        on: Some(0),
        quit: Some(0),
    })));

    // --- Spawn asynchronous tasks ---

    // CAN Receiver tasks
    let can_rx1_handle = tokio::spawn(can::can_rx(
        1, 
        Arc::clone(&bms_data1)
    ));
    let can_rx2_handle = tokio::spawn(can::can_rx(
        2, 
        Arc::clone(&bms_data2)
    ));

    // CAN Transmitter task (placeholder)
    let can_tx_handle = tokio::spawn(can::can_tx());

    // Modbus Server tasks
    let modbus1_handle = tokio::spawn(modbus_server::server(
        "172.18.143.93:40502", // Address for BMS 1
        Arc::clone(&bms_data1),
    ));
    let modbus2_handle = tokio::spawn(modbus_server::server(
        "172.18.143.93:41502", // Address for BMS 2
        Arc::clone(&bms_data2),
    ));

    log::info!("All tasks spawned. Waiting for Ctrl+C to exit.");

    // --- Graceful Shutdown Handling ---
    // Wait for Ctrl+C signal
    match signal::ctrl_c().await {
        Ok(()) => {
            log::info!("Ctrl+C received. Shutting down tasks...");
        }
        Err(err) => {
            log::error!("Failed to listen for Ctrl+C signal: {}", err);
            // Continue with shutdown anyway
        }
    }

    // --- Optional: Wait for tasks to complete (or abort them) ---
    // Aborting is simpler if tasks run in infinite loops
    can_rx1_handle.abort();
    can_rx2_handle.abort();
    can_tx_handle.abort();
    modbus1_handle.abort();
    modbus2_handle.abort();

    // Alternatively, you could signal tasks to stop and then .await their handles
    // For example, using tokio::sync::broadcast channels or shared AtomicBool flags.

    log::info!("Application finished.");
    Ok(())
}